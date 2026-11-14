//! Backpressured customer exports. Never collect the complete customer directory.
use super::litellm_spend::{customer_context, customer_dates, CustomerSpend, CustomerSpendContext};
use super::*;
use std::collections::BTreeSet;
use std::sync::LazyLock;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

const BATCH_SIZE: usize = 100;
const BATCH_TIMEOUT: Duration = Duration::from_secs(5);
const CSV_HEADER: &str = "customer_id,spend_usd,requests,start_date,end_date,key_id\n";
static EXPORTS: LazyLock<Arc<Semaphore>> = LazyLock::new(|| Arc::new(Semaphore::new(2)));

#[derive(Deserialize)]
pub(super) struct CustomerExportQuery {
    start_date: Option<String>,
    end_date: Option<String>,
}

#[derive(Clone, Copy)]
struct ExportMetadata {
    key_id: uuid::Uuid,
    start_date: chrono::NaiveDate,
    end_date: chrono::NaiveDate,
}

pub(super) async fn customer_spend_export(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(key_id): Path<uuid::Uuid>,
    Query(query): Query<CustomerExportQuery>,
) -> Response {
    let mut response = export(state, &headers, key_id, query).await;
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

async fn export(
    state: AppState,
    headers: &HeaderMap,
    key_id: uuid::Uuid,
    query: CustomerExportQuery,
) -> Response {
    if let Err(response) = require_admin_scope(&state, headers, SCOPE_USAGE_EXPORT).await {
        return response;
    }
    let (start_date, end_date) =
        match customer_dates(query.start_date.as_deref(), query.end_date.as_deref()) {
            Ok(dates) => dates,
            Err(error) => return error_response(headers, error),
        };
    let permit = match EXPORTS.clone().try_acquire_owned() {
        Ok(permit) => permit,
        Err(_) => return error_response(headers, GatewayError::GatewayOverloaded),
    };
    let key = match state.store.get_admin_key(key_id).await {
        Ok(Some(key)) => key,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(error) => return error_response(headers, error),
    };
    let mapping = match state
        .store
        .litellm_credential_mapping_for_context(key_id, key.project_id)
        .await
    {
        Ok(Some(mapping)) => mapping,
        Ok(None) => {
            return export_error(
                headers,
                StatusCode::CONFLICT,
                "litellm_customer_mapping_missing",
                "Configure an enabled LiteLLM credential mapping before exporting customer usage.",
            );
        }
        Err(error) => return error_response(headers, error),
    };
    // Freeze the provider and mapped-key context once for the complete download.
    let context = match customer_context(&state, &mapping.credential).await {
        Ok(context) => context,
        Err(()) => return unavailable(headers),
    };
    let metadata = ExportMetadata {
        key_id,
        start_date,
        end_date,
    };
    let source = LiteLlmExportSource {
        state,
        context,
        metadata,
    };
    streaming_response(source, metadata, permit, headers).await
}

fn export_error(
    headers: &HeaderMap,
    status: StatusCode,
    code: &'static str,
    message: &'static str,
) -> Response {
    (
        status,
        Json(serde_json::json!({"error": {
            "code": code,
            "message": message,
            "request_id": request_id_from_headers(headers)
        }})),
    )
        .into_response()
}

fn unavailable(headers: &HeaderMap) -> Response {
    export_error(
        headers,
        StatusCode::BAD_GATEWAY,
        "litellm_customer_export_unavailable",
        "LiteLLM customer usage export is unavailable. Retry or choose a shorter date range.",
    )
}

struct ExportBatch {
    ids: Vec<String>,
    customers: Vec<CustomerSpend>,
}

#[async_trait]
trait ExportSource: Send + Sync + 'static {
    async fn batch(&self, page: usize) -> Result<ExportBatch, ()>;
}

struct LiteLlmExportSource {
    state: AppState,
    context: CustomerSpendContext,
    metadata: ExportMetadata,
}

#[async_trait]
impl ExportSource for LiteLlmExportSource {
    async fn batch(&self, page: usize) -> Result<ExportBatch, ()> {
        let ids = self
            .context
            .directory(&self.state, page, BATCH_SIZE)
            .await?;
        let customers = self
            .context
            .costs(
                &self.state,
                &ids,
                self.metadata.start_date,
                self.metadata.end_date,
            )
            .await?;
        Ok(ExportBatch { ids, customers })
    }
}

struct CustomerExportStream<S> {
    source: S,
    metadata: ExportMetadata,
    page: usize,
    previous_ids: BTreeSet<String>,
    pending: Option<Bytes>,
    done: bool,
    _permit: OwnedSemaphorePermit,
}

async fn next_batch<S: ExportSource>(
    source: &S,
    page: usize,
    previous_ids: &BTreeSet<String>,
) -> Result<ExportBatch, ()> {
    let batch = tokio::time::timeout(BATCH_TIMEOUT, source.batch(page))
        .await
        .map_err(|_| ())??;
    if batch.ids.len() > BATCH_SIZE || batch.ids.iter().any(|id| previous_ids.contains(id)) {
        return Err(());
    }
    Ok(batch)
}

async fn streaming_response<S: ExportSource>(
    source: S,
    metadata: ExportMetadata,
    permit: OwnedSemaphorePermit,
    headers: &HeaderMap,
) -> Response {
    // Fail before committing CSV headers if the first upstream batch is unavailable.
    let first = match next_batch(&source, 1, &BTreeSet::new()).await {
        Ok(batch) => batch,
        Err(()) => return unavailable(headers),
    };
    let mut prefix = CSV_HEADER.to_owned();
    prefix.push_str(&csv_rows(&first.customers, metadata));
    let stream = futures_util::stream::try_unfold(
        CustomerExportStream {
            source,
            metadata,
            page: 1,
            done: first.ids.len() < BATCH_SIZE,
            previous_ids: first.ids.into_iter().collect(),
            pending: Some(Bytes::from(prefix)),
            _permit: permit,
        },
        |mut stream| async move {
            if let Some(chunk) = stream.pending.take() {
                return Ok(Some((chunk, stream)));
            }
            if stream.done {
                return Ok(None);
            }
            let error = || std::io::Error::other("LiteLLM customer usage export batch unavailable");
            stream.page = stream.page.checked_add(1).ok_or_else(error)?;
            let batch = next_batch(&stream.source, stream.page, &stream.previous_ids)
                .await
                .map_err(|()| error())?;
            stream.done = batch.ids.len() < BATCH_SIZE;
            stream.previous_ids = batch.ids.into_iter().collect();
            let chunk = csv_rows(&batch.customers, stream.metadata);
            Ok::<_, std::io::Error>(Some((Bytes::from(chunk), stream)))
        },
    );
    (
        StatusCode::OK,
        [
            ("content-type", "text/csv; charset=utf-8"),
            (
                "content-disposition",
                "attachment; filename=\"relayna-litellm-customer-usage.csv\"",
            ),
            ("cache-control", "no-store"),
        ],
        Body::from_stream(stream),
    )
        .into_response()
}

fn csv_rows(customers: &[CustomerSpend], metadata: ExportMetadata) -> String {
    let mut csv = String::new();
    for customer in customers {
        let mut id = customer.customer_id.replace(['\r', '\n'], " ");
        // Neutralize formula markers even when preceded by whitespace.
        if id.trim_start().starts_with(['=', '+', '-', '@']) {
            id.insert(0, '\'');
        }
        csv.push_str(&csv_escape(id));
        csv.push_str(&format!(
            ",{},{},{},{},{}\n",
            customer.spend_usd,
            customer.requests,
            metadata.start_date,
            metadata.end_date,
            metadata.key_id,
        ));
    }
    csv
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::StreamExt;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    fn metadata() -> ExportMetadata {
        ExportMetadata {
            key_id: uuid::Uuid::nil(),
            start_date: chrono::NaiveDate::from_ymd_opt(2026, 11, 1).unwrap(),
            end_date: chrono::NaiveDate::from_ymd_opt(2026, 11, 13).unwrap(),
        }
    }

    async fn permit() -> OwnedSemaphorePermit {
        Arc::new(Semaphore::new(1)).acquire_owned().await.unwrap()
    }

    struct DirectorySource {
        count: usize,
        reads: Arc<AtomicUsize>,
        fail_page: Option<usize>,
        repeat_page: bool,
        empty_activity: bool,
    }

    impl DirectorySource {
        fn new(count: usize) -> Self {
            Self {
                count,
                reads: Arc::new(AtomicUsize::new(0)),
                fail_page: None,
                repeat_page: false,
                empty_activity: false,
            }
        }
    }

    #[async_trait]
    impl ExportSource for DirectorySource {
        async fn batch(&self, page: usize) -> Result<ExportBatch, ()> {
            self.reads.fetch_add(1, Ordering::SeqCst);
            if self.fail_page == Some(page) {
                return Err(());
            }
            let start = if self.repeat_page {
                0
            } else {
                (page - 1) * BATCH_SIZE
            };
            let ids: Vec<_> = (start..(start + BATCH_SIZE).min(self.count))
                .map(|id| format!("customer-{id:07}"))
                .collect();
            let customers = if self.empty_activity {
                Vec::new()
            } else {
                ids.iter()
                    .map(|id| CustomerSpend {
                        customer_id: id.clone(),
                        spend_usd: 0.00001234,
                        requests: 2,
                    })
                    .collect()
            };
            Ok(ExportBatch { ids, customers })
        }
    }

    #[tokio::test]
    async fn export_streams_every_page_beyond_the_former_thousand_customer_limit() {
        let source = DirectorySource::new(1201);
        let reads = source.reads.clone();
        let response =
            streaming_response(source, metadata(), permit().await, &HeaderMap::new()).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        assert_eq!(reads.load(Ordering::SeqCst), 1);
        let mut chunks = response.into_body().into_data_stream();
        let first = chunks.next().await.unwrap().unwrap();
        assert!(first.starts_with(CSV_HEADER.as_bytes()));
        assert_eq!(
            reads.load(Ordering::SeqCst),
            1,
            "no prefetch while the client has not requested another chunk"
        );
        let mut rows = first.iter().filter(|byte| **byte == b'\n').count() - 1;
        while let Some(chunk) = chunks.next().await {
            let chunk = chunk.unwrap();
            rows += chunk.iter().filter(|byte| **byte == b'\n').count();
        }
        assert_eq!(rows, 1201);
        assert_eq!(reads.load(Ordering::SeqCst), 13);
    }

    #[tokio::test]
    async fn million_customer_export_streams_every_row_with_bounded_chunks() {
        let started = std::time::Instant::now();
        let source = DirectorySource::new(1_000_000);
        let reads = source.reads.clone();
        let response =
            streaming_response(source, metadata(), permit().await, &HeaderMap::new()).await;
        let mut chunks = response.into_body().into_data_stream();
        let mut rows = 0;
        let mut bytes = 0;
        let mut largest_chunk = 0;
        while let Some(chunk) = chunks.next().await {
            let chunk = chunk.unwrap();
            let lines = chunk.iter().filter(|byte| **byte == b'\n').count();
            assert!(lines <= BATCH_SIZE + 1);
            rows += lines;
            bytes += chunk.len();
            largest_chunk = largest_chunk.max(chunk.len());
        }
        assert_eq!(rows - 1, 1_000_000);
        assert_eq!(reads.load(Ordering::SeqCst), 10_001);
        eprintln!(
            "synthetic customer export: rows={}, bytes={bytes}, largest_chunk={largest_chunk}, elapsed_ms={}",
            rows - 1,
            started.elapsed().as_millis()
        );
    }

    #[tokio::test]
    async fn million_customer_source_fetches_only_demanded_batches_and_releases_permit_on_drop() {
        let source = DirectorySource::new(1_000_000);
        let reads = source.reads.clone();
        let semaphore = Arc::new(Semaphore::new(1));
        let permit = semaphore.clone().acquire_owned().await.unwrap();
        let response = streaming_response(source, metadata(), permit, &HeaderMap::new()).await;
        let mut chunks = response.into_body().into_data_stream();
        assert_eq!(reads.load(Ordering::SeqCst), 1);
        assert!(chunks.next().await.unwrap().is_ok());
        assert_eq!(reads.load(Ordering::SeqCst), 1);
        assert!(chunks.next().await.unwrap().is_ok());
        assert_eq!(reads.load(Ordering::SeqCst), 2);
        assert_eq!(semaphore.available_permits(), 0);
        drop(chunks);
        assert_eq!(reads.load(Ordering::SeqCst), 2);
        assert_eq!(semaphore.available_permits(), 1);
    }

    #[tokio::test]
    async fn failed_first_batch_is_an_http_error_and_later_failure_is_a_stream_error() {
        let mut source = DirectorySource::new(300);
        source.fail_page = Some(1);
        let response =
            streaming_response(source, metadata(), permit().await, &HeaderMap::new()).await;
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        assert_ne!(
            response.headers()[header::CONTENT_TYPE],
            "text/csv; charset=utf-8"
        );

        let mut source = DirectorySource::new(300);
        source.fail_page = Some(2);
        let response =
            streaming_response(source, metadata(), permit().await, &HeaderMap::new()).await;
        assert_eq!(response.status(), StatusCode::OK);
        let mut chunks = response.into_body().into_data_stream();
        assert!(chunks.next().await.unwrap().is_ok());
        let error = chunks.next().await.unwrap().unwrap_err().to_string();
        assert!(error.contains("export batch unavailable"));
        assert!(
            chunks.next().await.is_none(),
            "failed scans must not finish as a successful CSV"
        );
    }

    #[tokio::test]
    async fn empty_activity_does_not_stop_directory_enumeration_or_emit_unproven_rows() {
        let mut source = DirectorySource::new(201);
        source.empty_activity = true;
        let reads = source.reads.clone();
        let response =
            streaming_response(source, metadata(), permit().await, &HeaderMap::new()).await;
        let bytes = axum::body::to_bytes(response.into_body(), 1024)
            .await
            .unwrap();
        assert_eq!(bytes, CSV_HEADER);
        assert_eq!(reads.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn repeated_directory_page_fails_instead_of_looping_or_duplicating_rows() {
        let mut source = DirectorySource::new(500);
        source.repeat_page = true;
        let reads = source.reads.clone();
        let response =
            streaming_response(source, metadata(), permit().await, &HeaderMap::new()).await;
        let mut chunks = response.into_body().into_data_stream();
        assert!(chunks.next().await.unwrap().is_ok());
        assert!(chunks.next().await.unwrap().is_err());
        assert_eq!(reads.load(Ordering::SeqCst), 2);
    }

    struct CancellationSource {
        started: Arc<tokio::sync::Notify>,
        cancelled: Arc<AtomicBool>,
    }

    struct CancelledOnDrop(Arc<AtomicBool>);
    impl Drop for CancelledOnDrop {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    #[async_trait]
    impl ExportSource for CancellationSource {
        async fn batch(&self, page: usize) -> Result<ExportBatch, ()> {
            if page == 1 {
                return DirectorySource::new(BATCH_SIZE).batch(page).await;
            }
            let _cancel = CancelledOnDrop(self.cancelled.clone());
            self.started.notify_one();
            std::future::pending().await
        }
    }

    #[tokio::test]
    async fn disconnect_drops_the_inflight_upstream_batch() {
        let started = Arc::new(tokio::sync::Notify::new());
        let cancelled = Arc::new(AtomicBool::new(false));
        let source = CancellationSource {
            started: started.clone(),
            cancelled: cancelled.clone(),
        };
        let response =
            streaming_response(source, metadata(), permit().await, &HeaderMap::new()).await;
        let mut chunks = response.into_body().into_data_stream();
        assert!(chunks.next().await.unwrap().is_ok());
        {
            let read = chunks.next();
            tokio::pin!(read);
            tokio::select! {
                _ = &mut read => panic!("second batch should stay pending"),
                _ = started.notified() => {},
            }
        }
        assert!(!cancelled.load(Ordering::SeqCst));
        drop(chunks);
        assert!(cancelled.load(Ordering::SeqCst));
    }

    #[test]
    fn customer_csv_escapes_quotes_newlines_and_spreadsheet_formulas_without_rounding_costs() {
        for (id, expected) in [
            ("=1+1", "'=1+1"),
            ("  @SUM(1)", "'  @SUM(1)"),
            ("+123", "'+123"),
            ("-123", "'-123"),
            ("a\"b,c", "\"a\"\"b,c\""),
            ("a\r\nb", "a  b"),
        ] {
            let row = csv_rows(
                &[CustomerSpend {
                    customer_id: id.into(),
                    spend_usd: 0.00001234,
                    requests: 7,
                }],
                metadata(),
            );
            assert!(
                row.starts_with(&format!("{expected},0.00001234,7,")),
                "{row}"
            );
            assert_eq!(row.matches('\n').count(), 1);
        }
    }
}
