use async_trait::async_trait;
use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use gateway_core::{
    auth::VirtualKeyLookup, AdminKeyCreate, AdminKeyPatch, AdminKeyResponse, AdminKeyStore,
    CreatedAdminKeyResponse, GatewayError, GatewayResult, UsageEvent, VirtualKeyMaterial,
};
use gateway_store::{PostgresStore, RedisReadiness};
use serde::Serialize;
use std::sync::Arc;
use tower_http::{
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer},
    trace::TraceLayer,
};

#[async_trait]
pub trait GatewayData: VirtualKeyLookup + AdminKeyStore + Send + Sync {
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
    admin_token: String,
}

pub fn router(store: PostgresStore, redis: RedisReadiness, admin_token: String) -> Router {
    router_with_state(AppState {
        store: Arc::new(store),
        redis,
        admin_token,
    })
}

pub fn router_with_state(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz))
        .route("/admin/keys", post(create_key))
        .route("/admin/keys/{key_id}", get(get_key).patch(patch_key))
        .route("/admin/keys/{key_id}/revoke", post(revoke_key))
        .route("/admin/keys/{key_id}/disable", post(disable_key))
        .route("/admin/keys/{key_id}/usage", get(key_usage))
        .route("/admin/projects/{project_id}/usage", get(project_usage))
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

async fn create_key(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<AdminKeyCreate>,
) -> Response {
    if let Some(response) = require_admin(&state, &headers) {
        return response;
    }

    let material = match VirtualKeyMaterial::generate() {
        Ok(material) => material,
        Err(error) => return error_response(&headers, error),
    };
    match state.store.create_admin_key(request, &material).await {
        Ok(key) => Json(CreatedAdminKeyResponse {
            key,
            raw_key: material.raw_key,
        })
        .into_response(),
        Err(error) => error_response(&headers, error),
    }
}

async fn get_key(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(key_id): Path<uuid::Uuid>,
) -> Response {
    if let Some(response) = require_admin(&state, &headers) {
        return response;
    }

    match state.store.get_admin_key(key_id).await {
        Ok(Some(key)) => Json(key).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(error) => error_response(&headers, error),
    }
}

async fn patch_key(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(key_id): Path<uuid::Uuid>,
    Json(patch): Json<AdminKeyPatch>,
) -> Response {
    if let Some(response) = require_admin(&state, &headers) {
        return response;
    }

    match state.store.patch_admin_key(key_id, patch).await {
        Ok(Some(key)) => Json(key).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(error) => error_response(&headers, error),
    }
}

async fn revoke_key(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(key_id): Path<uuid::Uuid>,
) -> Response {
    mutate_key_lifecycle(state, headers, key_id, KeyLifecycleAction::Revoke).await
}

async fn disable_key(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(key_id): Path<uuid::Uuid>,
) -> Response {
    mutate_key_lifecycle(state, headers, key_id, KeyLifecycleAction::Disable).await
}

async fn key_usage(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(key_id): Path<uuid::Uuid>,
) -> Response {
    if let Some(response) = require_admin(&state, &headers) {
        return response;
    }

    match state.store.key_usage_summary(key_id).await {
        Ok(Some(summary)) => Json(summary).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(error) => error_response(&headers, error),
    }
}

async fn project_usage(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(project_id): Path<uuid::Uuid>,
) -> Response {
    if let Some(response) = require_admin(&state, &headers) {
        return response;
    }

    match state.store.project_usage_summary(project_id).await {
        Ok(summary) => Json(summary).into_response(),
        Err(error) => error_response(&headers, error),
    }
}

enum KeyLifecycleAction {
    Revoke,
    Disable,
}

async fn mutate_key_lifecycle(
    state: AppState,
    headers: HeaderMap,
    key_id: uuid::Uuid,
    action: KeyLifecycleAction,
) -> Response {
    if let Some(response) = require_admin(&state, &headers) {
        return response;
    }

    let result: GatewayResult<Option<AdminKeyResponse>> = match action {
        KeyLifecycleAction::Revoke => state.store.revoke_admin_key(key_id).await,
        KeyLifecycleAction::Disable => state.store.disable_admin_key(key_id).await,
    };

    match result {
        Ok(Some(key)) => Json(key).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(error) => error_response(&headers, error),
    }
}

fn require_admin(state: &AppState, headers: &HeaderMap) -> Option<Response> {
    let Some(authorization) = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
    else {
        return Some(error_response(headers, GatewayError::MissingAuthorization));
    };
    let Some(token) = authorization
        .strip_prefix("Bearer ")
        .map(str::trim)
        .filter(|token| !token.is_empty())
    else {
        return Some(error_response(
            headers,
            GatewayError::MalformedAuthorization,
        ));
    };

    if token != state.admin_token {
        return Some(error_response(
            headers,
            GatewayError::MalformedAuthorization,
        ));
    }

    None
}

fn error_response(headers: &HeaderMap, error: GatewayError) -> Response {
    let request_id = headers
        .get("x-request-id")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("unknown");
    (error.status_code(), Json(error.body(request_id))).into_response()
}

#[derive(Debug, Serialize)]
struct StatusBody {
    status: &'static str,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use gateway_core::{
        admin::{AdminKeyUsageSummary, AdminPolicyResponse, ProjectUsageSummary},
        auth::StoredVirtualKey,
    };
    use std::sync::Mutex;
    use tower::ServiceExt;
    use uuid::Uuid;

    #[derive(Clone)]
    struct MemoryStore {
        key: Arc<Mutex<Option<StoredVirtualKey>>>,
        admin_key: Arc<Mutex<Option<AdminKeyResponse>>>,
        events: Arc<Mutex<Vec<UsageEvent>>>,
        postgres_ready: bool,
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
            if self.postgres_ready {
                Ok(())
            } else {
                Err(GatewayError::StoreUnavailable)
            }
        }
    }

    #[async_trait]
    impl AdminKeyStore for MemoryStore {
        async fn create_admin_key(
            &self,
            request: AdminKeyCreate,
            material: &VirtualKeyMaterial,
        ) -> GatewayResult<AdminKeyResponse> {
            let key = AdminKeyResponse {
                id: Uuid::new_v4(),
                project_id: request.project_id,
                key_prefix: material.key_prefix.clone(),
                disabled: false,
                revoked_at: None,
                expires_at: request.expires_at,
                policy: AdminPolicyResponse {
                    allowed_routes: vec![
                        "/v1/chat/completions".to_owned(),
                        "/v1/responses".to_owned(),
                    ],
                    allowed_models: Vec::new(),
                    allowed_providers: vec!["litellm".to_owned()],
                    rpm_limit: None,
                    tpm_limit: None,
                    daily_budget_usd: None,
                    monthly_budget_usd: None,
                    allow_streaming: false,
                    allow_tools: false,
                },
                created_at: Utc::now(),
                updated_at: Utc::now(),
            };
            *self.admin_key.lock().expect("lock poisoned") = Some(key.clone());
            Ok(key)
        }

        async fn get_admin_key(&self, _key_id: Uuid) -> GatewayResult<Option<AdminKeyResponse>> {
            Ok(self.admin_key.lock().expect("lock poisoned").clone())
        }

        async fn patch_admin_key(
            &self,
            _key_id: Uuid,
            _patch: AdminKeyPatch,
        ) -> GatewayResult<Option<AdminKeyResponse>> {
            Ok(self.admin_key.lock().expect("lock poisoned").clone())
        }

        async fn revoke_admin_key(&self, _key_id: Uuid) -> GatewayResult<Option<AdminKeyResponse>> {
            let mut key = self.admin_key.lock().expect("lock poisoned");
            if let Some(key) = key.as_mut() {
                key.disabled = true;
                key.revoked_at = Some(Utc::now());
            }
            Ok(key.clone())
        }

        async fn disable_admin_key(
            &self,
            _key_id: Uuid,
        ) -> GatewayResult<Option<AdminKeyResponse>> {
            let mut key = self.admin_key.lock().expect("lock poisoned");
            if let Some(key) = key.as_mut() {
                key.disabled = true;
            }
            Ok(key.clone())
        }

        async fn key_usage_summary(
            &self,
            key_id: Uuid,
        ) -> GatewayResult<Option<AdminKeyUsageSummary>> {
            Ok(Some(AdminKeyUsageSummary {
                key_id,
                request_count: 0,
                success_count: 0,
                failure_count: 0,
                total_latency_ms: 0,
                estimated_cost_usd: None,
            }))
        }

        async fn project_usage_summary(
            &self,
            project_id: Uuid,
        ) -> GatewayResult<ProjectUsageSummary> {
            Ok(ProjectUsageSummary {
                project_id,
                request_count: 0,
                success_count: 0,
                failure_count: 0,
                total_latency_ms: 0,
                estimated_cost_usd: None,
            })
        }
    }

    fn stored_key(raw: &str) -> StoredVirtualKey {
        StoredVirtualKey {
            id: Uuid::new_v4(),
            project_id: Uuid::new_v4(),
            key_prefix: raw.chars().take(16).collect(),
            key_hash: "not-used-by-control-api".to_owned(),
            disabled: false,
            expires_at: None,
        }
    }

    fn test_state(store: MemoryStore) -> AppState {
        test_state_with_redis_url(store, "redis://127.0.0.1:6379")
    }

    fn test_state_with_redis_url(store: MemoryStore, redis_url: &str) -> AppState {
        let redis = RedisReadiness::new(redis_url).expect("redis client");
        AppState {
            store: Arc::new(store),
            redis,
            admin_token: "admin-test-token".to_owned(),
        }
    }

    async fn request(app: Router, route: &str) -> Response {
        app.oneshot(
            axum::http::Request::builder()
                .method(axum::http::Method::GET)
                .uri(route)
                .header("x-request-id", "req_test")
                .body(axum::body::Body::empty())
                .expect("request"),
        )
        .await
        .expect("response")
    }

    async fn admin_post(app: Router, route: &str, token: Option<&str>, body: &str) -> Response {
        let mut builder = axum::http::Request::builder()
            .method(axum::http::Method::POST)
            .uri(route)
            .header("x-request-id", "req_test")
            .header("content-type", "application/json");
        if let Some(token) = token {
            builder = builder.header("authorization", format!("Bearer {token}"));
        }

        app.oneshot(
            builder
                .body(axum::body::Body::from(body.to_owned()))
                .expect("request"),
        )
        .await
        .expect("response")
    }

    #[tokio::test]
    async fn healthz_returns_ok() {
        let store = MemoryStore {
            key: Arc::new(Mutex::new(None)),
            admin_key: Arc::new(Mutex::new(None)),
            events: Arc::new(Mutex::new(Vec::new())),
            postgres_ready: true,
        };

        let app = router_with_state(test_state_with_redis_url(store, "redis://127.0.0.1:0"));
        let response = request(app, "/healthz").await;

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn generation_routes_are_not_served_by_axum_control_api() {
        let raw = "rk_live_1234567890abcdef";
        let store = MemoryStore {
            key: Arc::new(Mutex::new(Some(stored_key(raw)))),
            admin_key: Arc::new(Mutex::new(None)),
            events: Arc::new(Mutex::new(Vec::new())),
            postgres_ready: true,
        };

        let app = router_with_state(test_state(store.clone()));
        let response = request(app, "/v1/chat/completions").await;

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert!(store.events.lock().expect("lock poisoned").is_empty());
    }

    #[tokio::test]
    async fn readyz_returns_unavailable_when_store_is_not_ready() {
        let raw = "rk_live_1234567890abcdef";
        let store = MemoryStore {
            key: Arc::new(Mutex::new(Some(stored_key(raw)))),
            admin_key: Arc::new(Mutex::new(None)),
            events: Arc::new(Mutex::new(Vec::new())),
            postgres_ready: false,
        };

        let app = router_with_state(test_state(store));
        let response = request(app, "/readyz").await;

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn admin_create_key_requires_admin_token() {
        let store = MemoryStore {
            key: Arc::new(Mutex::new(None)),
            admin_key: Arc::new(Mutex::new(None)),
            events: Arc::new(Mutex::new(Vec::new())),
            postgres_ready: true,
        };
        let app = router_with_state(test_state(store));
        let project_id = Uuid::new_v4();
        let response = admin_post(
            app,
            "/admin/keys",
            None,
            &format!(r#"{{"project_id":"{project_id}"}}"#),
        )
        .await;

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn admin_create_key_returns_raw_key_once() {
        let store = MemoryStore {
            key: Arc::new(Mutex::new(None)),
            admin_key: Arc::new(Mutex::new(None)),
            events: Arc::new(Mutex::new(Vec::new())),
            postgres_ready: true,
        };
        let app = router_with_state(test_state(store));
        let project_id = Uuid::new_v4();
        let response = admin_post(
            app,
            "/admin/keys",
            Some("admin-test-token"),
            &format!(r#"{{"project_id":"{project_id}"}}"#),
        )
        .await;

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let value: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert!(value["raw_key"]
            .as_str()
            .expect("raw key")
            .starts_with("rk_live_"));
        assert!(value["key"]["key_prefix"]
            .as_str()
            .expect("key prefix")
            .starts_with("rk_live_"));
        assert!(value["key"].get("key_hash").is_none());
    }
}
