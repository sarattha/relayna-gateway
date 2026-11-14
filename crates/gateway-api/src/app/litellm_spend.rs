//! Read-only spend snapshots. Never merge LiteLLM's resettable counter into usage.
use super::*;
use gateway_core::LiteLlmCredentialMappingScope;
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

#[derive(Serialize)]
struct SpendSnapshot {
    key_id: Uuid,
    status: &'static str,
    mapping_scope: Option<LiteLlmCredentialMappingScope>,
    spend_usd: Option<f64>,
    fetched_at: Option<chrono::DateTime<Utc>>,
}

const CUSTOMER_DEFAULT_PAGE_SIZE: usize = 50;
const CUSTOMER_MAX_PAGE_SIZE: usize = 100;
const CUSTOMER_MAX_DIRECTORY_PAGE: usize = 1_000_000;
const CUSTOMER_MAX_QUERY_BYTES: usize = 16 * 1024;
const CUSTOMER_MAX_BODY_BYTES: usize = 2 * 1024 * 1024;

#[derive(Default, Deserialize)]
pub(super) struct CustomerSpendQuery {
    start_date: Option<String>,
    end_date: Option<String>,
    page: Option<usize>,
    page_size: Option<usize>,
    customer_id: Option<String>,
}

struct CustomerSpendOptions {
    start_date: chrono::NaiveDate,
    end_date: chrono::NaiveDate,
    page: usize,
    page_size: usize,
    customer_id: Option<String>,
}

fn valid_customer_id(value: &str) -> bool {
    !value.trim().is_empty()
        && value.len() <= 1024
        && !value.contains(',')
        && !value.chars().any(char::is_control)
}

impl CustomerSpendQuery {
    fn validate(&self) -> GatewayResult<CustomerSpendOptions> {
        let (start_date, end_date) = self.dates()?;
        let page = self.page.unwrap_or(1);
        let page_size = self.page_size.unwrap_or(CUSTOMER_DEFAULT_PAGE_SIZE);
        if !(1..=CUSTOMER_MAX_DIRECTORY_PAGE).contains(&page)
            || !(1..=CUSTOMER_MAX_PAGE_SIZE).contains(&page_size)
        {
            return Err(GatewayError::InvalidUsageQuery);
        }
        let customer_id = self.customer_id.as_deref().map(str::trim);
        if customer_id.is_some_and(|value| !valid_customer_id(value)) {
            return Err(GatewayError::InvalidUsageQuery);
        }
        Ok(CustomerSpendOptions {
            start_date,
            end_date,
            page: if customer_id.is_some() { 1 } else { page },
            page_size,
            customer_id: customer_id.map(str::to_owned),
        })
    }

    fn dates(&self) -> GatewayResult<(chrono::NaiveDate, chrono::NaiveDate)> {
        customer_dates(self.start_date.as_deref(), self.end_date.as_deref())
    }
}

pub(super) fn customer_dates(
    start_date: Option<&str>,
    end_date: Option<&str>,
) -> GatewayResult<(chrono::NaiveDate, chrono::NaiveDate)> {
    let parse = |value: Option<&str>| {
        let value = value.ok_or(GatewayError::InvalidUsageQuery)?;
        let parsed = chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d")
            .map_err(|_| GatewayError::InvalidUsageQuery)?;
        if value.len() != 10 || parsed.to_string() != value {
            return Err(GatewayError::InvalidUsageQuery);
        }
        Ok(parsed)
    };
    let start = parse(start_date)?;
    let end = parse(end_date)?;
    if !(0..366).contains(&(end - start).num_days()) {
        return Err(GatewayError::InvalidUsageQuery);
    }
    Ok((start, end))
}

#[derive(Serialize)]
pub(super) struct CustomerSpend {
    pub(super) customer_id: String,
    pub(super) spend_usd: f64,
    pub(super) requests: u64,
}

#[derive(Serialize)]
struct CustomerSpendSnapshot {
    key_id: Uuid,
    status: &'static str,
    mapping_scope: Option<LiteLlmCredentialMappingScope>,
    customers: Vec<CustomerSpend>,
    page: usize,
    page_size: usize,
    has_more: bool,
    customers_checked: usize,
    customer_id: Option<String>,
    start_date: chrono::NaiveDate,
    end_date: chrono::NaiveDate,
    fetched_at: Option<chrono::DateTime<Utc>>,
}

pub(super) async fn customer_spend(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(key_id): Path<Uuid>,
    Query(query): Query<CustomerSpendQuery>,
) -> Response {
    if let Err(response) = require_admin_scope(&state, &headers, SCOPE_USAGE_READ).await {
        return response;
    }
    let mut response = match query.validate() {
        Err(error) => error_response(&headers, error),
        Ok(options) => match customer_snapshot(&state, key_id, &options).await {
            Ok(Some(value)) => Json(value).into_response(),
            Ok(None) => StatusCode::NOT_FOUND.into_response(),
            Err(error) => error_response(&headers, error),
        },
    };
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

async fn customer_snapshot(
    state: &AppState,
    key_id: Uuid,
    options: &CustomerSpendOptions,
) -> GatewayResult<Option<CustomerSpendSnapshot>> {
    let Some(key) = state.store.get_admin_key(key_id).await? else {
        return Ok(None);
    };
    let mapping = state
        .store
        .litellm_credential_mapping_for_context(key_id, key.project_id)
        .await?;
    let mut result = CustomerSpendSnapshot {
        key_id,
        status: "not_mapped",
        mapping_scope: mapping.as_ref().map(|mapping| mapping.scope),
        customers: Vec::new(),
        page: options.page,
        page_size: options.page_size,
        has_more: false,
        customers_checked: 0,
        customer_id: options.customer_id.clone(),
        start_date: options.start_date,
        end_date: options.end_date,
        fetched_at: None,
    };
    if let Some(credential) = mapping {
        result.status = "unavailable";
        // Bound the directory lookup and complete aggregate, including both bodies.
        if let Ok(Ok(scan)) = tokio::time::timeout(
            Duration::from_secs(5),
            fetch_customer_spend(state, &credential.credential, options),
        )
        .await
        {
            result.status = "available";
            result.customers = scan.customers;
            result.has_more = scan.has_more;
            result.customers_checked = scan.customers_checked;
            result.fetched_at = Some(Utc::now());
        }
    }
    Ok(Some(result))
}

struct CustomerSpendScan {
    customers: Vec<CustomerSpend>,
    has_more: bool,
    customers_checked: usize,
}

#[derive(Deserialize)]
struct RegisteredCustomer {
    user_id: String,
}

fn parse_customer_directory(bytes: &[u8], page_size: usize) -> Result<Vec<String>, ()> {
    let rows: Vec<RegisteredCustomer> = serde_json::from_slice(bytes).map_err(|_| ())?;
    if rows.len() > page_size {
        return Err(());
    }
    let mut seen = BTreeSet::new();
    let mut ids = Vec::with_capacity(rows.len());
    for row in rows {
        if !valid_customer_id(&row.user_id) || !seen.insert(row.user_id.clone()) {
            return Err(());
        }
        ids.push(row.user_id);
    }
    if ids.join(",").len() > CUSTOMER_MAX_QUERY_BYTES {
        return Err(());
    }
    Ok(ids)
}

async fn fetch_customer_bytes(
    state: &AppState,
    upstream: &LiteLlmUiUpstream,
    url: reqwest::Url,
) -> Result<Vec<u8>, ()> {
    let request = state.litellm_ui_client.get(url);
    let request = match upstream.credential_header_mode {
        CredentialHeaderMode::AuthorizationBearer => request.bearer_auth(&upstream.credential),
        CredentialHeaderMode::CustomHeader => request.header(
            upstream.credential_header_name.as_deref().ok_or(())?,
            litellm_ui_custom_header_credential(upstream),
        ),
    };
    // Never expose/log upstream URLs, errors, raw bodies or key metadata.
    let mut response = request.send().await.map_err(|_| ())?;
    if !response.status().is_success() {
        return Err(());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| ())? {
        if bytes.len() + chunk.len() > CUSTOMER_MAX_BODY_BYTES {
            return Err(());
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

#[derive(Deserialize)]
struct CustomerActivityPage {
    results: Vec<CustomerActivityDay>,
    metadata: CustomerActivityMetadata,
}

#[derive(Deserialize)]
struct CustomerActivityDay {
    date: chrono::NaiveDate,
    breakdown: CustomerActivityBreakdown,
}

#[derive(Deserialize)]
struct CustomerActivityBreakdown {
    entities: BTreeMap<String, CustomerActivityEntity>,
}

#[derive(Deserialize)]
struct CustomerActivityEntity {
    api_key_breakdown: BTreeMap<String, CustomerActivityKey>,
}

#[derive(Deserialize)]
struct CustomerActivityKey {
    metrics: CustomerActivityMetrics,
}

#[derive(Deserialize)]
struct CustomerActivityMetrics {
    spend: f64,
    api_requests: u64,
}

#[derive(Deserialize)]
struct CustomerActivityMetadata {
    page: usize,
    total_pages: usize,
    has_more: bool,
}

pub(super) struct CustomerSpendContext {
    upstream: LiteLlmUiUpstream,
    key_hash: String,
}

pub(super) async fn customer_context(
    state: &AppState,
    mapped_key: &str,
) -> Result<CustomerSpendContext, ()> {
    Ok(CustomerSpendContext {
        upstream: resolve_litellm_ui_upstream(state).await.map_err(|_| ())?,
        key_hash: format!("{:x}", Sha256::digest(mapped_key.as_bytes())),
    })
}

impl CustomerSpendContext {
    pub(super) async fn directory(
        &self,
        state: &AppState,
        page: usize,
        page_size: usize,
    ) -> Result<Vec<String>, ()> {
        if page == 0 || !(1..=CUSTOMER_MAX_PAGE_SIZE).contains(&page_size) {
            return Err(());
        }
        let mut url = reqwest::Url::parse(&self.upstream.base_url).map_err(|_| ())?;
        url.set_path("/customer/list");
        url.set_query(None);
        url.set_fragment(None);
        url.query_pairs_mut()
            .append_pair("page", &page.to_string())
            .append_pair("page_size", &page_size.to_string());
        let bytes = fetch_customer_bytes(state, &self.upstream, url).await?;
        parse_customer_directory(&bytes, page_size)
    }

    pub(super) async fn costs(
        &self,
        state: &AppState,
        ids: &[String],
        start: chrono::NaiveDate,
        end: chrono::NaiveDate,
    ) -> Result<Vec<CustomerSpend>, ()> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let allowed_ids: BTreeSet<_> = ids.iter().cloned().collect();
        let joined_ids = ids.join(",");
        if ids.len() > CUSTOMER_MAX_PAGE_SIZE
            || allowed_ids.len() != ids.len()
            || ids.iter().any(|id| !valid_customer_id(id))
            || joined_ids.len() > CUSTOMER_MAX_QUERY_BYTES
        {
            return Err(());
        }
        let mut url = reqwest::Url::parse(&self.upstream.base_url).map_err(|_| ())?;
        url.set_path("/customer/daily/activity/aggregated");
        url.set_query(None);
        url.set_fragment(None);
        // Explicit IDs avoid LiteLLM's unbounded customer-alias lookup. The
        // aggregate endpoint returns complete totals, not raw activity pages.
        url.query_pairs_mut()
            .append_pair("api_key", &self.key_hash)
            .append_pair("end_user_ids", &joined_ids)
            .append_pair("start_date", &start.to_string())
            .append_pair("end_date", &end.to_string())
            .append_pair("api_key_limit", "1");
        let bytes = fetch_customer_bytes(state, &self.upstream, url).await?;
        parse_customer_activity(&bytes, &self.key_hash, start, end, &allowed_ids)
    }
}

async fn fetch_customer_spend(
    state: &AppState,
    mapped_key: &str,
    options: &CustomerSpendOptions,
) -> Result<CustomerSpendScan, ()> {
    let context = customer_context(state, mapped_key).await?;
    let ids = if let Some(customer_id) = &options.customer_id {
        vec![customer_id.clone()]
    } else {
        context
            .directory(state, options.page, options.page_size)
            .await?
    };
    let has_more = options.customer_id.is_none() && ids.len() == options.page_size;
    let customers_checked = ids.len();
    let customers = context
        .costs(state, &ids, options.start_date, options.end_date)
        .await?;
    Ok(CustomerSpendScan {
        customers,
        has_more,
        customers_checked,
    })
}

fn parse_customer_activity(
    bytes: &[u8],
    key_hash: &str,
    start: chrono::NaiveDate,
    end: chrono::NaiveDate,
    allowed_ids: &BTreeSet<String>,
) -> Result<Vec<CustomerSpend>, ()> {
    let activity: CustomerActivityPage = serde_json::from_slice(bytes).map_err(|_| ())?;
    let metadata = activity.metadata;
    if metadata.page != 1
        || metadata.total_pages > 1
        || metadata.has_more
        || (!activity.results.is_empty() && metadata.total_pages == 0)
    {
        return Err(());
    }
    let mut dates = BTreeSet::new();
    let mut customers = BTreeMap::<String, CustomerSpend>::new();
    for day in activity.results {
        if day.date < start || day.date > end || !dates.insert(day.date) {
            return Err(());
        }
        for (customer_id, entity) in day.breakdown.entities {
            if !allowed_ids.contains(&customer_id) {
                return Err(());
            }
            // Never substitute global totals when an upstream ignores api_key.
            let metrics = &entity.api_key_breakdown.get(key_hash).ok_or(())?.metrics;
            if !metrics.spend.is_finite() || metrics.spend < 0.0 {
                return Err(());
            }
            let customer = customers
                .entry(customer_id.clone())
                .or_insert_with(|| CustomerSpend {
                    customer_id,
                    spend_usd: 0.0,
                    requests: 0,
                });
            customer.spend_usd += metrics.spend;
            customer.requests = customer
                .requests
                .checked_add(metrics.api_requests)
                .ok_or(())?;
            if !customer.spend_usd.is_finite() {
                return Err(());
            }
        }
    }
    let mut result: Vec<_> = customers.into_values().collect();
    result.sort_by(|a, b| {
        b.spend_usd
            .total_cmp(&a.spend_usd)
            .then_with(|| a.customer_id.cmp(&b.customer_id))
    });
    Ok(result)
}

pub(super) async fn key_spend(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(key_id): Path<Uuid>,
) -> Response {
    if let Err(response) = require_admin_scope(&state, &headers, SCOPE_USAGE_READ).await {
        return response;
    }
    let mut response = match snapshot(&state, key_id).await {
        Ok(Some(value)) => Json(value).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(error) => error_response(&headers, error),
    };
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

async fn snapshot(state: &AppState, key_id: Uuid) -> GatewayResult<Option<SpendSnapshot>> {
    let Some(key) = state.store.get_admin_key(key_id).await? else {
        return Ok(None);
    };
    let mapping = state
        .store
        .litellm_credential_mapping_for_context(key_id, key.project_id)
        .await?;
    let mut result = SpendSnapshot {
        key_id,
        status: "not_mapped",
        mapping_scope: mapping.as_ref().map(|m| m.scope),
        spend_usd: None,
        fetched_at: None,
    };
    if let Some(credential) = mapping {
        result.status = "unavailable";
        // Includes response-body consumption; redirects are disabled by the client.
        if let Ok(Ok(spend)) = tokio::time::timeout(
            Duration::from_secs(5),
            fetch_spend(state, &credential.credential),
        )
        .await
        {
            result.status = "available";
            result.spend_usd = Some(spend);
            result.fetched_at = Some(Utc::now());
        }
    }
    Ok(Some(result))
}

async fn fetch_spend(state: &AppState, mapped_key: &str) -> Result<f64, ()> {
    let upstream = resolve_litellm_ui_upstream(state).await.map_err(|_| ())?;
    let mut url = reqwest::Url::parse(&upstream.base_url).map_err(|_| ())?;
    url.set_path("/key/info");
    url.set_query(None);
    url.set_fragment(None);
    // LiteLLM accepts the SHA-256 identifier; keep raw keys out of access logs.
    url.query_pairs_mut().append_pair(
        "key",
        &format!("{:x}", Sha256::digest(mapped_key.as_bytes())),
    );
    let mut request = state.litellm_ui_client.get(url);
    request = match upstream.credential_header_mode {
        CredentialHeaderMode::AuthorizationBearer => request.bearer_auth(&upstream.credential),
        CredentialHeaderMode::CustomHeader => request.header(
            upstream.credential_header_name.as_deref().ok_or(())?,
            litellm_ui_custom_header_credential(&upstream),
        ),
    };
    // Do not return/log request errors, URLs, upstream bodies or key metadata.
    let mut response = request.send().await.map_err(|_| ())?;
    if !response.status().is_success() {
        return Err(());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| ())? {
        if bytes.len() + chunk.len() > 64 * 1024 {
            return Err(());
        }
        bytes.extend_from_slice(&chunk);
    }
    parse_spend(&bytes)
}

fn parse_spend(bytes: &[u8]) -> Result<f64, ()> {
    let value: serde_json::Value = serde_json::from_slice(bytes).map_err(|_| ())?;
    value
        .get("info")
        .and_then(|v| v.get("spend"))
        .and_then(|v| v.as_f64())
        .filter(|v| v.is_finite() && *v >= 0.0)
        .ok_or(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query() -> CustomerSpendQuery {
        CustomerSpendQuery {
            start_date: Some("2026-11-01".into()),
            end_date: Some("2026-11-13".into()),
            ..Default::default()
        }
    }

    fn activity(entities: Value) -> Value {
        serde_json::json!({
            "results":[{"date":"2026-11-13","breakdown":{"entities":entities}}],
            "metadata":{"page":1,"total_pages":1,"has_more":false}
        })
    }

    fn parse(value: Value, ids: &[&str]) -> Result<Vec<CustomerSpend>, ()> {
        let (start, end) = query().dates().unwrap();
        parse_customer_activity(
            &serde_json::to_vec(&value).unwrap(),
            "mapped-hash",
            start,
            end,
            &ids.iter().map(|id| (*id).to_owned()).collect(),
        )
    }

    #[test]
    fn customer_query_bounds_dates_directory_pages_and_exact_ids() {
        let defaults = query().validate().unwrap();
        assert_eq!(defaults.page, 1);
        assert_eq!(defaults.page_size, 50);
        for (start, end) in [
            (None, Some("2026-11-13")),
            (Some("2026-11-01"), None),
            (Some("2026-11-1"), Some("2026-11-13")),
            (Some("2026-02-30"), Some("2026-11-13")),
            (Some("2026-11-14"), Some("2026-11-13")),
            (Some("2025-01-01"), Some("2026-01-02")),
            (Some("2026-11-01T00:00:00Z"), Some("2026-11-13")),
        ] {
            assert!(CustomerSpendQuery {
                start_date: start.map(str::to_owned),
                end_date: end.map(str::to_owned),
                ..Default::default()
            }
            .validate()
            .is_err());
        }
        for (start, end) in [("2026-11-13", "2026-11-13"), ("2025-01-01", "2026-01-01")] {
            assert!(CustomerSpendQuery {
                start_date: Some(start.into()),
                end_date: Some(end.into()),
                ..Default::default()
            }
            .validate()
            .is_ok());
        }
        for page in [0, CUSTOMER_MAX_DIRECTORY_PAGE + 1] {
            assert!(CustomerSpendQuery {
                page: Some(page),
                ..query()
            }
            .validate()
            .is_err());
        }
        for page_size in [0, CUSTOMER_MAX_PAGE_SIZE + 1] {
            assert!(CustomerSpendQuery {
                page_size: Some(page_size),
                ..query()
            }
            .validate()
            .is_err());
        }
        for id in [
            "".into(),
            "   ".into(),
            "customer,other".into(),
            "customer\nother".into(),
            "x".repeat(1025),
        ] {
            assert!(CustomerSpendQuery {
                customer_id: Some(id),
                ..query()
            }
            .validate()
            .is_err());
        }
        let exact = CustomerSpendQuery {
            page: Some(900_000),
            customer_id: Some(" customer-one ".into()),
            ..query()
        }
        .validate()
        .unwrap();
        assert_eq!(exact.page, 1);
        assert_eq!(exact.customer_id.as_deref(), Some("customer-one"));
    }

    #[test]
    fn customer_directory_is_bounded_and_discards_global_costs_and_metadata() {
        let bytes =
            br#"[{"user_id":"customer-a","spend":9999,"token":"secret"},{"user_id":"customer-b"}]"#;
        assert_eq!(
            parse_customer_directory(bytes, 2).unwrap(),
            ["customer-a", "customer-b"]
        );
        assert!(parse_customer_directory(bytes, 1).is_err());
        for ids in [
            vec!["duplicate", "duplicate"],
            vec![""],
            vec!["customer,other"],
            vec!["customer\nother"],
        ] {
            let body: Vec<_> = ids
                .into_iter()
                .map(|id| serde_json::json!({"user_id":id}))
                .collect();
            assert!(parse_customer_directory(&serde_json::to_vec(&body).unwrap(), 50).is_err());
        }
        let oversized: Vec<_> = (0..17)
            .map(|id| serde_json::json!({"user_id":format!("{id}{}", "x".repeat(1000))}))
            .collect();
        assert!(parse_customer_directory(&serde_json::to_vec(&oversized).unwrap(), 50).is_err());
        assert!(parse_customer_directory(b"{}", 50).is_err());
        assert!(parse_customer_directory(br#"[{"alias":"missing-id"}]"#, 50).is_err());
        assert!(parse_customer_directory(br#"[{"user_id":null}]"#, 50).is_err());
        assert!(parse_customer_directory(b"[]", 50).unwrap().is_empty());
    }

    #[test]
    fn customer_aggregates_complete_days_and_only_requested_key_scoped_customers() {
        let mut body = activity(serde_json::json!({
            "customer-a":{"metrics":{"spend":9999},"metadata":{"token":"secret"},"api_key_breakdown":{
                "mapped-hash":{"metrics":{"spend":1.25,"api_requests":2}},
                "other-hash":{"metrics":{"spend":999,"api_requests":999}}
            }},
            "customer-b":{"api_key_breakdown":{"mapped-hash":{"metrics":{"spend":0,"api_requests":1}}}}
        }));
        body["results"].as_array_mut().unwrap().push(serde_json::json!({
            "date":"2026-11-12","breakdown":{"entities":{
                "customer-a":{"api_key_breakdown":{"mapped-hash":{"metrics":{"spend":0.75,"api_requests":1}}}}
            }}
        }));
        let customers = parse(body, &["customer-a", "customer-b"]).unwrap();
        assert_eq!(customers.len(), 2);
        assert_eq!(customers[0].customer_id, "customer-a");
        assert_eq!(customers[0].spend_usd, 2.0);
        assert_eq!(customers[0].requests, 3);
        assert_eq!(customers[1].spend_usd, 0.0);
        let serialized = serde_json::to_string(&customers).unwrap();
        assert!(!serialized.contains("secret"));
        assert!(!serialized.contains("hash"));
        assert!(!serialized.contains("999"));
    }

    #[test]
    fn explicit_registered_unassigned_customer_keeps_its_scoped_costs() {
        let body = activity(serde_json::json!({
            "Unassigned":{"api_key_breakdown":{"mapped-hash":{"metrics":{"spend":1.25,"api_requests":2}}}}
        }));
        let customers = parse(body, &["Unassigned"]).unwrap();
        assert_eq!(customers.len(), 1);
        assert_eq!(customers[0].customer_id, "Unassigned");
        assert_eq!(customers[0].spend_usd, 1.25);
        assert_eq!(customers[0].requests, 2);
    }

    #[test]
    fn customer_aggregates_reject_partial_unscoped_malformed_or_duplicate_data() {
        let valid = activity(serde_json::json!({
            "customer":{"api_key_breakdown":{"mapped-hash":{"metrics":{"spend":1,"api_requests":2}}}}
        }));
        let mut cases = Vec::new();
        for (field, value) in [
            ("page", serde_json::json!(2)),
            ("total_pages", serde_json::json!(2)),
            ("total_pages", serde_json::json!(0)),
            ("has_more", serde_json::json!(true)),
        ] {
            let mut body = valid.clone();
            body["metadata"][field] = value;
            cases.push(body);
        }
        for (field, value) in [
            ("spend", serde_json::json!(-1)),
            ("spend", Value::Null),
            ("spend", serde_json::json!("1")),
            ("api_requests", serde_json::json!(-1)),
            ("api_requests", serde_json::json!(1.5)),
        ] {
            let mut body = valid.clone();
            body["results"][0]["breakdown"]["entities"]["customer"]["api_key_breakdown"]
                ["mapped-hash"]["metrics"][field] = value;
            cases.push(body);
        }
        let mut unscoped = valid.clone();
        unscoped["results"][0]["breakdown"]["entities"]["customer"]["api_key_breakdown"] =
            serde_json::json!({"other-hash":{"metrics":{"spend":9,"api_requests":9}}});
        cases.push(unscoped);
        let mut outside = valid.clone();
        outside["results"][0]["date"] = serde_json::json!("2026-10-31");
        cases.push(outside);
        let mut duplicate = valid.clone();
        duplicate["results"]
            .as_array_mut()
            .unwrap()
            .push(valid["results"][0].clone());
        cases.push(duplicate);
        let foreign = activity(
            serde_json::json!({"other-customer":{"api_key_breakdown":{"mapped-hash":{"metrics":{"spend":2,"api_requests":1}}}}}),
        );
        cases.push(foreign);
        cases.push(activity(
            serde_json::json!({"Unassigned":{"api_key_breakdown":{}}}),
        ));
        let mut overflow = valid.clone();
        overflow["results"][0]["breakdown"]["entities"]["customer"]["api_key_breakdown"]
            ["mapped-hash"]["metrics"] =
            serde_json::json!({"spend":f64::MAX,"api_requests":u64::MAX});
        let mut next = overflow["results"][0].clone();
        next["date"] = serde_json::json!("2026-11-12");
        overflow["results"].as_array_mut().unwrap().push(next);
        cases.push(overflow);
        for body in cases {
            assert!(parse(body, &["customer"]).is_err());
        }
        for total_pages in [0, 1] {
            assert!(parse(serde_json::json!({"results":[],"metadata":{"page":1,"total_pages":total_pages,"has_more":false}}),&["customer"]).unwrap().is_empty());
        }
    }

    #[test]
    fn accepts_only_finite_nonnegative_spend_and_discards_secrets() {
        for (body, expected) in [
            (
                r#"{"key":"secret","info":{"spend":0,"token":"secret"}}"#,
                0.0,
            ),
            (r#"{"info":{"spend":12.345}}"#, 12.345),
        ] {
            assert_eq!(parse_spend(body.as_bytes()), Ok(expected));
        }
        for body in [
            "invalid",
            "{}",
            r#"{"info":null}"#,
            r#"{"info":{"spend":null}}"#,
            r#"{"info":{"spend":"12"}}"#,
            r#"{"info":{"spend":-1}}"#,
            r#"{"info":{"spend":1e999}}"#,
        ] {
            assert_eq!(parse_spend(body.as_bytes()), Err(()));
        }
        let serialized = serde_json::to_string(&SpendSnapshot {
            key_id: Uuid::nil(),
            status: "unavailable",
            mapping_scope: None,
            spend_usd: None,
            fetched_at: None,
        })
        .unwrap();
        assert!(!serialized.contains("secret"));
        assert!(serialized.contains("\"spend_usd\":null"));
    }
}
