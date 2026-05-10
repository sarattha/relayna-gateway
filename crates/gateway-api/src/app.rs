use async_trait::async_trait;
use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use chrono::Utc;
use gateway_core::{
    auth::VirtualKeyLookup, AdminKeyCreate, AdminKeyPatch, AdminKeyResponse, AdminKeyStore,
    AdminServiceStore, CreatedAdminKeyResponse, CreatedOperatorTokenResponse, GatewayError,
    GatewayResult, OperatorTokenMaterial, OperatorTokenStore, ServiceCreateRequest,
    ServicePatchRequest, StudioServiceImportRequest, UsageBreakdownDimension, UsageEvent,
    UsageQuery, UsageQueryStore, VirtualKeyMaterial,
};
use gateway_store::{PostgresStore, RedisReadiness};
use serde::Serialize;
use std::sync::Arc;
use tower_http::{
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer},
    trace::TraceLayer,
};

#[async_trait]
pub trait GatewayData:
    VirtualKeyLookup
    + AdminKeyStore
    + AdminServiceStore
    + OperatorTokenStore
    + UsageQueryStore
    + Send
    + Sync
{
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
}

pub fn router(store: PostgresStore, redis: RedisReadiness) -> Router {
    router_with_state(AppState {
        store: Arc::new(store),
        redis,
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
        .route("/admin/operator-token/rotate", post(rotate_operator_token))
        .route("/admin/services", post(create_service).get(list_services))
        .route("/admin/services/import", post(import_service))
        .route("/admin/services/sync", post(sync_service))
        .route(
            "/admin/services/{service_name}",
            get(get_service).patch(patch_service).delete(delete_service),
        )
        .route(
            "/admin/services/{service_name}/disable",
            post(disable_service),
        )
        .route(
            "/admin/services/{service_name}/enable",
            post(enable_service),
        )
        .route(
            "/admin/services/{service_name}/sync-status",
            get(service_sync_status),
        )
        .route("/admin/projects/{project_id}/usage", get(project_usage))
        .route("/admin/usage/summary", get(usage_summary))
        .route("/admin/usage/timeseries", get(usage_timeseries))
        .route("/admin/usage/by-key", get(usage_by_key))
        .route("/admin/usage/by-project", get(usage_by_project))
        .route("/admin/usage/by-model", get(usage_by_model))
        .route("/admin/usage/by-provider", get(usage_by_provider))
        .route("/admin/usage/by-service", get(usage_by_service))
        .route("/admin/usage/by-task", get(usage_by_task))
        .route("/admin/tasks/{task_id}/usage", get(task_usage))
        .route("/admin/provider-health", get(provider_health))
        .route("/admin-ui", get(admin_ui_index))
        .route("/admin-ui/{*path}", get(admin_ui_asset))
        .route("/metrics", get(metrics))
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
    if let Some(response) = require_admin(&state, &headers).await {
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
    if let Some(response) = require_admin(&state, &headers).await {
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
    if let Some(response) = require_admin(&state, &headers).await {
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
    if let Some(response) = require_admin(&state, &headers).await {
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
    if let Some(response) = require_admin(&state, &headers).await {
        return response;
    }

    match state.store.project_usage_summary(project_id).await {
        Ok(summary) => Json(summary).into_response(),
        Err(error) => error_response(&headers, error),
    }
}

async fn rotate_operator_token(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let current_raw_token = match bearer_token(&headers) {
        Ok(token) => token.to_owned(),
        Err(error) => return error_response(&headers, error),
    };
    if let Some(response) = require_admin(&state, &headers).await {
        return response;
    }

    let material = match OperatorTokenMaterial::generate() {
        Ok(material) => material,
        Err(error) => return error_response(&headers, error),
    };
    match state
        .store
        .rotate_operator_token(&current_raw_token, &material, Utc::now())
        .await
    {
        Ok(token) => Json(CreatedOperatorTokenResponse {
            token,
            raw_token: material.raw_token,
        })
        .into_response(),
        Err(error) => error_response(&headers, error),
    }
}

async fn create_service(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<ServiceCreateRequest>,
) -> Response {
    admin_query(headers, &state, |store| async move {
        store.create_service(request).await
    })
    .await
}

async fn list_services(State(state): State<AppState>, headers: HeaderMap) -> Response {
    admin_query(headers, &state, |store| async move {
        store.list_services().await
    })
    .await
}

async fn get_service(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(service_name): Path<String>,
) -> Response {
    if let Some(response) = require_admin(&state, &headers).await {
        return response;
    }

    match state.store.get_service(&service_name).await {
        Ok(Some(service)) => Json(service).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(error) => error_response(&headers, error),
    }
}

async fn patch_service(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(service_name): Path<String>,
    Json(patch): Json<ServicePatchRequest>,
) -> Response {
    if let Some(response) = require_admin(&state, &headers).await {
        return response;
    }

    match state.store.patch_service(&service_name, patch).await {
        Ok(Some(service)) => Json(service).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(error) => error_response(&headers, error),
    }
}

async fn delete_service(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(service_name): Path<String>,
) -> Response {
    if let Some(response) = require_admin(&state, &headers).await {
        return response;
    }

    match state.store.delete_service(&service_name).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => StatusCode::NOT_FOUND.into_response(),
        Err(error) => error_response(&headers, error),
    }
}

async fn disable_service(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(service_name): Path<String>,
) -> Response {
    mutate_service_enabled(state, headers, service_name, false).await
}

async fn enable_service(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(service_name): Path<String>,
) -> Response {
    mutate_service_enabled(state, headers, service_name, true).await
}

async fn import_service(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<StudioServiceImportRequest>,
) -> Response {
    admin_query(headers, &state, |store| async move {
        store.import_studio_service(request).await
    })
    .await
}

async fn sync_service(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<StudioServiceImportRequest>,
) -> Response {
    admin_query(headers, &state, |store| async move {
        store.sync_studio_service(request).await
    })
    .await
}

async fn service_sync_status(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(service_name): Path<String>,
) -> Response {
    if let Some(response) = require_admin(&state, &headers).await {
        return response;
    }

    match state.store.service_sync_status(&service_name).await {
        Ok(Some(status)) => Json(status).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(error) => error_response(&headers, error),
    }
}

async fn usage_summary(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<UsageQuery>,
) -> Response {
    admin_query(headers, &state, |store| async move {
        store.usage_summary(query).await
    })
    .await
}

async fn usage_timeseries(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<UsageQuery>,
) -> Response {
    admin_query(headers, &state, |store| async move {
        store.usage_timeseries(query).await
    })
    .await
}

async fn usage_by_key(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<UsageQuery>,
) -> Response {
    usage_breakdown(state, headers, query, UsageBreakdownDimension::Key).await
}

async fn usage_by_project(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<UsageQuery>,
) -> Response {
    usage_breakdown(state, headers, query, UsageBreakdownDimension::Project).await
}

async fn usage_by_model(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<UsageQuery>,
) -> Response {
    usage_breakdown(state, headers, query, UsageBreakdownDimension::Model).await
}

async fn usage_by_provider(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<UsageQuery>,
) -> Response {
    usage_breakdown(state, headers, query, UsageBreakdownDimension::Provider).await
}

async fn usage_by_service(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<UsageQuery>,
) -> Response {
    usage_breakdown(state, headers, query, UsageBreakdownDimension::Service).await
}

async fn usage_by_task(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<UsageQuery>,
) -> Response {
    usage_breakdown(state, headers, query, UsageBreakdownDimension::Task).await
}

async fn task_usage(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(task_id): Path<String>,
    Query(mut query): Query<UsageQuery>,
) -> Response {
    query.task_id = Some(task_id);
    admin_query(headers, &state, |store| async move {
        store.usage_summary(query).await
    })
    .await
}

async fn usage_breakdown(
    state: AppState,
    headers: HeaderMap,
    query: UsageQuery,
    dimension: UsageBreakdownDimension,
) -> Response {
    admin_query(headers, &state, |store| async move {
        store.usage_breakdown(query, dimension).await
    })
    .await
}

async fn provider_health(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<UsageQuery>,
) -> Response {
    admin_query(headers, &state, |store| async move {
        store.provider_health(query).await
    })
    .await
}

async fn admin_query<T, Fut>(
    headers: HeaderMap,
    state: &AppState,
    query: impl FnOnce(Arc<dyn GatewayData>) -> Fut,
) -> Response
where
    T: Serialize,
    Fut: std::future::Future<Output = GatewayResult<T>>,
{
    if let Some(response) = require_admin(state, &headers).await {
        return response;
    }
    match query(state.store.clone()).await {
        Ok(value) => Json(value).into_response(),
        Err(error) => error_response(&headers, error),
    }
}

async fn metrics() -> Response {
    (
        StatusCode::OK,
        [("content-type", "text/plain; version=0.0.4")],
        gateway_telemetry::prometheus(),
    )
        .into_response()
}

async fn admin_ui_index() -> Response {
    static_response(
        "text/html; charset=utf-8",
        include_str!("static/admin-ui/index.html"),
    )
}

async fn admin_ui_asset(Path(path): Path<String>) -> Response {
    match path.as_str() {
        "" | "index.html" => admin_ui_index().await,
        "app.css" => static_response(
            "text/css; charset=utf-8",
            include_str!("static/admin-ui/app.css"),
        ),
        "app.js" => static_response(
            "application/javascript; charset=utf-8",
            include_str!("static/admin-ui/app.js"),
        ),
        _ => StatusCode::NOT_FOUND.into_response(),
    }
}

fn static_response(content_type: &'static str, body: &'static str) -> Response {
    (StatusCode::OK, [("content-type", content_type)], body).into_response()
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
    if let Some(response) = require_admin(&state, &headers).await {
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

async fn mutate_service_enabled(
    state: AppState,
    headers: HeaderMap,
    service_name: String,
    enabled: bool,
) -> Response {
    if let Some(response) = require_admin(&state, &headers).await {
        return response;
    }

    match state
        .store
        .set_service_enabled(&service_name, enabled)
        .await
    {
        Ok(Some(service)) => Json(service).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(error) => error_response(&headers, error),
    }
}

async fn require_admin(state: &AppState, headers: &HeaderMap) -> Option<Response> {
    let token = match bearer_token(headers) {
        Ok(token) => token,
        Err(error) => return Some(error_response(headers, error)),
    };

    match state.store.verify_operator_token(token, Utc::now()).await {
        Ok(()) => None,
        Err(error) => Some(error_response(headers, error)),
    }
}

fn bearer_token(headers: &HeaderMap) -> GatewayResult<&str> {
    let Some(authorization) = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
    else {
        return Err(GatewayError::MissingAuthorization);
    };
    let Some(token) = authorization
        .strip_prefix("Bearer ")
        .map(str::trim)
        .filter(|token| !token.is_empty())
    else {
        return Err(GatewayError::MalformedAuthorization);
    };
    Ok(token)
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
        OperatorTokenResponse, ProviderHealth, ServiceCostMode, ServiceResponse, ServiceSource,
        ServiceSyncStatus, ServiceSyncStatusResponse, UsageBreakdown, UsageSummary,
        UsageTimeseriesPoint,
    };
    use std::sync::Mutex;
    use tower::ServiceExt;
    use uuid::Uuid;

    #[derive(Clone)]
    struct MemoryStore {
        key: Arc<Mutex<Option<StoredVirtualKey>>>,
        admin_key: Arc<Mutex<Option<AdminKeyResponse>>>,
        services: Arc<Mutex<Vec<ServiceResponse>>>,
        operator_tokens: Arc<Mutex<Vec<String>>>,
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
                    allowed_services: Vec::new(),
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
                input_tokens: 0,
                output_tokens: 0,
                total_tokens: 0,
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
                input_tokens: 0,
                output_tokens: 0,
                total_tokens: 0,
                estimated_cost_usd: None,
            })
        }
    }

    #[async_trait]
    impl AdminServiceStore for MemoryStore {
        async fn create_service(
            &self,
            request: ServiceCreateRequest,
        ) -> GatewayResult<ServiceResponse> {
            request.validate()?;
            let mut services = self.services.lock().expect("lock poisoned");
            if services.iter().any(|service| service.name == request.name) {
                return Err(GatewayError::DuplicateService);
            }
            let now = Utc::now();
            let response = ServiceResponse {
                name: request.name.clone(),
                studio_service_id: request.studio_service_id.clone(),
                route_pattern: request
                    .route_pattern
                    .clone()
                    .unwrap_or_else(|| format!("/services/{}/*", request.name)),
                upstream_base_url: request.upstream_base_url.clone(),
                enabled: request.enabled,
                allowed_methods: request.allowed_methods.clone(),
                credential_configured: request.credential.is_some(),
                timeout_ms: request.timeout_ms,
                max_body_bytes: request.max_body_bytes,
                cost_mode: request.cost_mode,
                estimated_cost_usd: request.estimated_cost_usd,
                fallback_services: request.fallback_services.clone(),
                source: if request.studio_service_id.is_some() {
                    ServiceSource::Studio
                } else {
                    ServiceSource::Gateway
                },
                sync_status: if request.upstream_base_url.is_some() && request.credential.is_some()
                {
                    ServiceSyncStatus::Local
                } else {
                    ServiceSyncStatus::Incomplete
                },
                last_synced_at: None,
                disabled_at: None,
                created_at: now,
                updated_at: now,
                missing_runtime_fields: missing_runtime_fields(
                    request.upstream_base_url.as_deref(),
                    request.credential.as_deref(),
                ),
            };
            services.push(response.clone());
            Ok(response)
        }

        async fn list_services(&self) -> GatewayResult<Vec<ServiceResponse>> {
            Ok(self.services.lock().expect("lock poisoned").clone())
        }

        async fn get_service(&self, name: &str) -> GatewayResult<Option<ServiceResponse>> {
            Ok(self
                .services
                .lock()
                .expect("lock poisoned")
                .iter()
                .find(|service| service.name == name)
                .cloned())
        }

        async fn patch_service(
            &self,
            name: &str,
            patch: ServicePatchRequest,
        ) -> GatewayResult<Option<ServiceResponse>> {
            patch.validate()?;
            let mut services = self.services.lock().expect("lock poisoned");
            let Some(service) = services.iter_mut().find(|service| service.name == name) else {
                return Ok(None);
            };
            if let Some(enabled) = patch.enabled {
                service.enabled = enabled;
            }
            if let Some(upstream_base_url) = patch.upstream_base_url {
                service.upstream_base_url = upstream_base_url;
            }
            if let Some(credential) = patch.credential {
                service.credential_configured = credential.is_some();
            }
            service.updated_at = Utc::now();
            service.missing_runtime_fields = if service.credential_configured {
                missing_runtime_fields(service.upstream_base_url.as_deref(), Some("configured"))
            } else {
                missing_runtime_fields(service.upstream_base_url.as_deref(), None)
            };
            Ok(Some(service.clone()))
        }

        async fn delete_service(&self, name: &str) -> GatewayResult<bool> {
            let mut services = self.services.lock().expect("lock poisoned");
            let before = services.len();
            services.retain(|service| service.name != name);
            Ok(services.len() != before)
        }

        async fn set_service_enabled(
            &self,
            name: &str,
            enabled: bool,
        ) -> GatewayResult<Option<ServiceResponse>> {
            let mut services = self.services.lock().expect("lock poisoned");
            let Some(service) = services.iter_mut().find(|service| service.name == name) else {
                return Ok(None);
            };
            service.enabled = enabled;
            service.disabled_at = if enabled { None } else { Some(Utc::now()) };
            Ok(Some(service.clone()))
        }

        async fn import_studio_service(
            &self,
            request: StudioServiceImportRequest,
        ) -> GatewayResult<ServiceResponse> {
            self.sync_studio_service(request).await
        }

        async fn sync_studio_service(
            &self,
            request: StudioServiceImportRequest,
        ) -> GatewayResult<ServiceResponse> {
            request.validate()?;
            let mut services = self.services.lock().expect("lock poisoned");
            let now = Utc::now();
            if let Some(service) = services.iter_mut().find(|service| {
                service.studio_service_id.as_deref() == Some(&request.studio_service_id)
            }) {
                service.name = request.name;
                service.route_pattern = request
                    .route_pattern
                    .unwrap_or_else(|| format!("/services/{}/*", service.name));
                service.source = ServiceSource::Studio;
                service.sync_status = if service.missing_runtime_fields.is_empty() {
                    ServiceSyncStatus::Synced
                } else {
                    ServiceSyncStatus::Incomplete
                };
                service.last_synced_at = Some(now);
                service.updated_at = now;
                return Ok(service.clone());
            }

            let response = ServiceResponse {
                name: request.name.clone(),
                studio_service_id: Some(request.studio_service_id),
                route_pattern: request
                    .route_pattern
                    .unwrap_or_else(|| format!("/services/{}/*", request.name)),
                upstream_base_url: None,
                enabled: false,
                allowed_methods: vec!["POST".to_owned()],
                credential_configured: false,
                timeout_ms: 60_000,
                max_body_bytes: 2_097_152,
                cost_mode: request
                    .default_pricing
                    .as_ref()
                    .map(|pricing| pricing.cost_mode)
                    .unwrap_or(ServiceCostMode::None),
                estimated_cost_usd: request
                    .default_pricing
                    .as_ref()
                    .and_then(|pricing| pricing.estimated_cost_usd),
                fallback_services: Vec::new(),
                source: ServiceSource::Studio,
                sync_status: ServiceSyncStatus::Incomplete,
                last_synced_at: Some(now),
                disabled_at: None,
                created_at: now,
                updated_at: now,
                missing_runtime_fields: vec![
                    "upstream_base_url".to_owned(),
                    "credential".to_owned(),
                ],
            };
            services.push(response.clone());
            Ok(response)
        }

        async fn service_sync_status(
            &self,
            name: &str,
        ) -> GatewayResult<Option<ServiceSyncStatusResponse>> {
            Ok(self
                .services
                .lock()
                .expect("lock poisoned")
                .iter()
                .find(|service| service.name == name)
                .map(|service| ServiceSyncStatusResponse {
                    name: service.name.clone(),
                    source: service.source,
                    sync_status: service.sync_status,
                    last_synced_at: service.last_synced_at,
                    missing_runtime_fields: service.missing_runtime_fields.clone(),
                }))
        }
    }

    #[async_trait]
    impl OperatorTokenStore for MemoryStore {
        async fn bootstrap_operator_token(
            &self,
            material: &OperatorTokenMaterial,
        ) -> GatewayResult<Option<OperatorTokenResponse>> {
            let mut tokens = self.operator_tokens.lock().expect("lock poisoned");
            if !tokens.is_empty() {
                return Ok(None);
            }
            tokens.push(material.raw_token.clone());
            Ok(Some(operator_response(&material.token_prefix)))
        }

        async fn verify_operator_token(
            &self,
            raw_token: &str,
            _now: chrono::DateTime<Utc>,
        ) -> GatewayResult<()> {
            if self
                .operator_tokens
                .lock()
                .expect("lock poisoned")
                .iter()
                .any(|token| token == raw_token)
            {
                Ok(())
            } else {
                Err(GatewayError::InvalidOperatorToken)
            }
        }

        async fn rotate_operator_token(
            &self,
            current_raw_token: &str,
            material: &OperatorTokenMaterial,
            _now: chrono::DateTime<Utc>,
        ) -> GatewayResult<OperatorTokenResponse> {
            let mut tokens = self.operator_tokens.lock().expect("lock poisoned");
            let Some(position) = tokens.iter().position(|token| token == current_raw_token) else {
                return Err(GatewayError::InvalidOperatorToken);
            };
            tokens.remove(position);
            tokens.push(material.raw_token.clone());
            Ok(operator_response(&material.token_prefix))
        }
    }

    #[async_trait]
    impl UsageQueryStore for MemoryStore {
        async fn usage_summary(&self, _query: UsageQuery) -> GatewayResult<UsageSummary> {
            Ok(UsageSummary::default())
        }

        async fn usage_timeseries(
            &self,
            _query: UsageQuery,
        ) -> GatewayResult<Vec<UsageTimeseriesPoint>> {
            Ok(Vec::new())
        }

        async fn usage_breakdown(
            &self,
            _query: UsageQuery,
            _dimension: UsageBreakdownDimension,
        ) -> GatewayResult<Vec<UsageBreakdown>> {
            Ok(Vec::new())
        }

        async fn provider_health(&self, _query: UsageQuery) -> GatewayResult<Vec<ProviderHealth>> {
            Ok(Vec::new())
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

    fn missing_runtime_fields(upstream: Option<&str>, credential: Option<&str>) -> Vec<String> {
        let mut fields = Vec::new();
        if upstream.is_none_or(str::is_empty) {
            fields.push("upstream_base_url".to_owned());
        }
        if credential.is_none_or(str::is_empty) {
            fields.push("credential".to_owned());
        }
        fields
    }

    fn operator_response(token_prefix: &str) -> OperatorTokenResponse {
        let now = Utc::now();
        OperatorTokenResponse {
            id: Uuid::new_v4(),
            token_prefix: token_prefix.to_owned(),
            disabled: false,
            revoked_at: None,
            last_used_at: None,
            created_at: now,
            updated_at: now,
        }
    }

    const TEST_OPERATOR_TOKEN: &str =
        "op_live_testoperator000000000000000000000000000000000000000000000000";

    fn test_state(store: MemoryStore) -> AppState {
        test_state_with_redis_url(store, "redis://127.0.0.1:6379")
    }

    fn test_state_with_redis_url(store: MemoryStore, redis_url: &str) -> AppState {
        let redis = RedisReadiness::new(redis_url).expect("redis client");
        AppState {
            store: Arc::new(store),
            redis,
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

    async fn admin_get(app: Router, route: &str, token: Option<&str>) -> Response {
        let mut builder = axum::http::Request::builder()
            .method(axum::http::Method::GET)
            .uri(route)
            .header("x-request-id", "req_test");
        if let Some(token) = token {
            builder = builder.header("authorization", format!("Bearer {token}"));
        }

        app.oneshot(builder.body(axum::body::Body::empty()).expect("request"))
            .await
            .expect("response")
    }

    async fn admin_patch(app: Router, route: &str, token: Option<&str>, body: &str) -> Response {
        let mut builder = axum::http::Request::builder()
            .method(axum::http::Method::PATCH)
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
            services: Arc::new(Mutex::new(Vec::new())),
            operator_tokens: Arc::new(Mutex::new(vec![TEST_OPERATOR_TOKEN.to_owned()])),
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
            services: Arc::new(Mutex::new(Vec::new())),
            operator_tokens: Arc::new(Mutex::new(vec![TEST_OPERATOR_TOKEN.to_owned()])),
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
            services: Arc::new(Mutex::new(Vec::new())),
            operator_tokens: Arc::new(Mutex::new(vec![TEST_OPERATOR_TOKEN.to_owned()])),
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
            services: Arc::new(Mutex::new(Vec::new())),
            operator_tokens: Arc::new(Mutex::new(vec![TEST_OPERATOR_TOKEN.to_owned()])),
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
            services: Arc::new(Mutex::new(Vec::new())),
            operator_tokens: Arc::new(Mutex::new(vec![TEST_OPERATOR_TOKEN.to_owned()])),
            events: Arc::new(Mutex::new(Vec::new())),
            postgres_ready: true,
        };
        let app = router_with_state(test_state(store));
        let project_id = Uuid::new_v4();
        let response = admin_post(
            app,
            "/admin/keys",
            Some(TEST_OPERATOR_TOKEN),
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

    #[tokio::test]
    async fn metrics_endpoint_is_scrapeable_without_admin_token() {
        let store = MemoryStore {
            key: Arc::new(Mutex::new(None)),
            admin_key: Arc::new(Mutex::new(None)),
            services: Arc::new(Mutex::new(Vec::new())),
            operator_tokens: Arc::new(Mutex::new(vec![TEST_OPERATOR_TOKEN.to_owned()])),
            events: Arc::new(Mutex::new(Vec::new())),
            postgres_ready: true,
        };
        let app = router_with_state(test_state(store));
        let response = request(app, "/metrics").await;

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn task_usage_requires_admin_token_and_returns_summary() {
        let store = MemoryStore {
            key: Arc::new(Mutex::new(None)),
            admin_key: Arc::new(Mutex::new(None)),
            services: Arc::new(Mutex::new(Vec::new())),
            operator_tokens: Arc::new(Mutex::new(vec![TEST_OPERATOR_TOKEN.to_owned()])),
            events: Arc::new(Mutex::new(Vec::new())),
            postgres_ready: true,
        };
        let app = router_with_state(test_state(store));
        let response = admin_get(app, "/admin/tasks/task-1/usage", Some(TEST_OPERATOR_TOKEN)).await;

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn admin_ui_assets_are_served_without_exposing_operator_token() {
        let store = MemoryStore {
            key: Arc::new(Mutex::new(None)),
            admin_key: Arc::new(Mutex::new(None)),
            services: Arc::new(Mutex::new(Vec::new())),
            operator_tokens: Arc::new(Mutex::new(vec![TEST_OPERATOR_TOKEN.to_owned()])),
            events: Arc::new(Mutex::new(Vec::new())),
            postgres_ready: true,
        };
        let app = router_with_state(test_state(store));
        let response = request(app, "/admin-ui").await;

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let body = String::from_utf8(body.to_vec()).expect("utf8");
        assert!(body.contains("Relayna Gateway Admin"));
        assert!(!body.contains(TEST_OPERATOR_TOKEN));
    }

    #[tokio::test]
    async fn operator_token_rotation_returns_new_raw_token_once_and_invalidates_old_token() {
        let store = MemoryStore {
            key: Arc::new(Mutex::new(None)),
            admin_key: Arc::new(Mutex::new(None)),
            services: Arc::new(Mutex::new(Vec::new())),
            operator_tokens: Arc::new(Mutex::new(vec![TEST_OPERATOR_TOKEN.to_owned()])),
            events: Arc::new(Mutex::new(Vec::new())),
            postgres_ready: true,
        };
        let app = router_with_state(test_state(store));
        let response = admin_post(
            app.clone(),
            "/admin/operator-token/rotate",
            Some(TEST_OPERATOR_TOKEN),
            "{}",
        )
        .await;

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let value: serde_json::Value = serde_json::from_slice(&body).expect("json");
        let new_token = value["raw_token"].as_str().expect("raw token");
        assert!(new_token.starts_with("op_live_"));
        assert!(value["token"].get("token_hash").is_none());

        let old_response = admin_get(
            app.clone(),
            "/admin/usage/summary",
            Some(TEST_OPERATOR_TOKEN),
        )
        .await;
        assert_eq!(old_response.status(), StatusCode::UNAUTHORIZED);
        let new_response = admin_get(app, "/admin/usage/summary", Some(new_token)).await;
        assert_eq!(new_response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn admin_service_create_redacts_raw_credential() {
        let store = MemoryStore {
            key: Arc::new(Mutex::new(None)),
            admin_key: Arc::new(Mutex::new(None)),
            services: Arc::new(Mutex::new(Vec::new())),
            operator_tokens: Arc::new(Mutex::new(vec![TEST_OPERATOR_TOKEN.to_owned()])),
            events: Arc::new(Mutex::new(Vec::new())),
            postgres_ready: true,
        };
        let app = router_with_state(test_state(store));
        let response = admin_post(
            app,
            "/admin/services",
            Some(TEST_OPERATOR_TOKEN),
            r#"{
                "name":"summary",
                "route_pattern":"/summary",
                "upstream_base_url":"http://summary.internal:8080",
                "allowed_methods":["POST"],
                "credential":"internal-summary-token",
                "timeout_ms":60000,
                "max_body_bytes":1048576,
                "cost_mode":"fixed",
                "estimated_cost_usd":0.01,
                "enabled":true
            }"#,
        )
        .await;

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let value: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(value["name"], "summary");
        assert_eq!(value["credential_configured"], true);
        assert!(value.get("credential").is_none());
    }

    #[tokio::test]
    async fn admin_service_import_reports_incomplete_runtime_fields() {
        let store = MemoryStore {
            key: Arc::new(Mutex::new(None)),
            admin_key: Arc::new(Mutex::new(None)),
            services: Arc::new(Mutex::new(Vec::new())),
            operator_tokens: Arc::new(Mutex::new(vec![TEST_OPERATOR_TOKEN.to_owned()])),
            events: Arc::new(Mutex::new(Vec::new())),
            postgres_ready: true,
        };
        let app = router_with_state(test_state(store));
        let response = admin_post(
            app.clone(),
            "/admin/services/import",
            Some(TEST_OPERATOR_TOKEN),
            r#"{
                "studio_service_id":"svc_1",
                "name":"translation",
                "route_pattern":"/translation",
                "category":"language",
                "default_pricing":{"cost_mode":"fixed","estimated_cost_usd":0.02}
            }"#,
        )
        .await;

        assert_eq!(response.status(), StatusCode::OK);
        let status = admin_get(
            app,
            "/admin/services/translation/sync-status",
            Some(TEST_OPERATOR_TOKEN),
        )
        .await;
        let body = axum::body::to_bytes(status.into_body(), usize::MAX)
            .await
            .expect("body");
        let value: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(value["sync_status"], "incomplete");
        assert_eq!(value["missing_runtime_fields"][0], "upstream_base_url");
        assert_eq!(value["missing_runtime_fields"][1], "credential");
    }

    #[tokio::test]
    async fn admin_service_patch_can_configure_imported_service() {
        let store = MemoryStore {
            key: Arc::new(Mutex::new(None)),
            admin_key: Arc::new(Mutex::new(None)),
            services: Arc::new(Mutex::new(Vec::new())),
            operator_tokens: Arc::new(Mutex::new(vec![TEST_OPERATOR_TOKEN.to_owned()])),
            events: Arc::new(Mutex::new(Vec::new())),
            postgres_ready: true,
        };
        let app = router_with_state(test_state(store));
        let _ = admin_post(
            app.clone(),
            "/admin/services/import",
            Some(TEST_OPERATOR_TOKEN),
            r#"{"studio_service_id":"svc_1","name":"translation","route_pattern":"/translation"}"#,
        )
        .await;
        let response = admin_patch(
            app,
            "/admin/services/translation",
            Some(TEST_OPERATOR_TOKEN),
            r#"{"upstream_base_url":"http://translation.internal:8080","credential":"token","enabled":true}"#,
        )
        .await;

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let value: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(value["enabled"], true);
        assert_eq!(value["credential_configured"], true);
        assert!(value["missing_runtime_fields"]
            .as_array()
            .unwrap()
            .is_empty());
    }
}
