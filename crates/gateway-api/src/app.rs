use async_trait::async_trait;
use axum::{
    body::{Body, Bytes},
    extract::State,
    http::{HeaderMap, Method, StatusCode, Uri},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use chrono::Utc;
use futures_util::TryStreamExt;
use gateway_core::{
    auth::{Authenticator, VirtualKeyLookup},
    extract_model, GatewayError, GatewayResult, Route, UsageEvent,
};
use gateway_proxy::{LiteLlmProxy, UpstreamResponse};
use gateway_store::{PostgresStore, RedisReadiness};
use serde::Serialize;
use std::{sync::Arc, time::Instant};
use tower_http::{
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer},
    trace::TraceLayer,
};
use uuid::Uuid;

#[async_trait]
pub trait GatewayData: VirtualKeyLookup + Send + Sync {
    async fn insert_usage_event(&self, event: &UsageEvent) -> GatewayResult<()>;
    async fn postgres_ready(&self) -> GatewayResult<()>;
}

#[async_trait]
impl GatewayData for PostgresStore {
    async fn insert_usage_event(&self, event: &UsageEvent) -> GatewayResult<()> {
        PostgresStore::insert_usage_event(self, event).await
    }

    async fn postgres_ready(&self) -> GatewayResult<()> {
        self.ready()
            .await
            .map_err(|_| GatewayError::StoreUnavailable)
    }
}

#[derive(Clone)]
pub struct AppState {
    store: Arc<dyn GatewayData>,
    redis: RedisReadiness,
    proxy: LiteLlmProxy,
}

pub fn router(store: PostgresStore, redis: RedisReadiness, proxy: LiteLlmProxy) -> Router {
    router_with_state(AppState {
        store: Arc::new(store),
        redis,
        proxy,
    })
}

pub fn router_with_state(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz))
        .route("/v1/chat/completions", post(proxy_generation))
        .route("/v1/responses", post(proxy_generation))
        .layer(PropagateRequestIdLayer::x_request_id())
        .layer(SetRequestIdLayer::x_request_id(MakeRequestUuid))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

async fn healthz() -> impl IntoResponse {
    Json(StatusBody { status: "ok" })
}

async fn readyz(State(state): State<AppState>) -> Response {
    let postgres = state.store.postgres_ready().await;
    let redis = state
        .redis
        .ready()
        .await
        .map_err(|_| GatewayError::StoreUnavailable);

    match (postgres, redis) {
        (Ok(()), Ok(())) => Json(StatusBody { status: "ready" }).into_response(),
        _ => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(StatusBody {
                status: "not_ready",
            }),
        )
            .into_response(),
    }
}

async fn proxy_generation(
    State(state): State<AppState>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let started = Instant::now();
    let request_id = request_id(&headers);
    let response = proxy_generation_inner(
        &state,
        &method,
        &uri,
        &headers,
        body.to_vec(),
        &request_id,
        started,
    )
    .await;

    match response {
        Ok(response) => response,
        Err(error) => error_response(error, &request_id),
    }
}

async fn proxy_generation_inner(
    state: &AppState,
    method: &Method,
    uri: &Uri,
    headers: &HeaderMap,
    body: Vec<u8>,
    request_id: &str,
    started: Instant,
) -> GatewayResult<Response> {
    let route = Route::resolve(method, uri.path())?;
    let auth_header = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok());
    let key = Authenticator::new(state.store.clone())
        .authenticate_authorization(auth_header, Utc::now())
        .await?;
    let model = extract_model(&body);

    let upstream = state
        .proxy
        .forward(route, uri.query(), headers, body, request_id, &key)
        .await;

    let latency_ms = i64::try_from(started.elapsed().as_millis()).unwrap_or(i64::MAX);

    match upstream {
        Ok(upstream) => {
            let event = UsageEvent::new(
                request_id,
                &key,
                route,
                model,
                upstream.status.as_u16(),
                latency_ms,
                Utc::now(),
            );
            if let Err(error) = state.store.insert_usage_event(&event).await {
                tracing::warn!(
                    request_id = %request_id,
                    error = %error,
                    "usage event insert failed after successful upstream response"
                );
            }
            Ok(upstream_response(upstream))
        }
        Err(error) => {
            let status = error.status_code().as_u16();
            let event = UsageEvent::new(
                request_id,
                &key,
                route,
                model,
                status,
                latency_ms,
                Utc::now(),
            );
            let _ = state.store.insert_usage_event(&event).await;
            Err(error)
        }
    }
}

fn upstream_response(upstream: UpstreamResponse) -> Response {
    let body = Body::from_stream(upstream.body.map_err(std::io::Error::other));
    let mut response = Response::new(body);
    *response.status_mut() = upstream.status;
    for (name, value) in upstream.headers {
        if let Some(name) = name {
            response.headers_mut().append(name, value);
        }
    }
    response
}

fn error_response(error: GatewayError, request_id: &str) -> Response {
    let status = error.status_code();
    (status, Json(error.body(request_id))).into_response()
}

fn request_id(headers: &HeaderMap) -> String {
    headers
        .get("x-request-id")
        .and_then(|value| value.to_str().ok())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| Uuid::new_v4().to_string())
}

#[derive(Debug, Serialize)]
struct StatusBody {
    status: &'static str,
}

#[cfg(test)]
mod tests {
    use super::*;
    use argon2::{
        password_hash::{rand_core::OsRng, PasswordHasher, SaltString},
        Argon2,
    };
    use chrono::Duration;
    use gateway_core::{auth::StoredVirtualKey, AuthenticatedKey, UsageStatus};
    use std::sync::Mutex;
    use tower::ServiceExt;
    use wiremock::{
        matchers::{header, method, path},
        Mock, MockServer, ResponseTemplate,
    };

    #[derive(Clone)]
    struct MemoryStore {
        key: Arc<Mutex<Option<StoredVirtualKey>>>,
        events: Arc<Mutex<Vec<UsageEvent>>>,
    }

    #[derive(Clone)]
    struct FailingUsageStore {
        key: StoredVirtualKey,
    }

    #[async_trait]
    impl VirtualKeyLookup for MemoryStore {
        async fn find_by_prefix(&self, _prefix: &str) -> GatewayResult<Option<StoredVirtualKey>> {
            Ok(self.key.lock().expect("lock poisoned").clone())
        }
    }

    #[async_trait]
    impl GatewayData for MemoryStore {
        async fn insert_usage_event(&self, event: &UsageEvent) -> GatewayResult<()> {
            self.events
                .lock()
                .expect("lock poisoned")
                .push(event.clone());
            Ok(())
        }

        async fn postgres_ready(&self) -> GatewayResult<()> {
            Ok(())
        }
    }

    #[async_trait]
    impl VirtualKeyLookup for FailingUsageStore {
        async fn find_by_prefix(&self, _prefix: &str) -> GatewayResult<Option<StoredVirtualKey>> {
            Ok(Some(self.key.clone()))
        }
    }

    #[async_trait]
    impl GatewayData for FailingUsageStore {
        async fn insert_usage_event(&self, _event: &UsageEvent) -> GatewayResult<()> {
            Err(GatewayError::StoreUnavailable)
        }

        async fn postgres_ready(&self) -> GatewayResult<()> {
            Ok(())
        }
    }

    fn hash(raw: &str) -> String {
        let salt = SaltString::generate(&mut OsRng);
        Argon2::default()
            .hash_password(raw.as_bytes(), &salt)
            .expect("hash")
            .to_string()
    }

    fn stored_key(raw: &str) -> StoredVirtualKey {
        StoredVirtualKey {
            id: Uuid::new_v4(),
            project_id: Uuid::new_v4(),
            key_prefix: raw.chars().take(16).collect(),
            key_hash: hash(raw),
            disabled: false,
            expires_at: None,
        }
    }

    fn test_state(store: MemoryStore, upstream_url: String) -> AppState {
        test_state_with_timeout(store, upstream_url, std::time::Duration::from_secs(5))
    }

    fn test_state_with_timeout(
        store: MemoryStore,
        upstream_url: String,
        timeout: std::time::Duration,
    ) -> AppState {
        test_state_with_gateway_data(Arc::new(store), upstream_url, timeout)
    }

    fn test_state_with_gateway_data(
        store: Arc<dyn GatewayData>,
        upstream_url: String,
        timeout: std::time::Duration,
    ) -> AppState {
        let redis = RedisReadiness::new("redis://127.0.0.1:6379").expect("redis client");
        let proxy = LiteLlmProxy::new(
            gateway_proxy::LiteLlmConfig::new(upstream_url, "litellm-service", timeout)
                .expect("config"),
        )
        .expect("proxy");
        AppState {
            store,
            redis,
            proxy,
        }
    }

    async fn request(app: Router, route: &str, raw_key: Option<&str>) -> Response {
        let auth_header = raw_key.map(|raw_key| format!("Bearer {raw_key}"));
        request_with_auth_header(app, Method::POST, route, auth_header.as_deref()).await
    }

    async fn request_with_auth_header(
        app: Router,
        method: Method,
        route: &str,
        auth_header: Option<&str>,
    ) -> Response {
        let mut builder = axum::http::Request::builder()
            .method(method)
            .uri(route)
            .header("content-type", "application/json")
            .header("x-request-id", "req_test");
        if let Some(auth_header) = auth_header {
            builder = builder.header("authorization", auth_header);
        }
        app.oneshot(
            builder
                .body(Body::from(r#"{"model":"gpt-4o-mini","input":"ping"}"#))
                .expect("request"),
        )
        .await
        .expect("response")
    }

    #[tokio::test]
    async fn proxies_chat_completions_and_records_usage() {
        let raw = "rk_live_1234567890abcdef";
        let store = MemoryStore {
            key: Arc::new(Mutex::new(Some(stored_key(raw)))),
            events: Arc::new(Mutex::new(Vec::new())),
        };
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .and(header("authorization", "Bearer litellm-service"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "chatcmpl_test"
            })))
            .mount(&server)
            .await;

        let app = router_with_state(test_state(store.clone(), server.uri()));
        let response = request(app, "/v1/chat/completions", Some(raw)).await;

        assert_eq!(response.status(), StatusCode::OK);
        let events = store.events.lock().expect("lock poisoned");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].route, Route::ChatCompletions);
        assert_eq!(events[0].model.as_deref(), Some("gpt-4o-mini"));
        assert_eq!(events[0].status, UsageStatus::Success);
    }

    #[tokio::test]
    async fn proxies_responses_and_records_usage() {
        let raw = "rk_live_1234567890abcdef";
        let store = MemoryStore {
            key: Arc::new(Mutex::new(Some(stored_key(raw)))),
            events: Arc::new(Mutex::new(Vec::new())),
        };
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/responses"))
            .and(header("authorization", "Bearer litellm-service"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "resp_test"
            })))
            .mount(&server)
            .await;

        let app = router_with_state(test_state(store.clone(), server.uri()));
        let response = request(app, "/v1/responses", Some(raw)).await;

        assert_eq!(response.status(), StatusCode::OK);
        let events = store.events.lock().expect("lock poisoned");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].route, Route::Responses);
    }

    #[tokio::test]
    async fn returns_successful_upstream_response_when_usage_insert_fails() {
        let raw = "rk_live_1234567890abcdef";
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .and(header("authorization", "Bearer litellm-service"))
            .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
                "id": "chatcmpl_test"
            })))
            .mount(&server)
            .await;

        let app = router_with_state(test_state_with_gateway_data(
            Arc::new(FailingUsageStore {
                key: stored_key(raw),
            }),
            server.uri(),
            std::time::Duration::from_secs(5),
        ));
        let response = request(app, "/v1/chat/completions", Some(raw)).await;

        assert_eq!(response.status(), StatusCode::CREATED);
    }

    #[tokio::test]
    async fn rejects_missing_auth_before_upstream() {
        let store = MemoryStore {
            key: Arc::new(Mutex::new(None)),
            events: Arc::new(Mutex::new(Vec::new())),
        };
        let server = MockServer::start().await;

        let app = router_with_state(test_state(store.clone(), server.uri()));
        let response = request(app, "/v1/chat/completions", None).await;

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert!(store.events.lock().expect("lock poisoned").is_empty());
    }

    #[tokio::test]
    async fn rejects_malformed_auth_before_upstream() {
        let raw = "rk_live_1234567890abcdef";
        let store = MemoryStore {
            key: Arc::new(Mutex::new(Some(stored_key(raw)))),
            events: Arc::new(Mutex::new(Vec::new())),
        };
        let server = MockServer::start().await;

        let app = router_with_state(test_state(store.clone(), server.uri()));
        let response = request_with_auth_header(
            app,
            Method::POST,
            "/v1/chat/completions",
            Some("Token nope"),
        )
        .await;

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert!(store.events.lock().expect("lock poisoned").is_empty());
    }

    #[tokio::test]
    async fn rejects_unknown_key_before_upstream() {
        let store = MemoryStore {
            key: Arc::new(Mutex::new(None)),
            events: Arc::new(Mutex::new(Vec::new())),
        };
        let server = MockServer::start().await;

        let app = router_with_state(test_state(store.clone(), server.uri()));
        let response = request(app, "/v1/chat/completions", Some("rk_live_unknownabcdef")).await;

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert!(store.events.lock().expect("lock poisoned").is_empty());
    }

    #[tokio::test]
    async fn rejects_disabled_key_before_upstream() {
        let raw = "rk_live_1234567890abcdef";
        let mut key = stored_key(raw);
        key.disabled = true;
        let store = MemoryStore {
            key: Arc::new(Mutex::new(Some(key))),
            events: Arc::new(Mutex::new(Vec::new())),
        };
        let server = MockServer::start().await;

        let app = router_with_state(test_state(store.clone(), server.uri()));
        let response = request(app, "/v1/chat/completions", Some(raw)).await;

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert!(store.events.lock().expect("lock poisoned").is_empty());
    }

    #[tokio::test]
    async fn rejects_expired_key_before_upstream() {
        let raw = "rk_live_1234567890abcdef";
        let mut key = stored_key(raw);
        key.expires_at = Some(Utc::now() - Duration::minutes(1));
        let store = MemoryStore {
            key: Arc::new(Mutex::new(Some(key))),
            events: Arc::new(Mutex::new(Vec::new())),
        };
        let server = MockServer::start().await;

        let app = router_with_state(test_state(store.clone(), server.uri()));
        let response = request(app, "/v1/chat/completions", Some(raw)).await;

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert!(store.events.lock().expect("lock poisoned").is_empty());
    }

    #[tokio::test]
    async fn rejects_legacy_completions_route() {
        let raw = "rk_live_1234567890abcdef";
        let store = MemoryStore {
            key: Arc::new(Mutex::new(Some(stored_key(raw)))),
            events: Arc::new(Mutex::new(Vec::new())),
        };
        let server = MockServer::start().await;

        let app = router_with_state(test_state(store.clone(), server.uri()));
        let response = request(app, "/v1/completions", Some(raw)).await;

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert!(store.events.lock().expect("lock poisoned").is_empty());
    }

    #[tokio::test]
    async fn maps_upstream_timeout_and_records_failure_usage() {
        let raw = "rk_live_1234567890abcdef";
        let store = MemoryStore {
            key: Arc::new(Mutex::new(Some(stored_key(raw)))),
            events: Arc::new(Mutex::new(Vec::new())),
        };
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(
                ResponseTemplate::new(200).set_delay(std::time::Duration::from_millis(250)),
            )
            .mount(&server)
            .await;

        let app = router_with_state(test_state_with_timeout(
            store.clone(),
            server.uri(),
            std::time::Duration::from_millis(50),
        ));
        let response = request(app, "/v1/chat/completions", Some(raw)).await;

        assert_eq!(response.status(), StatusCode::GATEWAY_TIMEOUT);
        let events = store.events.lock().expect("lock poisoned");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].status, UsageStatus::Failure);
        assert_eq!(events[0].status_code, StatusCode::GATEWAY_TIMEOUT.as_u16());
    }

    #[tokio::test]
    async fn maps_upstream_connection_failure_and_records_failure_usage() {
        let raw = "rk_live_1234567890abcdef";
        let store = MemoryStore {
            key: Arc::new(Mutex::new(Some(stored_key(raw)))),
            events: Arc::new(Mutex::new(Vec::new())),
        };
        let app = router_with_state(test_state_with_timeout(
            store.clone(),
            "http://127.0.0.1:9".to_owned(),
            std::time::Duration::from_millis(500),
        ));
        let response = request(app, "/v1/chat/completions", Some(raw)).await;

        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        let events = store.events.lock().expect("lock poisoned");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].status, UsageStatus::Failure);
        assert_eq!(events[0].status_code, StatusCode::BAD_GATEWAY.as_u16());
    }

    #[allow(dead_code)]
    fn _assert_authenticated_key_send_sync(_: AuthenticatedKey) {}
}
