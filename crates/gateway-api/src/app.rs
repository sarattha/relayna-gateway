use async_trait::async_trait;
use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, Method, StatusCode},
    response::{IntoResponse, Response},
    routing::{delete, get, patch, post},
    Json, Router,
};
use chrono::Utc;
use gateway_core::CircuitBreakerState;
use gateway_core::{
    auth::{Authenticator, VirtualKeyLookup},
    evaluate_policy, evaluate_policy_limits, extract_generation_features,
    guardrail_executor_for_definitions, resolve_guardrail_plan, AdminAuditStore,
    AdminGuardrailDefinitionResponse, AdminKeyCreate, AdminKeyPatch, AdminKeyResponse,
    AdminKeyStore, AdminOpenAiRouteStore, AdminPolicyLayerStore, AdminPolicyLayerUpsert,
    AdminProjectStore, AdminProviderConfigStore, AdminServiceStore, AdminStudioConnectionStore,
    AuditEvent, AuditEventCreate, AuditEventQuery, CreatedAdminKeyResponse,
    CreatedOperatorTokenResponse, EffectiveStudioConnection, GatewayError, GatewayResult,
    GuardrailAdminCreateRequest, GuardrailAdminPatchRequest, GuardrailDefinitionResponse,
    GuardrailEventQuery, GuardrailExecutionEvent, GuardrailExecutionSummary, GuardrailMode,
    GuardrailObservabilityStore, GuardrailPlanRequest, GuardrailPolicySet, GuardrailStore,
    GuardrailTestRequest, GuardrailTestResponse, KeyPolicy, OperatorAuthorization,
    OperatorTokenMaterial, OperatorTokenStore, PolicyLookup, ProjectCreateRequest,
    ProjectPatchRequest, ProjectResponse, Provider, ProviderConfigCreateRequest,
    ProviderConfigPatchRequest, ProviderConfigResponse, ProviderHealthState, ProviderHealthStatus,
    ProviderIntelligenceStore, Route, ServiceCreateRequest, ServiceImportDiff,
    ServiceImportValidationIssue, ServicePatchRequest, ServiceRegistrySnapshot, ServiceResponse,
    StudioConnectionEnv, StudioConnectionPatchRequest, StudioConnectionTestResponse,
    StudioServiceCatalogResponse, StudioServiceImportPreview, StudioServiceImportRequest,
    UsageBreakdownDimension, UsageEvent, UsageExport, UsageQuery, UsageQueryStore,
    VirtualKeyMaterial, SCOPE_AUDIT_READ, SCOPE_GUARDRAILS_UPDATE, SCOPE_KEYS_CREATE,
    SCOPE_KEYS_DISABLE, SCOPE_OPERATORS_MANAGE, SCOPE_POLICIES_UPDATE, SCOPE_PROVIDERS_UPDATE,
    SCOPE_SERVICES_UPDATE, SCOPE_SETTINGS_UPDATE, SCOPE_USAGE_EXPORT, SCOPE_USAGE_READ,
};
use gateway_store::{PostgresStore, RedisReadiness};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{sync::Arc, time::Duration};
use tower_http::{
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer},
    trace::TraceLayer,
};

#[async_trait]
pub trait GatewayData:
    VirtualKeyLookup
    + PolicyLookup
    + AdminKeyStore
    + AdminPolicyLayerStore
    + AdminOpenAiRouteStore
    + AdminProjectStore
    + AdminProviderConfigStore
    + AdminServiceStore
    + AdminStudioConnectionStore
    + GuardrailStore
    + GuardrailObservabilityStore
    + ProviderIntelligenceStore
    + OperatorTokenStore
    + AdminAuditStore
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
    studio_env: StudioConnectionEnv,
}

const STUDIO_CATALOG_TIMEOUT: Duration = Duration::from_secs(8);

#[derive(Clone)]
pub struct StudioCatalogClient {
    base_url: String,
    token: Option<String>,
    client: reqwest::Client,
}

impl StudioCatalogClient {
    pub fn new(base_url: impl Into<String>, token: Option<String>) -> Self {
        Self {
            base_url: base_url.into().trim().trim_end_matches('/').to_owned(),
            token,
            client: reqwest::Client::new(),
        }
    }

    async fn services(&self) -> GatewayResult<Vec<StudioServiceImportPreview>> {
        let url = format!("{}/studio/gateway/services", self.base_url);
        let mut request = self.client.get(url).timeout(STUDIO_CATALOG_TIMEOUT);
        if let Some(token) = &self.token {
            request = request.bearer_auth(token);
        }
        let response = request
            .send()
            .await
            .map_err(|_| GatewayError::StudioUnavailable)?;
        if !response.status().is_success() {
            return Err(GatewayError::StudioUnavailable);
        }
        let value = response
            .json::<serde_json::Value>()
            .await
            .map_err(|_| GatewayError::StudioUnavailable)?;
        let catalog = if value.is_array() {
            StudioServiceCatalogResponse {
                services: serde_json::from_value(value)
                    .map_err(|_| GatewayError::StudioUnavailable)?,
            }
        } else {
            serde_json::from_value::<StudioServiceCatalogResponse>(value)
                .map_err(|_| GatewayError::StudioUnavailable)?
        };

        catalog
            .services
            .into_iter()
            .map(|service| service.into_preview())
            .collect()
    }
}

pub fn router(store: PostgresStore, redis: RedisReadiness) -> Router {
    router_with_state(AppState {
        store: Arc::new(store),
        redis,
        studio_env: StudioConnectionEnv::default(),
    })
}

pub fn router_with_studio(
    store: PostgresStore,
    redis: RedisReadiness,
    studio: Option<StudioCatalogClient>,
) -> Router {
    let studio_env = studio
        .map(|studio| StudioConnectionEnv {
            base_url: Some(studio.base_url),
            token: studio.token,
        })
        .unwrap_or_default();
    router_with_state(AppState {
        store: Arc::new(store),
        redis,
        studio_env,
    })
}

pub fn router_with_state(state: AppState) -> Router {
    Router::new()
        .route("/admin-ui/healthz", get(healthz))
        .route("/admin-ui/readyz", get(readyz))
        .route("/admin-ui/v1/guardrails", get(list_guardrails))
        .route("/admin-ui/v1/guardrails/test", post(test_guardrails))
        .route(
            "/admin-ui/admin/guardrails",
            get(admin_guardrails).post(create_admin_guardrail),
        )
        .route(
            "/admin-ui/admin/guardrails/{name}",
            patch(patch_admin_guardrail).delete(delete_admin_guardrail),
        )
        .route(
            "/admin-ui/admin/guardrails/executions",
            get(admin_guardrail_executions),
        )
        .route(
            "/admin-ui/admin/guardrails/summary",
            get(admin_guardrail_summary),
        )
        .route("/admin-ui/admin/audit-events", get(list_audit_events))
        .route("/admin-ui/admin/policy/simulate", post(simulate_policy))
        .route(
            "/admin-ui/admin/policy-layers",
            get(list_policy_layers).post(upsert_policy_layer),
        )
        .route(
            "/admin-ui/admin/policy-layers/{layer_id}",
            delete(delete_policy_layer),
        )
        .route("/admin-ui/admin/keys", post(create_key).get(list_keys))
        .route(
            "/admin-ui/admin/keys/{key_id}",
            get(get_key).patch(patch_key),
        )
        .route("/admin-ui/admin/keys/{key_id}/revoke", post(revoke_key))
        .route("/admin-ui/admin/keys/{key_id}/disable", post(disable_key))
        .route("/admin-ui/admin/keys/{key_id}/enable", post(enable_key))
        .route("/admin-ui/admin/keys/{key_id}/usage", get(key_usage))
        .route(
            "/admin-ui/admin/projects",
            post(create_project).get(list_projects),
        )
        .route(
            "/admin-ui/admin/projects/{project_id}",
            get(get_project).patch(patch_project).delete(delete_project),
        )
        .route(
            "/admin-ui/admin/operator-token/rotate",
            post(rotate_operator_token),
        )
        .route(
            "/admin-ui/admin/providers",
            post(create_provider).get(list_providers),
        )
        .route(
            "/admin-ui/admin/providers/{provider_id}",
            get(get_provider)
                .patch(patch_provider)
                .delete(delete_provider),
        )
        .route(
            "/admin-ui/admin/providers/{provider_id}/disable",
            post(disable_provider),
        )
        .route(
            "/admin-ui/admin/providers/{provider_id}/enable",
            post(enable_provider),
        )
        .route("/admin-ui/admin/openai-routes", get(list_openai_routes))
        .route(
            "/admin-ui/admin/openai-routes/{route_id}/disable",
            post(disable_openai_route),
        )
        .route(
            "/admin-ui/admin/openai-routes/{route_id}/enable",
            post(enable_openai_route),
        )
        .route(
            "/admin-ui/admin/services",
            post(create_service).get(list_services),
        )
        .route(
            "/admin-ui/admin/studio/connection",
            get(get_studio_connection).patch(patch_studio_connection),
        )
        .route(
            "/admin-ui/admin/studio/connection/test",
            post(test_studio_connection),
        )
        .route("/admin-ui/admin/studio/services", get(studio_services))
        .route("/admin-ui/admin/services/import", post(import_service))
        .route("/admin-ui/admin/services/sync", post(sync_service))
        .route(
            "/admin-ui/admin/services/{service_name}",
            get(get_service).patch(patch_service).delete(delete_service),
        )
        .route(
            "/admin-ui/admin/services/{service_name}/disable",
            post(disable_service),
        )
        .route(
            "/admin-ui/admin/services/{service_name}/enable",
            post(enable_service),
        )
        .route(
            "/admin-ui/admin/services/{service_name}/sync-status",
            get(service_sync_status),
        )
        .route(
            "/admin-ui/admin/projects/{project_id}/usage",
            get(project_usage),
        )
        .route("/admin-ui/admin/usage/summary", get(usage_summary))
        .route("/admin-ui/admin/usage/timeseries", get(usage_timeseries))
        .route("/admin-ui/admin/usage/by-key", get(usage_by_key))
        .route("/admin-ui/admin/usage/by-project", get(usage_by_project))
        .route("/admin-ui/admin/usage/by-model", get(usage_by_model))
        .route("/admin-ui/admin/usage/by-provider", get(usage_by_provider))
        .route("/admin-ui/admin/usage/by-service", get(usage_by_service))
        .route("/admin-ui/admin/usage/by-task", get(usage_by_task))
        .route("/admin-ui/admin/usage/export.json", get(usage_export_json))
        .route("/admin-ui/admin/usage/export.csv", get(usage_export_csv))
        .route("/admin-ui/admin/tasks/{task_id}/usage", get(task_usage))
        .route("/admin-ui/admin/provider-health", get(provider_health))
        .route(
            "/admin-ui/admin/provider-health/state",
            get(provider_health_state).post(upsert_provider_health_state),
        )
        .route(
            "/admin-ui/admin/provider-health/check",
            post(run_provider_health_checks),
        )
        .route(
            "/admin-ui/admin/debug-bundles/{request_id}",
            get(get_debug_bundle),
        )
        .route(
            "/admin-ui/admin/services/import/preview",
            post(preview_service_import),
        )
        .route(
            "/admin-ui/admin/services/import/activate",
            post(activate_service_import),
        )
        .route(
            "/admin-ui/admin/services/import/versions",
            get(service_import_versions),
        )
        .route(
            "/admin-ui/admin/services/import/rollback/{version}",
            post(rollback_service_import),
        )
        .route("/admin-ui/metrics", get(metrics))
        .route("/admin-ui", get(admin_ui_index))
        .route("/admin-ui/{*path}", get(admin_ui_asset))
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

async fn list_guardrails(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let key = match require_virtual_key(&state, &headers).await {
        Ok(key) => key,
        Err(error) => return error_response(&headers, error),
    };
    let definitions = match state.store.list_guardrail_definitions().await {
        Ok(definitions) => definitions,
        Err(error) => return error_response(&headers, error),
    };
    let policy = match state.store.guardrail_policy_for_key(key.key_id).await {
        Ok(policy) => policy,
        Err(error) => return error_response(&headers, error),
    };
    let guardrails = definitions
        .into_iter()
        .filter(|definition| {
            definition.default_on
                || policy
                    .mandatory_guardrails
                    .iter()
                    .any(|name| name == &definition.name)
                || policy
                    .optional_guardrails
                    .iter()
                    .any(|name| name == &definition.name)
        })
        .filter(|definition| {
            !policy
                .forbidden_guardrails
                .iter()
                .any(|name| name == &definition.name)
        })
        .map(|definition| definition.response())
        .collect();

    Json(GuardrailListResponse { guardrails }).into_response()
}

async fn test_guardrails(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<GuardrailTestRequest>,
) -> Response {
    let key = match require_virtual_key(&state, &headers).await {
        Ok(key) => key,
        Err(error) => return error_response(&headers, error),
    };
    if request.mode == GuardrailMode::DuringCall {
        return error_response(&headers, GatewayError::InvalidGuardrailRequest);
    }
    let definitions = match state.store.list_guardrail_definitions().await {
        Ok(definitions) => definitions,
        Err(error) => return error_response(&headers, error),
    };
    let policy = match state.store.guardrail_policy_for_key(key.key_id).await {
        Ok(policy) => policy,
        Err(error) => return error_response(&headers, error),
    };
    let executor = guardrail_executor_for_definitions(&definitions);
    let plan = match resolve_guardrail_plan(GuardrailPlanRequest {
        mode: request.mode,
        definitions,
        policies: GuardrailPolicySet {
            key_policy: policy,
            ..GuardrailPolicySet::default()
        },
        client_requested_guardrails: request.guardrails,
    }) {
        Ok(plan) => plan,
        Err(error) => return error_response(&headers, error),
    };
    let context = gateway_core::GuardrailContext {
        request_id: request_id_from_headers(&headers),
        key_id: Some(key.key_id),
        project_id: key.project_id,
        ..gateway_core::GuardrailContext::default()
    };
    let execution = match executor.execute(
        &plan,
        request.mode,
        context,
        if request.mode == GuardrailMode::PreCall {
            Some(request.input.clone())
        } else {
            None
        },
        if request.mode == GuardrailMode::PostCall {
            Some(request.input.clone())
        } else {
            None
        },
    ) {
        Ok(execution) => execution,
        Err(error) => return error_response(&headers, error),
    };
    let input = if request.mode == GuardrailMode::PreCall {
        execution.request.unwrap_or(request.input)
    } else {
        execution.response.unwrap_or(request.input)
    };
    Json(GuardrailTestResponse {
        input,
        applied_guardrails: execution.context.applied_guardrails,
        results: execution.records,
    })
    .into_response()
}

async fn admin_guardrails(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Some(response) = require_admin(&state, &headers).await {
        return response;
    }
    match state.store.list_admin_guardrail_definitions().await {
        Ok(guardrails) => Json(AdminGuardrailListResponse { guardrails }).into_response(),
        Err(error) => error_response(&headers, error),
    }
}

async fn admin_guardrail_executions(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<GuardrailEventQuery>,
) -> Response {
    if let Some(response) = require_admin(&state, &headers).await {
        return response;
    }
    match state.store.guardrail_execution_events(query).await {
        Ok(executions) => Json(AdminGuardrailExecutionListResponse { executions }).into_response(),
        Err(error) => error_response(&headers, error),
    }
}

async fn admin_guardrail_summary(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<GuardrailEventQuery>,
) -> Response {
    if let Some(response) = require_admin(&state, &headers).await {
        return response;
    }
    match state.store.guardrail_execution_summary(query).await {
        Ok(summary) => Json(AdminGuardrailSummaryResponse { summary }).into_response(),
        Err(error) => error_response(&headers, error),
    }
}

async fn list_audit_events(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<AuditEventQuery>,
) -> Response {
    admin_query(headers, &state, SCOPE_AUDIT_READ, |store| async move {
        store.list_audit_events(query).await
    })
    .await
}

async fn create_admin_guardrail(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<GuardrailAdminCreateRequest>,
) -> Response {
    let actor = match require_admin_scope(&state, &headers, SCOPE_GUARDRAILS_UPDATE).await {
        Ok(actor) => actor,
        Err(response) => return response,
    };
    match state.store.create_http_guardrail(request).await {
        Ok(guardrail) => {
            if let Err(error) = record_admin_audit(
                &state,
                &headers,
                &actor,
                "guardrails:create",
                "guardrail",
                Some(guardrail.name.clone()),
                None,
                audit_json(&guardrail),
            )
            .await
            {
                return error_response(&headers, error);
            }
            Json(guardrail).into_response()
        }
        Err(error) => error_response(&headers, error),
    }
}

async fn patch_admin_guardrail(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(name): Path<String>,
    Json(request): Json<GuardrailAdminPatchRequest>,
) -> Response {
    let actor = match require_admin_scope(&state, &headers, SCOPE_GUARDRAILS_UPDATE).await {
        Ok(actor) => actor,
        Err(response) => return response,
    };
    match state.store.patch_admin_guardrail(name, request).await {
        Ok(guardrail) => {
            if let Err(error) = record_admin_audit(
                &state,
                &headers,
                &actor,
                "guardrails:update",
                "guardrail",
                Some(guardrail.name.clone()),
                None,
                audit_json(&guardrail),
            )
            .await
            {
                return error_response(&headers, error);
            }
            Json(guardrail).into_response()
        }
        Err(error) => error_response(&headers, error),
    }
}

async fn delete_admin_guardrail(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(name): Path<String>,
) -> Response {
    let actor = match require_admin_scope(&state, &headers, SCOPE_GUARDRAILS_UPDATE).await {
        Ok(actor) => actor,
        Err(response) => return response,
    };
    match state.store.delete_admin_guardrail(name.clone()).await {
        Ok(()) => {
            if let Err(error) = record_admin_audit(
                &state,
                &headers,
                &actor,
                "guardrails:delete",
                "guardrail",
                Some(name),
                None,
                None,
            )
            .await
            {
                return error_response(&headers, error);
            }
            StatusCode::NO_CONTENT.into_response()
        }
        Err(error) => error_response(&headers, error),
    }
}

async fn create_key(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<AdminKeyCreate>,
) -> Response {
    let actor = match require_admin_scope(&state, &headers, SCOPE_KEYS_CREATE).await {
        Ok(actor) => actor,
        Err(response) => return response,
    };

    let material = match VirtualKeyMaterial::generate() {
        Ok(material) => material,
        Err(error) => return error_response(&headers, error),
    };
    match state.store.create_admin_key(request, &material).await {
        Ok(key) => {
            if let Err(error) = record_admin_audit(
                &state,
                &headers,
                &actor,
                "keys:create",
                "key",
                Some(key.id.to_string()),
                None,
                audit_json(&key),
            )
            .await
            {
                return error_response(&headers, error);
            }
            Json(CreatedAdminKeyResponse {
                key,
                raw_key: material.raw_key,
            })
            .into_response()
        }
        Err(error) => error_response(&headers, error),
    }
}

#[derive(Debug, Deserialize)]
struct PolicySimulationRequest {
    key_id: Option<uuid::Uuid>,
    #[serde(default)]
    team_id: Option<String>,
    path: String,
    #[serde(default = "default_simulation_method")]
    method: String,
    provider: Option<String>,
    #[serde(default)]
    body: Option<Value>,
    #[serde(default)]
    request_body_bytes: Option<i64>,
    #[serde(default)]
    response_body_bytes: Option<i64>,
    #[serde(default)]
    estimated_cost_usd: Option<f64>,
    #[serde(default)]
    preset: Option<gateway_core::KeyPreset>,
    #[serde(default)]
    policy: Option<gateway_core::admin::KeyPolicyPatch>,
    #[serde(default)]
    guardrail_policy: Option<gateway_core::GuardrailPolicyPatch>,
}

#[derive(Debug, Serialize)]
struct PolicySimulationResponse {
    auth: PolicySimulationAuth,
    route_match: PolicySimulationRoute,
    policy_merge: PolicySimulationPolicy,
    guardrail_plan: Vec<String>,
    rate_limit_projection: PolicySimulationRateLimitProjection,
    budget_projection: PolicySimulationBudgetProjection,
    final_decision: PolicySimulationDecision,
}

#[derive(Debug, Serialize)]
struct PolicySimulationAuth {
    key_id: Option<uuid::Uuid>,
    source: &'static str,
}

#[derive(Debug, Serialize)]
struct PolicySimulationRoute {
    route: &'static str,
    provider: &'static str,
    service_name: Option<String>,
}

#[derive(Debug, Serialize)]
struct PolicySimulationPolicy {
    policy_version: i64,
    deny: bool,
    allowed_routes: Vec<&'static str>,
    allowed_models: Vec<String>,
    allowed_providers: Vec<&'static str>,
    allowed_services: Vec<String>,
}

#[derive(Debug, Serialize)]
struct PolicySimulationRateLimitProjection {
    rpm_limit: Option<i32>,
    tpm_limit: Option<i32>,
    max_requests_per_day: Option<i32>,
    max_tokens_per_day: Option<i32>,
}

#[derive(Debug, Serialize)]
struct PolicySimulationBudgetProjection {
    daily_budget_usd: Option<f64>,
    monthly_budget_usd: Option<f64>,
    max_cost_per_request: Option<f64>,
}

#[derive(Debug, Serialize)]
struct PolicySimulationDecision {
    allowed: bool,
    error_code: Option<&'static str>,
    message: &'static str,
}

fn default_simulation_method() -> String {
    "POST".to_owned()
}

async fn simulate_policy(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<PolicySimulationRequest>,
) -> Response {
    let actor = match require_admin_scope(&state, &headers, SCOPE_POLICIES_UPDATE).await {
        Ok(actor) => actor,
        Err(response) => return response,
    };

    let method = match Method::from_bytes(request.method.as_bytes()) {
        Ok(method) => method,
        Err(_) => return error_response(&headers, GatewayError::UnsupportedRoute),
    };
    let route_match = match Route::resolve_match(&method, &request.path) {
        Ok(route_match) => route_match,
        Err(error) => return error_response(&headers, error),
    };
    let provider = match request.provider.as_deref() {
        Some(value) => match parse_simulation_provider(value) {
            Ok(provider) => provider,
            Err(error) => return error_response(&headers, error),
        },
        None => route_match.provider,
    };
    let body_bytes = request
        .body
        .as_ref()
        .and_then(|value| serde_json::to_vec(value).ok())
        .unwrap_or_default();
    let mut features = extract_generation_features(&body_bytes);
    if features.service_name.is_none() {
        features.service_name = route_match.service_name.clone();
    }

    let effective = match request.key_id {
        Some(key_id) => match state
            .store
            .effective_policy_for_context(
                key_id,
                None,
                request.team_id.clone(),
                Some(route_match.route),
                features.model.clone(),
            )
            .await
        {
            Ok(effective) => effective,
            Err(error) => return error_response(&headers, error),
        },
        None => gateway_core::EffectivePolicy {
            policy: request
                .preset
                .map(|preset| preset.apply(KeyPolicy::default()))
                .unwrap_or_default(),
            guardrail_policy: Default::default(),
            applied_layers: Vec::new(),
        },
    };
    let policy = effective.policy;
    let policy = match request.policy {
        Some(policy_patch) => match apply_simulation_policy_patch(policy, policy_patch) {
            Ok(policy) => policy,
            Err(error) => return error_response(&headers, error),
        },
        None => policy,
    };
    let guardrail_policy = effective.guardrail_policy;
    let guardrail_policy = match request.guardrail_policy {
        Some(patch) => match patch.apply(guardrail_policy) {
            Ok(policy) => policy,
            Err(error) => return error_response(&headers, error),
        },
        None => guardrail_policy,
    };
    let definitions = match state.store.list_guardrail_definitions().await {
        Ok(definitions) => definitions,
        Err(error) => return error_response(&headers, error),
    };
    let guardrail_plan = resolve_guardrail_plan(GuardrailPlanRequest {
        mode: GuardrailMode::PreCall,
        definitions,
        policies: GuardrailPolicySet {
            key_policy: guardrail_policy,
            ..GuardrailPolicySet::default()
        },
        client_requested_guardrails: Vec::new(),
    });

    let decision_error = evaluate_policy(&policy, route_match.route, provider, &features)
        .and_then(|_| {
            evaluate_policy_limits(
                &policy,
                Utc::now(),
                request
                    .request_body_bytes
                    .or_else(|| i64::try_from(body_bytes.len()).ok()),
                request.response_body_bytes,
                None,
                None,
                request.estimated_cost_usd,
            )
        })
        .err()
        .or_else(|| guardrail_plan.as_ref().err().cloned());
    let final_decision = match decision_error {
        Some(error) => PolicySimulationDecision {
            allowed: false,
            error_code: Some(error.code()),
            message: error.public_message(),
        },
        None => PolicySimulationDecision {
            allowed: true,
            error_code: None,
            message: "Request would be allowed by configured policy.",
        },
    };
    let guardrail_plan = guardrail_plan
        .map(|plan| {
            plan.entries
                .into_iter()
                .map(|entry| entry.definition.name)
                .collect()
        })
        .unwrap_or_default();

    let response = PolicySimulationResponse {
        auth: PolicySimulationAuth {
            key_id: request.key_id,
            source: if request.key_id.is_some() {
                "stored_key"
            } else {
                "default_policy"
            },
        },
        route_match: PolicySimulationRoute {
            route: route_match.route.as_str(),
            provider: provider.as_str(),
            service_name: features.service_name,
        },
        policy_merge: PolicySimulationPolicy {
            policy_version: policy.policy_version,
            deny: policy.deny,
            allowed_routes: policy
                .allowed_routes
                .iter()
                .map(|route| route.as_str())
                .collect(),
            allowed_models: policy.allowed_models.clone(),
            allowed_providers: policy
                .allowed_providers
                .iter()
                .map(|provider| provider.as_str())
                .collect(),
            allowed_services: policy.allowed_services.clone(),
        },
        rate_limit_projection: PolicySimulationRateLimitProjection {
            rpm_limit: policy.rpm_limit,
            tpm_limit: policy.tpm_limit,
            max_requests_per_day: policy.max_requests_per_day,
            max_tokens_per_day: policy.max_tokens_per_day,
        },
        budget_projection: PolicySimulationBudgetProjection {
            daily_budget_usd: policy.daily_budget_usd,
            monthly_budget_usd: policy.monthly_budget_usd,
            max_cost_per_request: policy.max_cost_per_request,
        },
        guardrail_plan,
        final_decision,
    };

    if let Err(error) = record_admin_audit(
        &state,
        &headers,
        &actor,
        "policies:simulate",
        "policy",
        request.key_id.map(|id| id.to_string()),
        None,
        audit_json(&response),
    )
    .await
    {
        return error_response(&headers, error);
    }
    Json(response).into_response()
}

async fn list_policy_layers(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(response) = require_admin_scope(&state, &headers, SCOPE_POLICIES_UPDATE).await {
        return response;
    }
    match state.store.list_policy_layers().await {
        Ok(layers) => Json(layers).into_response(),
        Err(error) => error_response(&headers, error),
    }
}

async fn upsert_policy_layer(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<AdminPolicyLayerUpsert>,
) -> Response {
    let actor = match require_admin_scope(&state, &headers, SCOPE_POLICIES_UPDATE).await {
        Ok(actor) => actor,
        Err(response) => return response,
    };
    match state.store.upsert_policy_layer(request).await {
        Ok(layer) => {
            if let Err(error) = record_admin_audit(
                &state,
                &headers,
                &actor,
                "policies:upsert-layer",
                "policy_layer",
                Some(layer.id.to_string()),
                None,
                audit_json(&layer),
            )
            .await
            {
                return error_response(&headers, error);
            }
            Json(layer).into_response()
        }
        Err(error) => error_response(&headers, error),
    }
}

async fn delete_policy_layer(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(layer_id): Path<uuid::Uuid>,
) -> Response {
    let actor = match require_admin_scope(&state, &headers, SCOPE_POLICIES_UPDATE).await {
        Ok(actor) => actor,
        Err(response) => return response,
    };
    match state.store.delete_policy_layer(layer_id).await {
        Ok(true) => {
            if let Err(error) = record_admin_audit(
                &state,
                &headers,
                &actor,
                "policies:delete-layer",
                "policy_layer",
                Some(layer_id.to_string()),
                None,
                None,
            )
            .await
            {
                return error_response(&headers, error);
            }
            StatusCode::NO_CONTENT.into_response()
        }
        Ok(false) => StatusCode::NOT_FOUND.into_response(),
        Err(error) => error_response(&headers, error),
    }
}

fn parse_simulation_provider(value: &str) -> GatewayResult<Provider> {
    match value {
        "litellm" => Ok(Provider::LiteLlm),
        "openai-compatible" => Ok(Provider::OpenAiCompatible),
        "internal-service" => Ok(Provider::InternalService),
        _ => Err(GatewayError::PolicyDenied),
    }
}

fn apply_simulation_policy_patch(
    mut policy: KeyPolicy,
    patch: gateway_core::admin::KeyPolicyPatch,
) -> GatewayResult<KeyPolicy> {
    if let Some(deny) = patch.deny {
        policy.deny = deny;
    }
    if let Some(routes) = patch.allowed_routes {
        policy.allowed_routes = routes
            .iter()
            .map(|route| match route.as_str() {
                "/v1/chat/completions" => Ok(Route::ChatCompletions),
                "/v1/responses" => Ok(Route::Responses),
                "/providers/openai/*" => Ok(Route::DirectOpenAi),
                "/summary" => Ok(Route::Summary),
                "/translation" => Ok(Route::Translation),
                "/ocr" => Ok(Route::Ocr),
                "/embeddings" => Ok(Route::Embeddings),
                "/services/*" => Ok(Route::ServiceWildcard),
                _ => Err(GatewayError::PolicyDenied),
            })
            .collect::<GatewayResult<Vec<_>>>()?;
    }
    if let Some(models) = patch.allowed_models {
        policy.allowed_models = models;
    }
    if let Some(providers) = patch.allowed_providers {
        policy.allowed_providers = providers
            .iter()
            .map(|provider| parse_simulation_provider(provider))
            .collect::<GatewayResult<Vec<_>>>()?;
    }
    if let Some(services) = patch.allowed_services {
        policy.allowed_services = services;
    }
    if let Some(value) = patch.rpm_limit {
        policy.rpm_limit = value;
    }
    if let Some(value) = patch.tpm_limit {
        policy.tpm_limit = value;
    }
    if let Some(value) = patch.daily_budget_usd {
        policy.daily_budget_usd = value;
    }
    if let Some(value) = patch.monthly_budget_usd {
        policy.monthly_budget_usd = value;
    }
    if let Some(value) = patch.allow_streaming {
        policy.allow_streaming = value;
    }
    if let Some(value) = patch.allow_tools {
        policy.allow_tools = value;
    }
    if let Some(value) = patch.max_requests_per_day {
        policy.max_requests_per_day = value;
    }
    if let Some(value) = patch.max_tokens_per_day {
        policy.max_tokens_per_day = value;
    }
    if let Some(value) = patch.max_cost_per_request {
        policy.max_cost_per_request = value;
    }
    if let Some(value) = patch.max_input_tokens_per_request {
        policy.max_input_tokens_per_request = value;
    }
    if let Some(value) = patch.max_output_tokens_per_request {
        policy.max_output_tokens_per_request = value;
    }
    if let Some(hours) = patch.allowed_hours_utc {
        if hours.iter().any(|hour| !(0..=23).contains(hour)) {
            return Err(GatewayError::PolicyDenied);
        }
        policy.allowed_hours_utc = hours;
    }
    if let Some(value) = patch.unused_key_auto_disable_after_days {
        policy.unused_key_auto_disable_after_days = value;
    }
    if let Some(value) = patch.max_request_body_bytes {
        policy.max_request_body_bytes = value;
    }
    if let Some(value) = patch.max_response_body_bytes {
        policy.max_response_body_bytes = value;
    }
    if let Some(value) = patch.max_stream_duration_seconds {
        policy.max_stream_duration_seconds = value;
    }
    if let Some(value) = patch.max_sse_event_bytes {
        policy.max_sse_event_bytes = value;
    }
    if let Some(value) = patch.max_tool_call_count {
        policy.max_tool_call_count = value;
    }
    if let Some(value) = patch.max_tool_schema_bytes {
        policy.max_tool_schema_bytes = value;
    }
    Ok(policy)
}

async fn list_keys(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Some(response) = require_admin(&state, &headers).await {
        return response;
    }

    match state.store.list_admin_keys().await {
        Ok(keys) => Json(keys).into_response(),
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
    let required_scopes = key_patch_required_scopes(&patch);
    let actor = match require_admin_scopes(&state, &headers, &required_scopes).await {
        Ok(actor) => actor,
        Err(response) => return response,
    };
    let before = match state.store.get_admin_key(key_id).await {
        Ok(before) => before,
        Err(error) => return error_response(&headers, error),
    };

    match state.store.patch_admin_key(key_id, patch).await {
        Ok(Some(key)) => {
            if let Err(error) = record_admin_audit(
                &state,
                &headers,
                &actor,
                "keys:update",
                "key",
                Some(key.id.to_string()),
                before.as_ref().and_then(audit_json),
                audit_json(&key),
            )
            .await
            {
                return error_response(&headers, error);
            }
            Json(key).into_response()
        }
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(error) => error_response(&headers, error),
    }
}

fn key_patch_required_scopes(patch: &AdminKeyPatch) -> Vec<&'static str> {
    let mut scopes = Vec::new();
    if patch.disabled.is_some() {
        scopes.push(SCOPE_KEYS_DISABLE);
    }
    if patch.owner_type.is_some()
        || patch.project_id.is_some()
        || patch.service_names.is_some()
        || patch.expires_at.is_some()
        || patch.rotation_due_at.is_some()
        || patch.policy.is_some()
        || patch.guardrail_policy.is_some()
    {
        scopes.push(SCOPE_POLICIES_UPDATE);
    }
    if scopes.is_empty() {
        scopes.push(SCOPE_POLICIES_UPDATE);
    }
    scopes
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

async fn enable_key(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(key_id): Path<uuid::Uuid>,
) -> Response {
    mutate_key_lifecycle(state, headers, key_id, KeyLifecycleAction::Enable).await
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

async fn create_project(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<ProjectCreateRequest>,
) -> Response {
    admin_mutation(
        headers,
        &state,
        SCOPE_SETTINGS_UPDATE,
        "projects:create",
        "project",
        |project: &ProjectResponse| Some(project.id.to_string()),
        |store| async move { store.create_project(request).await },
    )
    .await
}

async fn list_projects(State(state): State<AppState>, headers: HeaderMap) -> Response {
    admin_query(headers, &state, SCOPE_USAGE_READ, |store| async move {
        store.list_projects().await
    })
    .await
}

async fn get_project(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(project_id): Path<uuid::Uuid>,
) -> Response {
    if let Some(response) = require_admin(&state, &headers).await {
        return response;
    }

    match state.store.get_project(project_id).await {
        Ok(Some(project)) => Json(project).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(error) => error_response(&headers, error),
    }
}

async fn patch_project(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(project_id): Path<uuid::Uuid>,
    Json(patch): Json<ProjectPatchRequest>,
) -> Response {
    let actor = match require_admin_scope(&state, &headers, SCOPE_SETTINGS_UPDATE).await {
        Ok(actor) => actor,
        Err(response) => return response,
    };
    let before = match state.store.get_project(project_id).await {
        Ok(before) => before,
        Err(error) => return error_response(&headers, error),
    };

    match state.store.patch_project(project_id, patch).await {
        Ok(Some(project)) => {
            if let Err(error) = record_admin_audit(
                &state,
                &headers,
                &actor,
                "projects:update",
                "project",
                Some(project.id.to_string()),
                before.as_ref().and_then(audit_json),
                audit_json(&project),
            )
            .await
            {
                return error_response(&headers, error);
            }
            Json(project).into_response()
        }
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(error) => error_response(&headers, error),
    }
}

async fn delete_project(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(project_id): Path<uuid::Uuid>,
) -> Response {
    let actor = match require_admin_scope(&state, &headers, SCOPE_SETTINGS_UPDATE).await {
        Ok(actor) => actor,
        Err(response) => return response,
    };
    let before = match state.store.get_project(project_id).await {
        Ok(before) => before,
        Err(error) => return error_response(&headers, error),
    };

    match state.store.delete_project(project_id).await {
        Ok(true) => {
            if let Err(error) = record_admin_audit(
                &state,
                &headers,
                &actor,
                "projects:delete",
                "project",
                Some(project_id.to_string()),
                before.as_ref().and_then(audit_json),
                None,
            )
            .await
            {
                return error_response(&headers, error);
            }
            StatusCode::NO_CONTENT.into_response()
        }
        Ok(false) => StatusCode::NOT_FOUND.into_response(),
        Err(error) => error_response(&headers, error),
    }
}

async fn rotate_operator_token(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let current_raw_token = match bearer_token(&headers) {
        Ok(token) => token.to_owned(),
        Err(error) => return error_response(&headers, error),
    };
    let actor = match require_admin_scope(&state, &headers, SCOPE_OPERATORS_MANAGE).await {
        Ok(actor) => actor,
        Err(response) => return response,
    };

    let material = match OperatorTokenMaterial::generate() {
        Ok(material) => material,
        Err(error) => return error_response(&headers, error),
    };
    match state
        .store
        .rotate_operator_token(&current_raw_token, &material, Utc::now())
        .await
    {
        Ok(token) => {
            if let Err(error) = record_admin_audit(
                &state,
                &headers,
                &actor,
                "operators:rotate",
                "operator_token",
                Some(token.id.to_string()),
                None,
                audit_json(&token),
            )
            .await
            {
                return error_response(&headers, error);
            }
            Json(CreatedOperatorTokenResponse {
                token,
                raw_token: material.raw_token,
            })
            .into_response()
        }
        Err(error) => error_response(&headers, error),
    }
}

async fn create_provider(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<ProviderConfigCreateRequest>,
) -> Response {
    admin_mutation(
        headers,
        &state,
        SCOPE_PROVIDERS_UPDATE,
        "providers:create",
        "provider",
        |provider: &ProviderConfigResponse| Some(provider.id.to_string()),
        |store| async move { store.create_provider_config(request).await },
    )
    .await
}

async fn list_providers(State(state): State<AppState>, headers: HeaderMap) -> Response {
    admin_query(headers, &state, SCOPE_USAGE_READ, |store| async move {
        store.list_provider_configs().await
    })
    .await
}

async fn get_provider(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(provider_id): Path<uuid::Uuid>,
) -> Response {
    if let Some(response) = require_admin(&state, &headers).await {
        return response;
    }

    match state.store.get_provider_config(provider_id).await {
        Ok(Some(provider)) => Json(provider).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(error) => error_response(&headers, error),
    }
}

async fn patch_provider(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(provider_id): Path<uuid::Uuid>,
    Json(patch): Json<ProviderConfigPatchRequest>,
) -> Response {
    let actor = match require_admin_scope(&state, &headers, SCOPE_PROVIDERS_UPDATE).await {
        Ok(actor) => actor,
        Err(response) => return response,
    };
    let before = match state.store.get_provider_config(provider_id).await {
        Ok(before) => before,
        Err(error) => return error_response(&headers, error),
    };

    match state.store.patch_provider_config(provider_id, patch).await {
        Ok(Some(provider)) => {
            if let Err(error) = record_admin_audit(
                &state,
                &headers,
                &actor,
                "providers:update",
                "provider",
                Some(provider.id.to_string()),
                before.as_ref().and_then(audit_json),
                audit_json(&provider),
            )
            .await
            {
                return error_response(&headers, error);
            }
            Json(provider).into_response()
        }
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(error) => error_response(&headers, error),
    }
}

async fn delete_provider(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(provider_id): Path<uuid::Uuid>,
) -> Response {
    let actor = match require_admin_scope(&state, &headers, SCOPE_PROVIDERS_UPDATE).await {
        Ok(actor) => actor,
        Err(response) => return response,
    };
    let before = match state.store.get_provider_config(provider_id).await {
        Ok(before) => before,
        Err(error) => return error_response(&headers, error),
    };

    match state.store.delete_provider_config(provider_id).await {
        Ok(true) => {
            if let Err(error) = record_admin_audit(
                &state,
                &headers,
                &actor,
                "providers:delete",
                "provider",
                Some(provider_id.to_string()),
                before.as_ref().and_then(audit_json),
                None,
            )
            .await
            {
                return error_response(&headers, error);
            }
            StatusCode::NO_CONTENT.into_response()
        }
        Ok(false) => StatusCode::NOT_FOUND.into_response(),
        Err(error) => error_response(&headers, error),
    }
}

async fn disable_provider(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(provider_id): Path<uuid::Uuid>,
) -> Response {
    mutate_provider_enabled(state, headers, provider_id, false).await
}

async fn enable_provider(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(provider_id): Path<uuid::Uuid>,
) -> Response {
    mutate_provider_enabled(state, headers, provider_id, true).await
}

async fn list_openai_routes(State(state): State<AppState>, headers: HeaderMap) -> Response {
    admin_query(headers, &state, SCOPE_USAGE_READ, |store| async move {
        store.list_openai_route_settings().await
    })
    .await
}

async fn disable_openai_route(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(route_id): Path<String>,
) -> Response {
    mutate_openai_route_enabled(state, headers, route_id, false).await
}

async fn enable_openai_route(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(route_id): Path<String>,
) -> Response {
    mutate_openai_route_enabled(state, headers, route_id, true).await
}

async fn create_service(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<ServiceCreateRequest>,
) -> Response {
    admin_mutation(
        headers,
        &state,
        SCOPE_SERVICES_UPDATE,
        "services:create",
        "service",
        |service: &ServiceResponse| Some(service.name.clone()),
        |store| async move { store.create_service(request).await },
    )
    .await
}

async fn list_services(State(state): State<AppState>, headers: HeaderMap) -> Response {
    admin_query(headers, &state, SCOPE_USAGE_READ, |store| async move {
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
    let actor = match require_admin_scope(&state, &headers, SCOPE_SERVICES_UPDATE).await {
        Ok(actor) => actor,
        Err(response) => return response,
    };
    let before = match state.store.get_service(&service_name).await {
        Ok(before) => before,
        Err(error) => return error_response(&headers, error),
    };

    match state.store.patch_service(&service_name, patch).await {
        Ok(Some(service)) => {
            if let Err(error) = record_admin_audit(
                &state,
                &headers,
                &actor,
                "services:update",
                "service",
                Some(service.name.clone()),
                before.as_ref().and_then(audit_json),
                audit_json(&service),
            )
            .await
            {
                return error_response(&headers, error);
            }
            Json(service).into_response()
        }
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(error) => error_response(&headers, error),
    }
}

async fn delete_service(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(service_name): Path<String>,
) -> Response {
    let actor = match require_admin_scope(&state, &headers, SCOPE_SERVICES_UPDATE).await {
        Ok(actor) => actor,
        Err(response) => return response,
    };
    let before = match state.store.get_service(&service_name).await {
        Ok(before) => before,
        Err(error) => return error_response(&headers, error),
    };

    match state.store.delete_service(&service_name).await {
        Ok(true) => {
            if let Err(error) = record_admin_audit(
                &state,
                &headers,
                &actor,
                "services:delete",
                "service",
                Some(service_name),
                before.as_ref().and_then(audit_json),
                None,
            )
            .await
            {
                return error_response(&headers, error);
            }
            StatusCode::NO_CONTENT.into_response()
        }
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
    admin_mutation(
        headers,
        &state,
        SCOPE_SERVICES_UPDATE,
        "services:import",
        "service",
        |service: &ServiceResponse| Some(service.name.clone()),
        |store| async move { store.import_studio_service(request).await },
    )
    .await
}

async fn get_studio_connection(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Some(response) = require_admin(&state, &headers).await {
        return response;
    }

    match effective_studio_connection(&state).await {
        Ok(connection) => Json(connection.response()).into_response(),
        Err(error) => error_response(&headers, error),
    }
}

async fn patch_studio_connection(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(patch): Json<StudioConnectionPatchRequest>,
) -> Response {
    let actor = match require_admin_scope(&state, &headers, SCOPE_SETTINGS_UPDATE).await {
        Ok(actor) => actor,
        Err(response) => return response,
    };
    let before = match effective_studio_connection(&state).await {
        Ok(connection) => Some(connection.response()),
        Err(GatewayError::InvalidConfiguration) => None,
        Err(error) => return error_response(&headers, error),
    };

    match state.store.patch_studio_connection_settings(patch).await {
        Ok(_) => match effective_studio_connection(&state).await {
            Ok(connection) => {
                let response = connection.response();
                if let Err(error) = record_admin_audit(
                    &state,
                    &headers,
                    &actor,
                    "settings:studio_connection_update",
                    "studio_connection",
                    Some("singleton".to_owned()),
                    before.as_ref().and_then(audit_json),
                    audit_json(&response),
                )
                .await
                {
                    return error_response(&headers, error);
                }
                Json(response).into_response()
            }
            Err(error) => error_response(&headers, error),
        },
        Err(error) => error_response(&headers, error),
    }
}

async fn test_studio_connection(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Some(response) = require_admin(&state, &headers).await {
        return response;
    }

    match effective_studio_client(&state).await {
        Ok(studio) => match studio.services().await {
            Ok(services) => Json(StudioConnectionTestResponse {
                ok: true,
                service_count: services.len(),
            })
            .into_response(),
            Err(error) => error_response(&headers, error),
        },
        Err(error) => error_response(&headers, error),
    }
}

async fn studio_services(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Some(response) = require_admin(&state, &headers).await {
        return response;
    }

    let studio = match effective_studio_client(&state).await {
        Ok(studio) => studio,
        Err(error) => return error_response(&headers, error),
    };
    match studio.services().await {
        Ok(services) => Json(services).into_response(),
        Err(error) => error_response(&headers, error),
    }
}

async fn sync_service(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<StudioServiceImportRequest>,
) -> Response {
    admin_mutation(
        headers,
        &state,
        SCOPE_SERVICES_UPDATE,
        "services:sync",
        "service",
        |service: &ServiceResponse| Some(service.name.clone()),
        |store| async move { store.sync_studio_service(request).await },
    )
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
    admin_query(headers, &state, SCOPE_USAGE_READ, |store| async move {
        store.usage_summary(query).await
    })
    .await
}

async fn usage_timeseries(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<UsageQuery>,
) -> Response {
    admin_query(headers, &state, SCOPE_USAGE_READ, |store| async move {
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

async fn usage_export_json(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<UsageQuery>,
) -> Response {
    admin_query(headers, &state, SCOPE_USAGE_EXPORT, |store| async move {
        store.usage_export(query).await
    })
    .await
}

async fn usage_export_csv(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<UsageQuery>,
) -> Response {
    if let Err(response) = require_admin_scope(&state, &headers, SCOPE_USAGE_EXPORT).await {
        return response;
    }
    match state.store.usage_export(query).await {
        Ok(export) => (
            StatusCode::OK,
            [("content-type", "text/csv; charset=utf-8")],
            usage_export_csv_body(&export),
        )
            .into_response(),
        Err(error) => error_response(&headers, error),
    }
}

async fn task_usage(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(task_id): Path<String>,
    Query(mut query): Query<UsageQuery>,
) -> Response {
    query.task_id = Some(task_id);
    admin_query(headers, &state, SCOPE_USAGE_READ, |store| async move {
        store.usage_summary(query).await
    })
    .await
}

fn usage_export_csv_body(export: &UsageExport) -> String {
    let mut csv = "request_id,key_id,project_id,route,model,provider,status,status_code,latency_ms,input_tokens,output_tokens,total_tokens,estimated_cost_usd,service_name,task_id,run_id,fallback_count,guardrail_action_count,created_at\n".to_owned();
    for row in &export.rows {
        let fields = [
            row.request_id.clone(),
            row.key_id.to_string(),
            row.project_id
                .map(|value| value.to_string())
                .unwrap_or_default(),
            row.route.clone(),
            row.model.clone().unwrap_or_default(),
            row.provider.clone(),
            row.status.clone(),
            row.status_code.to_string(),
            row.latency_ms.to_string(),
            row.input_tokens.to_string(),
            row.output_tokens.to_string(),
            row.total_tokens.to_string(),
            row.estimated_cost_usd
                .map(|value| value.to_string())
                .unwrap_or_default(),
            row.service_name.clone().unwrap_or_default(),
            row.task_id.clone().unwrap_or_default(),
            row.run_id.clone().unwrap_or_default(),
            row.fallback_count.to_string(),
            row.guardrail_action_count.to_string(),
            row.created_at.to_rfc3339(),
        ];
        csv.push_str(
            &fields
                .into_iter()
                .map(csv_escape)
                .collect::<Vec<_>>()
                .join(","),
        );
        csv.push('\n');
    }
    csv
}

fn csv_escape(mut value: String) -> String {
    if value
        .chars()
        .next()
        .is_some_and(|first| matches!(first, '=' | '+' | '-' | '@' | '\t'))
    {
        value.insert(0, '\'');
    }
    if value.contains(',') || value.contains('"') || value.contains('\n') || value.contains('\r') {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value
    }
}

async fn usage_breakdown(
    state: AppState,
    headers: HeaderMap,
    query: UsageQuery,
    dimension: UsageBreakdownDimension,
) -> Response {
    admin_query(headers, &state, SCOPE_USAGE_READ, |store| async move {
        store.usage_breakdown(query, dimension).await
    })
    .await
}

async fn provider_health(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<UsageQuery>,
) -> Response {
    admin_query(headers, &state, SCOPE_USAGE_READ, |store| async move {
        store.provider_health(query).await
    })
    .await
}

async fn provider_health_state(State(state): State<AppState>, headers: HeaderMap) -> Response {
    admin_query(headers, &state, SCOPE_USAGE_READ, |store| async move {
        store.list_provider_health_states().await
    })
    .await
}

async fn upsert_provider_health_state(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<ProviderHealthState>,
) -> Response {
    admin_query(
        headers,
        &state,
        SCOPE_PROVIDERS_UPDATE,
        |store| async move { store.upsert_provider_health_state(request).await },
    )
    .await
}

async fn run_provider_health_checks(State(state): State<AppState>, headers: HeaderMap) -> Response {
    admin_query(
        headers,
        &state,
        SCOPE_PROVIDERS_UPDATE,
        |store| async move {
            let targets = store.provider_health_check_targets().await?;
            let client = reqwest::Client::builder()
                .timeout(Duration::from_secs(3))
                .build()
                .map_err(|_| GatewayError::InvalidConfiguration)?;
            let mut results = Vec::new();

            for target in targets {
                let checked = active_health_check(
                    &client,
                    &target.name,
                    target.base_url.clone(),
                    target.credential.as_deref(),
                )
                .await;
                let state = provider_health_state_from_check(target.name, target.provider, checked);
                results.push(store.upsert_provider_health_state(state).await?);
            }

            Ok(ProviderHealthCheckResponse { results })
        },
    )
    .await
}

async fn get_debug_bundle(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(request_id): Path<String>,
) -> Response {
    if let Err(response) = require_admin_scope(&state, &headers, SCOPE_USAGE_READ).await {
        return response;
    }
    match state.store.get_debug_bundle(&request_id).await {
        Ok(Some(bundle)) => Json(bundle).into_response(),
        Ok(None) => error_response(&headers, GatewayError::UnsupportedRoute),
        Err(error) => error_response(&headers, error),
    }
}

async fn preview_service_import(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<ServiceImportBatchRequest>,
) -> Response {
    admin_query(headers, &state, SCOPE_SERVICES_UPDATE, |store| async move {
        let existing = store.list_services().await?;
        let diff = service_import_diff(&existing, &request.services);
        Ok(ServiceImportPreviewResponse { diff })
    })
    .await
}

async fn activate_service_import(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<ServiceImportBatchRequest>,
) -> Response {
    admin_query(headers, &state, SCOPE_SERVICES_UPDATE, |store| async move {
        let existing = store.list_services().await?;
        let diff = service_import_diff(&existing, &request.services);
        if !diff.invalid.is_empty() {
            return Err(GatewayError::InvalidServicePayload);
        }
        let (snapshot, services) = store
            .activate_service_registry_import(
                request.source.unwrap_or_else(|| "admin-api".to_owned()),
                diff,
                request.services,
                None,
            )
            .await?;
        Ok(ServiceImportActivationResponse { snapshot, services })
    })
    .await
}

async fn service_import_versions(State(state): State<AppState>, headers: HeaderMap) -> Response {
    admin_query(headers, &state, SCOPE_SERVICES_UPDATE, |store| async move {
        store.list_service_registry_snapshots().await
    })
    .await
}

async fn rollback_service_import(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(version): Path<i64>,
) -> Response {
    admin_query(headers, &state, SCOPE_SERVICES_UPDATE, |store| async move {
        let Some(snapshot) = store.service_registry_snapshot(version).await? else {
            return Err(GatewayError::MissingService);
        };
        let services: Vec<StudioServiceImportRequest> =
            serde_json::from_value(snapshot.services_json.clone())
                .map_err(|_| GatewayError::InvalidServicePayload)?;
        let (rollback_snapshot, activated) = store
            .activate_service_registry_import(
                "rollback".to_owned(),
                snapshot.diff.clone(),
                services,
                Some(version),
            )
            .await?;
        Ok(ServiceImportActivationResponse {
            snapshot: rollback_snapshot,
            services: activated,
        })
    })
    .await
}

async fn admin_query<T, Fut>(
    headers: HeaderMap,
    state: &AppState,
    required_scope: &'static str,
    query: impl FnOnce(Arc<dyn GatewayData>) -> Fut,
) -> Response
where
    T: Serialize,
    Fut: std::future::Future<Output = GatewayResult<T>>,
{
    if let Err(response) = require_admin_scope(state, &headers, required_scope).await {
        return response;
    }
    match query(state.store.clone()).await {
        Ok(value) => Json(value).into_response(),
        Err(error) => error_response(&headers, error),
    }
}

async fn admin_mutation<T, Fut>(
    headers: HeaderMap,
    state: &AppState,
    required_scope: &'static str,
    action: &'static str,
    target_type: &'static str,
    target_id: impl FnOnce(&T) -> Option<String>,
    mutation: impl FnOnce(Arc<dyn GatewayData>) -> Fut,
) -> Response
where
    T: Serialize,
    Fut: std::future::Future<Output = GatewayResult<T>>,
{
    let actor = match require_admin_scope(state, &headers, required_scope).await {
        Ok(actor) => actor,
        Err(response) => return response,
    };
    match mutation(state.store.clone()).await {
        Ok(value) => {
            if let Err(error) = record_admin_audit(
                state,
                &headers,
                &actor,
                action,
                target_type,
                target_id(&value),
                None,
                audit_json(&value),
            )
            .await
            {
                return error_response(&headers, error);
            }
            Json(value).into_response()
        }
        Err(error) => error_response(&headers, error),
    }
}

#[allow(clippy::too_many_arguments)]
async fn record_admin_audit(
    state: &AppState,
    headers: &HeaderMap,
    actor: &OperatorAuthorization,
    action: impl Into<String>,
    target_type: impl Into<String>,
    target_id: Option<String>,
    before: Option<serde_json::Value>,
    after: Option<serde_json::Value>,
) -> GatewayResult<AuditEvent> {
    state
        .store
        .record_audit_event(AuditEventCreate {
            actor_token_id: actor.token_id,
            action: action.into(),
            target_type: target_type.into(),
            target_id,
            before,
            after,
            request_id: request_id_from_headers(headers),
            ip: forwarded_for(headers),
            user_agent: headers
                .get("user-agent")
                .and_then(|value| value.to_str().ok())
                .map(ToOwned::to_owned),
        })
        .await
}

fn audit_json<T: Serialize>(value: &T) -> Option<serde_json::Value> {
    serde_json::to_value(value).ok()
}

fn forwarded_for(headers: &HeaderMap) -> Option<String> {
    headers
        .get("x-forwarded-for")
        .or_else(|| headers.get("x-real-ip"))
        .and_then(|value| value.to_str().ok())
        .map(|value| value.split(',').next().unwrap_or(value).trim().to_owned())
        .filter(|value| !value.is_empty())
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

#[derive(Clone, Copy)]
enum KeyLifecycleAction {
    Revoke,
    Disable,
    Enable,
}

async fn mutate_key_lifecycle(
    state: AppState,
    headers: HeaderMap,
    key_id: uuid::Uuid,
    action: KeyLifecycleAction,
) -> Response {
    let actor = match require_admin_scope(&state, &headers, SCOPE_KEYS_DISABLE).await {
        Ok(actor) => actor,
        Err(response) => return response,
    };
    let before = match state.store.get_admin_key(key_id).await {
        Ok(before) => before,
        Err(error) => return error_response(&headers, error),
    };

    let result: GatewayResult<Option<AdminKeyResponse>> = match action {
        KeyLifecycleAction::Revoke => state.store.revoke_admin_key(key_id).await,
        KeyLifecycleAction::Disable => state.store.disable_admin_key(key_id).await,
        KeyLifecycleAction::Enable => state.store.enable_admin_key(key_id).await,
    };

    match result {
        Ok(Some(key)) => {
            let action_name = match action {
                KeyLifecycleAction::Revoke => "keys:revoke",
                KeyLifecycleAction::Disable => "keys:disable",
                KeyLifecycleAction::Enable => "keys:enable",
            };
            if let Err(error) = record_admin_audit(
                &state,
                &headers,
                &actor,
                action_name,
                "key",
                Some(key.id.to_string()),
                before.as_ref().and_then(audit_json),
                audit_json(&key),
            )
            .await
            {
                return error_response(&headers, error);
            }
            Json(key).into_response()
        }
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
    let actor = match require_admin_scope(&state, &headers, SCOPE_SERVICES_UPDATE).await {
        Ok(actor) => actor,
        Err(response) => return response,
    };
    let before = match state.store.get_service(&service_name).await {
        Ok(before) => before,
        Err(error) => return error_response(&headers, error),
    };

    match state
        .store
        .set_service_enabled(&service_name, enabled)
        .await
    {
        Ok(Some(service)) => {
            if let Err(error) = record_admin_audit(
                &state,
                &headers,
                &actor,
                if enabled {
                    "services:enable"
                } else {
                    "services:disable"
                },
                "service",
                Some(service.name.clone()),
                before.as_ref().and_then(audit_json),
                audit_json(&service),
            )
            .await
            {
                return error_response(&headers, error);
            }
            Json(service).into_response()
        }
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(error) => error_response(&headers, error),
    }
}

async fn mutate_openai_route_enabled(
    state: AppState,
    headers: HeaderMap,
    route_id: String,
    enabled: bool,
) -> Response {
    let actor = match require_admin_scope(&state, &headers, SCOPE_POLICIES_UPDATE).await {
        Ok(actor) => actor,
        Err(response) => return response,
    };

    match state
        .store
        .set_openai_route_enabled(&route_id, enabled)
        .await
    {
        Ok(Some(setting)) => {
            if let Err(error) = record_admin_audit(
                &state,
                &headers,
                &actor,
                if enabled {
                    "policies:route_enable"
                } else {
                    "policies:route_disable"
                },
                "openai_route",
                Some(route_id),
                None,
                audit_json(&setting),
            )
            .await
            {
                return error_response(&headers, error);
            }
            Json(setting).into_response()
        }
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(error) => error_response(&headers, error),
    }
}

async fn mutate_provider_enabled(
    state: AppState,
    headers: HeaderMap,
    provider_id: uuid::Uuid,
    enabled: bool,
) -> Response {
    let actor = match require_admin_scope(&state, &headers, SCOPE_PROVIDERS_UPDATE).await {
        Ok(actor) => actor,
        Err(response) => return response,
    };
    let before = match state.store.get_provider_config(provider_id).await {
        Ok(before) => before,
        Err(error) => return error_response(&headers, error),
    };

    match state
        .store
        .set_provider_config_enabled(provider_id, enabled)
        .await
    {
        Ok(Some(provider)) => {
            if let Err(error) = record_admin_audit(
                &state,
                &headers,
                &actor,
                if enabled {
                    "providers:enable"
                } else {
                    "providers:disable"
                },
                "provider",
                Some(provider.id.to_string()),
                before.as_ref().and_then(audit_json),
                audit_json(&provider),
            )
            .await
            {
                return error_response(&headers, error);
            }
            Json(provider).into_response()
        }
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(error) => error_response(&headers, error),
    }
}

async fn effective_studio_connection(state: &AppState) -> GatewayResult<EffectiveStudioConnection> {
    let stored = state.store.studio_connection_settings().await?;
    Ok(EffectiveStudioConnection::from_sources(
        stored,
        &state.studio_env,
    ))
}

async fn effective_studio_client(state: &AppState) -> GatewayResult<StudioCatalogClient> {
    let connection = effective_studio_connection(state).await?;
    let base_url = connection
        .base_url
        .ok_or(GatewayError::InvalidConfiguration)?;
    Ok(StudioCatalogClient::new(base_url, connection.token))
}

async fn require_admin_scope(
    state: &AppState,
    headers: &HeaderMap,
    required_scope: &str,
) -> Result<OperatorAuthorization, Response> {
    require_admin_scopes(state, headers, &[required_scope]).await
}

async fn require_admin_scopes(
    state: &AppState,
    headers: &HeaderMap,
    required_scopes: &[&str],
) -> Result<OperatorAuthorization, Response> {
    let token = match bearer_token(headers) {
        Ok(token) => token,
        Err(error) => return Err(error_response(headers, error)),
    };

    match state.store.verify_operator_token(token, Utc::now()).await {
        Ok(authorization)
            if required_scopes
                .iter()
                .all(|required_scope| authorization.has_scope(required_scope)) =>
        {
            Ok(authorization)
        }
        Ok(_) => Err(error_response(
            headers,
            GatewayError::InsufficientOperatorScope,
        )),
        Err(error) => Err(error_response(headers, error)),
    }
}

async fn require_admin(state: &AppState, headers: &HeaderMap) -> Option<Response> {
    require_admin_scope(state, headers, SCOPE_OPERATORS_MANAGE)
        .await
        .err()
}

async fn require_virtual_key(
    state: &AppState,
    headers: &HeaderMap,
) -> GatewayResult<gateway_core::AuthenticatedKey> {
    let token = bearer_token(headers)?;
    Authenticator::new(state.store.clone())
        .authenticate_authorization(Some(&format!("Bearer {token}")), Utc::now())
        .await
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
    (
        error.status_code(),
        Json(error.body(request_id_from_headers(headers))),
    )
        .into_response()
}

fn request_id_from_headers(headers: &HeaderMap) -> String {
    headers
        .get("x-request-id")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("unknown")
        .to_owned()
}

#[derive(Debug, Serialize)]
struct StatusBody {
    status: &'static str,
}

#[derive(Debug, Serialize)]
struct GuardrailListResponse {
    guardrails: Vec<GuardrailDefinitionResponse>,
}

#[derive(Debug, Serialize)]
struct AdminGuardrailListResponse {
    guardrails: Vec<AdminGuardrailDefinitionResponse>,
}

#[derive(Debug, Serialize)]
struct AdminGuardrailExecutionListResponse {
    executions: Vec<GuardrailExecutionEvent>,
}

#[derive(Debug, Serialize)]
struct AdminGuardrailSummaryResponse {
    summary: Vec<GuardrailExecutionSummary>,
}

#[derive(Debug, Deserialize)]
struct ServiceImportBatchRequest {
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    services: Vec<StudioServiceImportRequest>,
}

#[derive(Debug, Serialize)]
struct ServiceImportPreviewResponse {
    diff: ServiceImportDiff,
}

#[derive(Debug, Serialize)]
struct ServiceImportActivationResponse {
    snapshot: ServiceRegistrySnapshot,
    services: Vec<ServiceResponse>,
}

#[derive(Debug, Serialize)]
struct ProviderHealthCheckResponse {
    results: Vec<ProviderHealthState>,
}

struct ActiveHealthCheck {
    ok: bool,
    latency_ms: Option<i64>,
    error_code: Option<String>,
    checked_at: chrono::DateTime<Utc>,
}

async fn active_health_check(
    client: &reqwest::Client,
    name: &str,
    base_url: Option<String>,
    credential: Option<&str>,
) -> ActiveHealthCheck {
    let checked_at = Utc::now();
    let Some(base_url) = base_url else {
        return ActiveHealthCheck {
            ok: false,
            latency_ms: None,
            error_code: Some("missing_upstream_url".to_owned()),
            checked_at,
        };
    };
    let Ok(url) = reqwest::Url::parse(&base_url) else {
        return ActiveHealthCheck {
            ok: false,
            latency_ms: None,
            error_code: Some("invalid_upstream_url".to_owned()),
            checked_at,
        };
    };
    let started = std::time::Instant::now();
    let mut request = client.get(url);
    if let Some(credential) = credential.filter(|value| !value.trim().is_empty()) {
        request = request.bearer_auth(credential);
    }
    match request.send().await {
        Ok(response) if response.status().is_success() => ActiveHealthCheck {
            ok: true,
            latency_ms: Some(i64::try_from(started.elapsed().as_millis()).unwrap_or(i64::MAX)),
            error_code: None,
            checked_at,
        },
        Ok(response) => ActiveHealthCheck {
            ok: false,
            latency_ms: Some(i64::try_from(started.elapsed().as_millis()).unwrap_or(i64::MAX)),
            error_code: Some(format!("http_{}", response.status().as_u16())),
            checked_at,
        },
        Err(error) => ActiveHealthCheck {
            ok: false,
            latency_ms: Some(i64::try_from(started.elapsed().as_millis()).unwrap_or(i64::MAX)),
            error_code: Some(if error.is_timeout() {
                "timeout".to_owned()
            } else {
                format!("health_check_failed:{name}")
            }),
            checked_at,
        },
    }
}

fn provider_health_state_from_check(
    name: String,
    provider: Provider,
    checked: ActiveHealthCheck,
) -> ProviderHealthState {
    ProviderHealthState {
        name,
        provider,
        status: if checked.ok {
            ProviderHealthStatus::Healthy
        } else {
            ProviderHealthStatus::Unhealthy
        },
        circuit_state: if checked.ok {
            CircuitBreakerState::Closed
        } else {
            CircuitBreakerState::Open
        },
        active_check_ok: Some(checked.ok),
        passive_success_count: i64::from(checked.ok),
        passive_failure_count: i64::from(!checked.ok),
        consecutive_failures: i32::from(!checked.ok),
        average_latency_ms: checked.latency_ms,
        last_error_code: checked.error_code,
        cooldown_until: None,
        checked_at: Some(checked.checked_at),
        updated_at: Utc::now(),
    }
}

fn service_import_diff(
    existing: &[ServiceResponse],
    requested: &[StudioServiceImportRequest],
) -> ServiceImportDiff {
    let requested_names: std::collections::BTreeSet<_> = requested
        .iter()
        .map(|service| service.name.clone())
        .collect();
    let existing_names: std::collections::BTreeSet<_> = existing
        .iter()
        .map(|service| service.name.clone())
        .collect();
    let added = requested_names
        .difference(&existing_names)
        .cloned()
        .collect::<Vec<_>>();
    let removed = existing
        .iter()
        .filter(|service| service.source == gateway_core::ServiceSource::Studio)
        .filter(|service| !requested_names.contains(&service.name))
        .map(|service| service.name.clone())
        .collect::<Vec<_>>();
    let changed = requested
        .iter()
        .filter_map(|request| {
            existing
                .iter()
                .find(|service| {
                    service.name == request.name
                        || service.studio_service_id.as_deref()
                            == Some(request.studio_service_id.as_str())
                })
                .filter(|service| {
                    request
                        .route_pattern
                        .as_ref()
                        .is_some_and(|route_pattern| route_pattern != &service.route_pattern)
                        || request.upstream_base_url != service.upstream_base_url
                        || request.allowed_methods != service.allowed_methods
                })
                .map(|_| request.name.clone())
        })
        .collect::<Vec<_>>();
    let invalid = requested
        .iter()
        .flat_map(service_import_validation_issues)
        .collect();

    ServiceImportDiff {
        added,
        changed,
        removed,
        invalid,
    }
}

fn service_import_validation_issues(
    request: &StudioServiceImportRequest,
) -> Vec<ServiceImportValidationIssue> {
    let mut issues = Vec::new();
    if request.validate().is_err() {
        issues.push(ServiceImportValidationIssue {
            service_name: request.name.clone(),
            field: "request".to_owned(),
            message: "service import payload is invalid".to_owned(),
        });
    }
    if let Some(base_url) = request.upstream_base_url.as_deref() {
        let valid_url = reqwest::Url::parse(base_url).ok().is_some_and(|url| {
            matches!(url.scheme(), "http" | "https") && url.host_str().is_some()
        });
        if !valid_url {
            issues.push(ServiceImportValidationIssue {
                service_name: request.name.clone(),
                field: "upstream_base_url".to_owned(),
                message: "upstream URL must be absolute http or https".to_owned(),
            });
        }
    }
    issues
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use gateway_core::{
        admin::{AdminKeyUsageSummary, AdminPolicyResponse, ProjectUsageSummary},
        auth::StoredVirtualKey,
        default_operator_roles, default_operator_scopes, OpenAiRouteSetting, OperatorTokenResponse,
        PatchValue, ProjectCreateRequest, ProjectPatchRequest, ProjectResponse,
        ProviderConfigCreateRequest, ProviderConfigPatchRequest, ProviderConfigResponse,
        ProviderHealth, Route, ServiceCostMode, ServiceResponse, ServiceSource, ServiceSyncStatus,
        ServiceSyncStatusResponse, StoredStudioConnection, StudioConnectionPatchRequest,
        UsageBreakdown, UsageExportRow, UsageStatus, UsageSummary, UsageTimeseriesPoint,
    };
    use std::sync::Mutex;
    use tower::ServiceExt;
    use uuid::Uuid;

    #[derive(Clone)]
    struct MemoryStore {
        key: Arc<Mutex<Option<StoredVirtualKey>>>,
        admin_key: Arc<Mutex<Option<AdminKeyResponse>>>,
        services: Arc<Mutex<Vec<ServiceResponse>>>,
        openai_routes: Arc<Mutex<Vec<OpenAiRouteSetting>>>,
        operator_tokens: Arc<Mutex<Vec<String>>>,
        events: Arc<Mutex<Vec<UsageEvent>>>,
        audit_events: Arc<Mutex<Vec<AuditEvent>>>,
        studio_connection: Arc<Mutex<Option<StoredStudioConnection>>>,
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
    impl AdminAuditStore for MemoryStore {
        async fn record_audit_event(&self, event: AuditEventCreate) -> GatewayResult<AuditEvent> {
            let audit_event = AuditEvent {
                id: Uuid::new_v4(),
                actor_token_id: event.actor_token_id,
                action: event.action,
                target_type: event.target_type,
                target_id: event.target_id,
                before: event.before,
                after: event.after,
                request_id: event.request_id,
                ip: event.ip,
                user_agent: event.user_agent,
                created_at: Utc::now(),
            };
            self.audit_events
                .lock()
                .expect("lock poisoned")
                .push(audit_event.clone());
            Ok(audit_event)
        }

        async fn list_audit_events(
            &self,
            query: AuditEventQuery,
        ) -> GatewayResult<Vec<AuditEvent>> {
            let mut events = self.audit_events.lock().expect("lock poisoned").clone();
            events.retain(|event| {
                query
                    .actor_token_id
                    .is_none_or(|actor_token_id| event.actor_token_id == actor_token_id)
                    && query
                        .action
                        .as_ref()
                        .is_none_or(|action| event.action == *action)
                    && query
                        .target_type
                        .as_ref()
                        .is_none_or(|target_type| event.target_type == *target_type)
                    && query
                        .target_id
                        .as_ref()
                        .is_none_or(|target_id| event.target_id.as_ref() == Some(target_id))
            });
            events.sort_by_key(|event| std::cmp::Reverse(event.created_at));
            events.truncate(query.limit.clamp(1, 500) as usize);
            Ok(events)
        }
    }

    #[async_trait]
    impl AdminStudioConnectionStore for MemoryStore {
        async fn studio_connection_settings(
            &self,
        ) -> GatewayResult<Option<StoredStudioConnection>> {
            Ok(self
                .studio_connection
                .lock()
                .expect("lock poisoned")
                .clone())
        }

        async fn patch_studio_connection_settings(
            &self,
            patch: StudioConnectionPatchRequest,
        ) -> GatewayResult<StoredStudioConnection> {
            patch.validate()?;
            let mut stored = self.studio_connection.lock().expect("lock poisoned");
            let mut connection = stored.clone().unwrap_or_default();

            match patch.base_url {
                PatchValue::Unchanged => {}
                PatchValue::Clear => {
                    connection.base_url = None;
                    connection.bearer_token_secret = None;
                }
                PatchValue::Set(value) => {
                    connection.base_url = Some(gateway_core::normalize_base_url(&value)?);
                }
            }

            match patch.token {
                PatchValue::Unchanged => {}
                PatchValue::Clear => {
                    connection.bearer_token_secret = None;
                }
                PatchValue::Set(value) => {
                    connection.bearer_token_secret = Some(gateway_core::normalize_secret(&value)?);
                }
            }

            connection.updated_at = Some(Utc::now());
            *stored = Some(connection.clone());
            Ok(connection)
        }
    }

    #[async_trait]
    impl PolicyLookup for MemoryStore {
        async fn policy_for_key(&self, _key_id: Uuid) -> GatewayResult<KeyPolicy> {
            let Some(key) = self.admin_key.lock().expect("lock poisoned").clone() else {
                return Ok(KeyPolicy::default());
            };
            Ok(KeyPolicy {
                deny: key.policy.deny,
                allowed_routes: key
                    .policy
                    .allowed_routes
                    .iter()
                    .filter_map(|route| match route.as_str() {
                        "/v1/chat/completions" => Some(Route::ChatCompletions),
                        "/v1/responses" => Some(Route::Responses),
                        "/providers/openai/*" => Some(Route::DirectOpenAi),
                        "/summary" => Some(Route::Summary),
                        "/translation" => Some(Route::Translation),
                        "/ocr" => Some(Route::Ocr),
                        "/embeddings" => Some(Route::Embeddings),
                        "/services/*" => Some(Route::ServiceWildcard),
                        _ => None,
                    })
                    .collect(),
                allowed_models: key.policy.allowed_models,
                allowed_providers: key
                    .policy
                    .allowed_providers
                    .iter()
                    .filter_map(|provider| parse_simulation_provider(provider).ok())
                    .collect(),
                allowed_services: key.policy.allowed_services,
                rpm_limit: key.policy.rpm_limit,
                tpm_limit: key.policy.tpm_limit,
                daily_budget_usd: key.policy.daily_budget_usd,
                monthly_budget_usd: key.policy.monthly_budget_usd,
                allow_streaming: key.policy.allow_streaming,
                allow_tools: key.policy.allow_tools,
                max_requests_per_day: key.policy.max_requests_per_day,
                max_tokens_per_day: key.policy.max_tokens_per_day,
                max_cost_per_request: key.policy.max_cost_per_request,
                max_input_tokens_per_request: key.policy.max_input_tokens_per_request,
                max_output_tokens_per_request: key.policy.max_output_tokens_per_request,
                allowed_hours_utc: key.policy.allowed_hours_utc,
                unused_key_auto_disable_after_days: key.policy.unused_key_auto_disable_after_days,
                max_request_body_bytes: key.policy.max_request_body_bytes,
                max_response_body_bytes: key.policy.max_response_body_bytes,
                max_stream_duration_seconds: key.policy.max_stream_duration_seconds,
                max_sse_event_bytes: key.policy.max_sse_event_bytes,
                max_tool_call_count: key.policy.max_tool_call_count,
                max_tool_schema_bytes: key.policy.max_tool_schema_bytes,
                policy_version: key.policy.policy_version,
            })
        }
    }

    #[async_trait]
    impl GuardrailStore for MemoryStore {
        async fn list_guardrail_definitions(
            &self,
        ) -> GatewayResult<Vec<gateway_core::GuardrailDefinition>> {
            Ok(vec![gateway_core::pii_redact_definition()])
        }

        async fn guardrail_policy_for_key(
            &self,
            _key_id: Uuid,
        ) -> GatewayResult<gateway_core::GuardrailPolicy> {
            Ok(self
                .admin_key
                .lock()
                .expect("lock poisoned")
                .as_ref()
                .map(|key| key.guardrail_policy.clone())
                .unwrap_or_default())
        }

        async fn upsert_guardrail_policy_for_key(
            &self,
            _key_id: Uuid,
            policy: &gateway_core::GuardrailPolicy,
        ) -> GatewayResult<()> {
            if let Some(key) = self.admin_key.lock().expect("lock poisoned").as_mut() {
                key.guardrail_policy = policy.clone();
            }
            Ok(())
        }

        async fn insert_guardrail_execution_event(
            &self,
            _event: &gateway_core::GuardrailExecutionEvent,
        ) -> GatewayResult<()> {
            Ok(())
        }
    }

    #[async_trait]
    impl GuardrailObservabilityStore for MemoryStore {
        async fn list_admin_guardrail_definitions(
            &self,
        ) -> GatewayResult<Vec<AdminGuardrailDefinitionResponse>> {
            Ok(vec![AdminGuardrailDefinitionResponse {
                name: "pii-redact".to_owned(),
                description: "Redacts common PII before provider calls and optionally restores placeholders after responses.".to_owned(),
                provider_kind: gateway_core::GuardrailProviderKind::BuiltIn,
                modes: vec![
                    GuardrailMode::PreCall,
                    GuardrailMode::PostCall,
                    GuardrailMode::DuringCall,
                ],
                default_on: false,
                failure_policy: gateway_core::GuardrailFailurePolicy::FailClosed,
                config_schema: serde_json::json!({ "restore_output": "boolean" }),
                runtime_config: serde_json::json!({ "restore_output": true }),
                enabled: true,
                endpoint_configured: false,
                endpoint_url: None,
                timeout_ms: None,
                token_configured: false,
            }])
        }

        async fn guardrail_execution_events(
            &self,
            _query: GuardrailEventQuery,
        ) -> GatewayResult<Vec<GuardrailExecutionEvent>> {
            Ok(Vec::new())
        }

        async fn guardrail_execution_summary(
            &self,
            _query: GuardrailEventQuery,
        ) -> GatewayResult<Vec<GuardrailExecutionSummary>> {
            Ok(Vec::new())
        }

        async fn create_http_guardrail(
            &self,
            request: GuardrailAdminCreateRequest,
        ) -> GatewayResult<AdminGuardrailDefinitionResponse> {
            Ok(AdminGuardrailDefinitionResponse {
                name: request.name,
                description: request.description,
                provider_kind: gateway_core::GuardrailProviderKind::Http,
                modes: request.modes,
                default_on: request.default_on,
                failure_policy: request.failure_policy,
                config_schema: request.config_schema,
                runtime_config: request.runtime_config,
                enabled: request.enabled,
                endpoint_configured: !request.endpoint_url.is_empty(),
                endpoint_url: Some(request.endpoint_url),
                timeout_ms: Some(request.timeout_ms.unwrap_or(1500).clamp(100, 10_000)),
                token_configured: request.bearer_token.is_some(),
            })
        }

        async fn patch_admin_guardrail(
            &self,
            name: String,
            request: GuardrailAdminPatchRequest,
        ) -> GatewayResult<AdminGuardrailDefinitionResponse> {
            if name == "pii-redact"
                && (request.description.is_some()
                    || request.endpoint_url.is_some()
                    || request.timeout_ms.is_some()
                    || request.bearer_token.is_some())
            {
                return Err(GatewayError::InvalidGuardrailRequest);
            }
            Ok(AdminGuardrailDefinitionResponse {
                description: request.description.unwrap_or_else(|| {
                    "Redacts common PII before provider calls and optionally restores placeholders after responses.".to_owned()
                }),
                provider_kind: if name == "pii-redact" {
                    gateway_core::GuardrailProviderKind::BuiltIn
                } else {
                    gateway_core::GuardrailProviderKind::Http
                },
                modes: request.modes.unwrap_or_else(|| vec![GuardrailMode::PreCall]),
                default_on: request.default_on.unwrap_or(false),
                failure_policy: request
                    .failure_policy
                    .unwrap_or(gateway_core::GuardrailFailurePolicy::FailClosed),
                config_schema: request
                    .config_schema
                    .unwrap_or_else(|| serde_json::json!({})),
                runtime_config: request
                    .runtime_config
                    .unwrap_or_else(|| serde_json::json!({})),
                enabled: request.enabled.unwrap_or(true),
                endpoint_configured: request.endpoint_url.is_some(),
                endpoint_url: request.endpoint_url,
                timeout_ms: request.timeout_ms,
                token_configured: request.bearer_token.flatten().is_some(),
                name,
            })
        }

        async fn delete_admin_guardrail(&self, name: String) -> GatewayResult<()> {
            if name == "pii-redact" || name == "unknown" {
                return Err(GatewayError::InvalidGuardrailRequest);
            }
            if let Some(key) = self.admin_key.lock().expect("lock poisoned").as_mut() {
                key.guardrail_policy
                    .mandatory_guardrails
                    .retain(|guardrail| guardrail != &name);
                key.guardrail_policy
                    .optional_guardrails
                    .retain(|guardrail| guardrail != &name);
                key.guardrail_policy
                    .forbidden_guardrails
                    .retain(|guardrail| guardrail != &name);
                key.guardrail_policy
                    .guardrail_config_overrides
                    .remove(&name);
            }
            Ok(())
        }
    }

    #[async_trait]
    impl AdminKeyStore for MemoryStore {
        async fn create_admin_key(
            &self,
            request: AdminKeyCreate,
            material: &VirtualKeyMaterial,
        ) -> GatewayResult<AdminKeyResponse> {
            let policy = request
                .preset
                .map(|preset| preset.apply(KeyPolicy::default()))
                .unwrap_or_default();
            let key = AdminKeyResponse {
                id: Uuid::new_v4(),
                owner_type: request.owner_type,
                project_id: request.project_id,
                service_names: request.service_names,
                key_prefix: material.key_prefix.clone(),
                disabled: false,
                revoked_at: None,
                expires_at: request.expires_at,
                rotation_due_at: request.rotation_due_at,
                last_used_at: None,
                policy: AdminPolicyResponse {
                    deny: policy.deny,
                    allowed_routes: policy
                        .allowed_routes
                        .iter()
                        .map(|route| route.as_str().to_owned())
                        .collect(),
                    allowed_models: policy.allowed_models,
                    allowed_providers: policy
                        .allowed_providers
                        .iter()
                        .map(|provider| provider.as_str().to_owned())
                        .collect(),
                    allowed_services: policy.allowed_services,
                    rpm_limit: policy.rpm_limit,
                    tpm_limit: policy.tpm_limit,
                    daily_budget_usd: policy.daily_budget_usd,
                    monthly_budget_usd: policy.monthly_budget_usd,
                    allow_streaming: policy.allow_streaming,
                    allow_tools: policy.allow_tools,
                    max_requests_per_day: policy.max_requests_per_day,
                    max_tokens_per_day: policy.max_tokens_per_day,
                    max_cost_per_request: policy.max_cost_per_request,
                    max_input_tokens_per_request: policy.max_input_tokens_per_request,
                    max_output_tokens_per_request: policy.max_output_tokens_per_request,
                    allowed_hours_utc: policy.allowed_hours_utc,
                    unused_key_auto_disable_after_days: policy.unused_key_auto_disable_after_days,
                    max_request_body_bytes: policy.max_request_body_bytes,
                    max_response_body_bytes: policy.max_response_body_bytes,
                    max_stream_duration_seconds: policy.max_stream_duration_seconds,
                    max_sse_event_bytes: policy.max_sse_event_bytes,
                    max_tool_call_count: policy.max_tool_call_count,
                    max_tool_schema_bytes: policy.max_tool_schema_bytes,
                    policy_version: policy.policy_version,
                },
                guardrail_policy: request.guardrail_policy,
                created_at: Utc::now(),
                updated_at: Utc::now(),
            };
            *self.admin_key.lock().expect("lock poisoned") = Some(key.clone());
            Ok(key)
        }

        async fn get_admin_key(&self, _key_id: Uuid) -> GatewayResult<Option<AdminKeyResponse>> {
            Ok(self.admin_key.lock().expect("lock poisoned").clone())
        }

        async fn list_admin_keys(&self) -> GatewayResult<Vec<AdminKeyResponse>> {
            Ok(self
                .admin_key
                .lock()
                .expect("lock poisoned")
                .clone()
                .into_iter()
                .collect())
        }

        async fn patch_admin_key(
            &self,
            _key_id: Uuid,
            patch: AdminKeyPatch,
        ) -> GatewayResult<Option<AdminKeyResponse>> {
            let mut key = self.admin_key.lock().expect("lock poisoned");
            if let Some(key) = key.as_mut() {
                if let Some(expires_at) = patch.expires_at {
                    key.expires_at = expires_at;
                }
                if let Some(rotation_due_at) = patch.rotation_due_at {
                    key.rotation_due_at = rotation_due_at;
                }
                if let Some(disabled) = patch.disabled {
                    key.disabled = disabled;
                }
                if let Some(guardrail_patch) = patch.guardrail_policy {
                    key.guardrail_policy = guardrail_patch.apply(key.guardrail_policy.clone())?;
                }
                key.updated_at = Utc::now();
            }
            Ok(key.clone())
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

        async fn enable_admin_key(&self, _key_id: Uuid) -> GatewayResult<Option<AdminKeyResponse>> {
            let mut key = self.admin_key.lock().expect("lock poisoned");
            if let Some(key) = key.as_mut() {
                if key.revoked_at.is_none() {
                    key.disabled = false;
                }
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
    impl AdminPolicyLayerStore for MemoryStore {
        async fn list_policy_layers(
            &self,
        ) -> GatewayResult<Vec<gateway_core::AdminPolicyLayerResponse>> {
            Ok(Vec::new())
        }

        async fn upsert_policy_layer(
            &self,
            request: AdminPolicyLayerUpsert,
        ) -> GatewayResult<gateway_core::AdminPolicyLayerResponse> {
            let now = Utc::now();
            Ok(gateway_core::AdminPolicyLayerResponse {
                id: Uuid::new_v4(),
                kind: request.kind,
                scope_id: request.scope_id,
                policy: AdminPolicyResponse {
                    deny: request.policy.deny.unwrap_or(false),
                    allowed_routes: request.policy.allowed_routes.unwrap_or_default(),
                    allowed_models: request.policy.allowed_models.unwrap_or_default(),
                    allowed_providers: request.policy.allowed_providers.unwrap_or_default(),
                    allowed_services: request.policy.allowed_services.unwrap_or_default(),
                    rpm_limit: request.policy.rpm_limit.flatten(),
                    tpm_limit: request.policy.tpm_limit.flatten(),
                    daily_budget_usd: request.policy.daily_budget_usd.flatten(),
                    monthly_budget_usd: request.policy.monthly_budget_usd.flatten(),
                    allow_streaming: request.policy.allow_streaming.unwrap_or(true),
                    allow_tools: request.policy.allow_tools.unwrap_or(true),
                    max_requests_per_day: request.policy.max_requests_per_day.flatten(),
                    max_tokens_per_day: request.policy.max_tokens_per_day.flatten(),
                    max_cost_per_request: request.policy.max_cost_per_request.flatten(),
                    max_input_tokens_per_request: request
                        .policy
                        .max_input_tokens_per_request
                        .flatten(),
                    max_output_tokens_per_request: request
                        .policy
                        .max_output_tokens_per_request
                        .flatten(),
                    allowed_hours_utc: request.policy.allowed_hours_utc.unwrap_or_default(),
                    unused_key_auto_disable_after_days: request
                        .policy
                        .unused_key_auto_disable_after_days
                        .flatten(),
                    max_request_body_bytes: request.policy.max_request_body_bytes.flatten(),
                    max_response_body_bytes: request.policy.max_response_body_bytes.flatten(),
                    max_stream_duration_seconds: request
                        .policy
                        .max_stream_duration_seconds
                        .flatten(),
                    max_sse_event_bytes: request.policy.max_sse_event_bytes.flatten(),
                    max_tool_call_count: request.policy.max_tool_call_count.flatten(),
                    max_tool_schema_bytes: request.policy.max_tool_schema_bytes.flatten(),
                    policy_version: 1,
                },
                guardrail_policy: gateway_core::GuardrailPolicy::default(),
                created_at: now,
                updated_at: now,
            })
        }

        async fn delete_policy_layer(&self, _layer_id: Uuid) -> GatewayResult<bool> {
            Ok(true)
        }
    }

    #[async_trait]
    impl AdminProjectStore for MemoryStore {
        async fn create_project(
            &self,
            request: ProjectCreateRequest,
        ) -> GatewayResult<ProjectResponse> {
            request.validate()?;
            let now = Utc::now();
            Ok(ProjectResponse {
                id: Uuid::new_v4(),
                name: request.name,
                service_names: Vec::new(),
                created_at: now,
                updated_at: now,
            })
        }

        async fn list_projects(&self) -> GatewayResult<Vec<ProjectResponse>> {
            Ok(Vec::new())
        }

        async fn get_project(&self, _project_id: Uuid) -> GatewayResult<Option<ProjectResponse>> {
            Ok(None)
        }

        async fn patch_project(
            &self,
            _project_id: Uuid,
            _patch: ProjectPatchRequest,
        ) -> GatewayResult<Option<ProjectResponse>> {
            Ok(None)
        }

        async fn delete_project(&self, _project_id: Uuid) -> GatewayResult<bool> {
            Ok(false)
        }
    }

    #[async_trait]
    impl AdminOpenAiRouteStore for MemoryStore {
        async fn list_openai_route_settings(&self) -> GatewayResult<Vec<OpenAiRouteSetting>> {
            Ok(self.openai_routes.lock().expect("lock poisoned").clone())
        }

        async fn set_openai_route_enabled(
            &self,
            route_id: &str,
            enabled: bool,
        ) -> GatewayResult<Option<OpenAiRouteSetting>> {
            let mut routes = self.openai_routes.lock().expect("lock poisoned");
            let Some(route) = routes.iter_mut().find(|route| route.route_id == route_id) else {
                return Ok(None);
            };
            route.enabled = enabled;
            route.updated_at = Utc::now();
            Ok(Some(route.clone()))
        }
    }

    #[async_trait]
    impl AdminProviderConfigStore for MemoryStore {
        async fn create_provider_config(
            &self,
            request: ProviderConfigCreateRequest,
        ) -> GatewayResult<ProviderConfigResponse> {
            request.validate()?;
            let now = Utc::now();
            Ok(ProviderConfigResponse {
                id: Uuid::new_v4(),
                provider: request.provider,
                name: request.name,
                base_url: request.base_url,
                enabled: request.enabled,
                credential_configured: request.credential.is_some(),
                created_at: now,
                updated_at: now,
            })
        }

        async fn list_provider_configs(&self) -> GatewayResult<Vec<ProviderConfigResponse>> {
            Ok(Vec::new())
        }

        async fn get_provider_config(
            &self,
            _provider_id: Uuid,
        ) -> GatewayResult<Option<ProviderConfigResponse>> {
            Ok(None)
        }

        async fn patch_provider_config(
            &self,
            _provider_id: Uuid,
            _patch: ProviderConfigPatchRequest,
        ) -> GatewayResult<Option<ProviderConfigResponse>> {
            Ok(None)
        }

        async fn delete_provider_config(&self, _provider_id: Uuid) -> GatewayResult<bool> {
            Ok(false)
        }

        async fn set_provider_config_enabled(
            &self,
            _provider_id: Uuid,
            _enabled: bool,
        ) -> GatewayResult<Option<ProviderConfigResponse>> {
            Ok(None)
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
                project_id: request.project_id,
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
            if let Some(route_pattern) = patch.route_pattern {
                service.route_pattern = route_pattern;
            }
            if let Some(upstream_base_url) = patch.upstream_base_url {
                service.upstream_base_url = upstream_base_url;
            }
            if let Some(allowed_methods) = patch.allowed_methods {
                service.allowed_methods = allowed_methods;
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
                project_id: request.project_id,
                studio_service_id: Some(request.studio_service_id),
                route_pattern: request
                    .route_pattern
                    .unwrap_or_else(|| format!("/services/{}/*", request.name)),
                upstream_base_url: request.upstream_base_url.clone(),
                enabled: false,
                allowed_methods: request.allowed_methods.clone(),
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
                missing_runtime_fields: missing_runtime_fields(
                    request.upstream_base_url.as_deref(),
                    None,
                ),
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
        ) -> GatewayResult<OperatorAuthorization> {
            if self
                .operator_tokens
                .lock()
                .expect("lock poisoned")
                .iter()
                .any(|token| token == raw_token)
            {
                let scopes = match raw_token {
                    TEST_USAGE_OPERATOR_TOKEN => vec![SCOPE_USAGE_READ.to_owned()],
                    TEST_POLICY_OPERATOR_TOKEN => vec![SCOPE_POLICIES_UPDATE.to_owned()],
                    _ => default_operator_scopes(),
                };
                Ok(OperatorAuthorization {
                    token_id: Uuid::nil(),
                    token_prefix: raw_token.chars().take(16).collect(),
                    roles: default_operator_roles(),
                    scopes,
                })
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

        async fn usage_export(&self, query: UsageQuery) -> GatewayResult<UsageExport> {
            let rows: Vec<UsageExportRow> = self
                .events
                .lock()
                .expect("lock poisoned")
                .iter()
                .filter(|event| usage_event_matches_query(event, &query))
                .map(|event| UsageExportRow {
                    request_id: event.request_id.clone(),
                    key_id: event.key_id,
                    project_id: event.project_id,
                    route: event.route.as_str().to_owned(),
                    model: event.model.clone(),
                    provider: event.provider.as_str().to_owned(),
                    status: match event.status {
                        gateway_core::UsageStatus::Success => "success".to_owned(),
                        gateway_core::UsageStatus::Failure => "failure".to_owned(),
                    },
                    status_code: i32::from(event.status_code),
                    latency_ms: event.latency_ms,
                    input_tokens: event.input_tokens.unwrap_or_default(),
                    output_tokens: event.output_tokens.unwrap_or_default(),
                    total_tokens: event.total_tokens.unwrap_or_default(),
                    estimated_cost_usd: event.estimated_cost_usd,
                    service_name: event.service_name.clone(),
                    task_id: event.task_id.clone(),
                    run_id: event.run_id.clone(),
                    fallback_count: event.fallback_count,
                    guardrail_action_count: 0,
                    created_at: event.created_at,
                })
                .collect();
            let summary = usage_summary_from_rows(&rows);
            let offset = query.offset.unwrap_or_default().max(0) as usize;
            let limit = query.limit.unwrap_or(1_000).clamp(1, 10_000) as usize;
            let rows = rows.into_iter().skip(offset).take(limit).collect();
            Ok(UsageExport { summary, rows })
        }

        async fn provider_health(&self, _query: UsageQuery) -> GatewayResult<Vec<ProviderHealth>> {
            Ok(Vec::new())
        }
    }

    #[async_trait]
    impl ProviderIntelligenceStore for MemoryStore {
        async fn list_provider_health_states(&self) -> GatewayResult<Vec<ProviderHealthState>> {
            Ok(Vec::new())
        }

        async fn provider_health_check_targets(
            &self,
        ) -> GatewayResult<Vec<gateway_core::ProviderHealthCheckTarget>> {
            Ok(Vec::new())
        }

        async fn upsert_provider_health_state(
            &self,
            state: ProviderHealthState,
        ) -> GatewayResult<ProviderHealthState> {
            Ok(state)
        }

        async fn get_debug_bundle(
            &self,
            _request_id: &str,
        ) -> GatewayResult<Option<gateway_core::DebugBundle>> {
            Ok(None)
        }

        async fn insert_debug_bundle(
            &self,
            _bundle: gateway_core::DebugBundle,
        ) -> GatewayResult<()> {
            Ok(())
        }

        async fn list_service_registry_snapshots(
            &self,
        ) -> GatewayResult<Vec<ServiceRegistrySnapshot>> {
            Ok(Vec::new())
        }

        async fn insert_service_registry_snapshot(
            &self,
            mut snapshot: ServiceRegistrySnapshot,
        ) -> GatewayResult<ServiceRegistrySnapshot> {
            snapshot.version = 1;
            Ok(snapshot)
        }

        async fn service_registry_snapshot(
            &self,
            _version: i64,
        ) -> GatewayResult<Option<ServiceRegistrySnapshot>> {
            Ok(None)
        }

        async fn activate_service_registry_import(
            &self,
            source: String,
            diff: ServiceImportDiff,
            services: Vec<StudioServiceImportRequest>,
            rolled_back_from_version: Option<i64>,
        ) -> GatewayResult<(ServiceRegistrySnapshot, Vec<ServiceResponse>)> {
            let mut activated = Vec::new();
            for service in services.clone() {
                activated.push(self.import_studio_service(service).await?);
            }
            Ok((
                ServiceRegistrySnapshot {
                    version: 1,
                    source,
                    diff,
                    services_json: serde_json::to_value(services)
                        .map_err(|_| GatewayError::InvalidServicePayload)?,
                    activated_at: Some(Utc::now()),
                    rolled_back_from_version,
                    created_at: Utc::now(),
                },
                activated,
            ))
        }
    }

    fn usage_event_matches_query(event: &UsageEvent, query: &UsageQuery) -> bool {
        if query.from.is_some_and(|from| event.created_at < from) {
            return false;
        }
        if query.to.is_some_and(|to| event.created_at >= to) {
            return false;
        }
        if query
            .project_id
            .is_some_and(|project_id| event.project_id != Some(project_id))
        {
            return false;
        }
        if query.key_id.is_some_and(|key_id| event.key_id != key_id) {
            return false;
        }
        if query
            .route
            .as_deref()
            .is_some_and(|route| event.route.as_str() != route)
        {
            return false;
        }
        if query
            .provider
            .as_deref()
            .is_some_and(|provider| event.provider.as_str() != provider)
        {
            return false;
        }
        if query
            .service
            .as_deref()
            .is_some_and(|service| event.service_name.as_deref() != Some(service))
        {
            return false;
        }
        if query
            .task_id
            .as_deref()
            .is_some_and(|task_id| event.task_id.as_deref() != Some(task_id))
        {
            return false;
        }
        if query
            .model
            .as_deref()
            .is_some_and(|model| event.model.as_deref() != Some(model))
        {
            return false;
        }
        if query.status.as_deref().is_some_and(|status| {
            let event_status = match event.status {
                gateway_core::UsageStatus::Success => "success",
                gateway_core::UsageStatus::Failure => "failure",
            };
            event_status != status
        }) {
            return false;
        }
        true
    }

    fn usage_summary_from_rows(rows: &[UsageExportRow]) -> UsageSummary {
        UsageSummary {
            request_count: i64::try_from(rows.len()).unwrap_or(i64::MAX),
            success_count: i64::try_from(rows.iter().filter(|row| row.status == "success").count())
                .unwrap_or(i64::MAX),
            failure_count: i64::try_from(rows.iter().filter(|row| row.status == "failure").count())
                .unwrap_or(i64::MAX),
            input_tokens: rows.iter().map(|row| row.input_tokens).sum(),
            output_tokens: rows.iter().map(|row| row.output_tokens).sum(),
            total_tokens: rows.iter().map(|row| row.total_tokens).sum(),
            estimated_cost_usd: Some(
                rows.iter()
                    .filter_map(|row| row.estimated_cost_usd)
                    .sum::<f64>(),
            ),
            total_latency_ms: rows.iter().map(|row| row.latency_ms).sum(),
            fallback_count: rows.iter().map(|row| i64::from(row.fallback_count)).sum(),
        }
    }

    fn stored_key(raw: &str) -> StoredVirtualKey {
        let material = VirtualKeyMaterial::from_raw(raw.to_owned()).expect("key material");
        StoredVirtualKey {
            id: Uuid::new_v4(),
            project_id: Some(Uuid::new_v4()),
            key_prefix: material.key_prefix,
            key_hash: material.key_hash,
            disabled: false,
            revoked_at: None,
            expires_at: None,
        }
    }

    fn admin_key_for(
        stored: &StoredVirtualKey,
        guardrail_policy: gateway_core::GuardrailPolicy,
    ) -> AdminKeyResponse {
        let now = Utc::now();
        AdminKeyResponse {
            id: stored.id,
            owner_type: gateway_core::AdminKeyOwnerType::Project,
            project_id: stored.project_id,
            service_names: Vec::new(),
            key_prefix: stored.key_prefix.clone(),
            disabled: false,
            revoked_at: None,
            expires_at: None,
            rotation_due_at: None,
            last_used_at: None,
            policy: AdminPolicyResponse {
                deny: false,
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
                max_requests_per_day: None,
                max_tokens_per_day: None,
                max_cost_per_request: None,
                max_input_tokens_per_request: None,
                max_output_tokens_per_request: None,
                allowed_hours_utc: Vec::new(),
                unused_key_auto_disable_after_days: None,
                max_request_body_bytes: None,
                max_response_body_bytes: None,
                max_stream_duration_seconds: None,
                max_sse_event_bytes: None,
                max_tool_call_count: None,
                max_tool_schema_bytes: None,
                policy_version: 1,
            },
            guardrail_policy,
            created_at: now,
            updated_at: now,
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
            roles: default_operator_roles(),
            scopes: default_operator_scopes(),
            disabled: false,
            revoked_at: None,
            last_used_at: None,
            created_at: now,
            updated_at: now,
        }
    }

    const TEST_OPERATOR_TOKEN: &str =
        "op_live_testoperator000000000000000000000000000000000000000000000000";
    const TEST_USAGE_OPERATOR_TOKEN: &str =
        "op_live_usageoperator000000000000000000000000000000000000000000000";
    const TEST_POLICY_OPERATOR_TOKEN: &str =
        "op_live_policyoperator00000000000000000000000000000000000000000000";

    fn test_state(store: MemoryStore) -> AppState {
        test_state_with_redis_url(store, "redis://127.0.0.1:6379")
    }

    fn test_state_with_redis_url(store: MemoryStore, redis_url: &str) -> AppState {
        let redis = RedisReadiness::new(redis_url).expect("redis client");
        AppState {
            store: Arc::new(store),
            redis,
            studio_env: StudioConnectionEnv::default(),
        }
    }

    fn test_state_with_studio_env(store: MemoryStore, studio_env: StudioConnectionEnv) -> AppState {
        let redis = RedisReadiness::new("redis://127.0.0.1:6379").expect("redis client");
        AppState {
            store: Arc::new(store),
            redis,
            studio_env,
        }
    }

    fn default_store() -> MemoryStore {
        MemoryStore {
            key: Arc::new(Mutex::new(None)),
            admin_key: Arc::new(Mutex::new(None)),
            services: Arc::new(Mutex::new(Vec::new())),
            openai_routes: Arc::new(Mutex::new(default_openai_routes())),
            operator_tokens: Arc::new(Mutex::new(vec![TEST_OPERATOR_TOKEN.to_owned()])),
            events: Arc::new(Mutex::new(Vec::new())),
            audit_events: Arc::new(Mutex::new(Vec::new())),
            studio_connection: Arc::new(Mutex::new(None)),
            postgres_ready: true,
        }
    }

    fn default_openai_routes() -> Vec<OpenAiRouteSetting> {
        let now = Utc::now();
        vec![
            OpenAiRouteSetting {
                route_id: "chat-completions".to_owned(),
                route: "/v1/chat/completions".to_owned(),
                enabled: true,
                updated_at: now,
            },
            OpenAiRouteSetting {
                route_id: "responses".to_owned(),
                route: "/v1/responses".to_owned(),
                enabled: true,
                updated_at: now,
            },
        ]
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

    async fn admin_delete(app: Router, route: &str, token: Option<&str>) -> Response {
        let mut builder = axum::http::Request::builder()
            .method(axum::http::Method::DELETE)
            .uri(route)
            .header("x-request-id", "req_test");
        if let Some(token) = token {
            builder = builder.header("authorization", format!("Bearer {token}"));
        }

        app.oneshot(builder.body(axum::body::Body::empty()).expect("request"))
            .await
            .expect("response")
    }

    async fn response_json(response: Response) -> serde_json::Value {
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        serde_json::from_slice(&body).expect("json")
    }

    #[tokio::test]
    async fn healthz_returns_ok() {
        let store = MemoryStore {
            key: Arc::new(Mutex::new(None)),
            admin_key: Arc::new(Mutex::new(None)),
            services: Arc::new(Mutex::new(Vec::new())),
            openai_routes: Arc::new(Mutex::new(default_openai_routes())),
            operator_tokens: Arc::new(Mutex::new(vec![TEST_OPERATOR_TOKEN.to_owned()])),
            events: Arc::new(Mutex::new(Vec::new())),
            audit_events: Arc::new(Mutex::new(Vec::new())),
            postgres_ready: true,
            studio_connection: Arc::new(Mutex::new(None)),
        };

        let app = router_with_state(test_state_with_redis_url(store, "redis://127.0.0.1:0"));
        let response = request(app, "/admin-ui/healthz").await;

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn generation_routes_are_not_served_by_axum_control_api() {
        let raw = "rk_live_1234567890abcdef";
        let store = MemoryStore {
            key: Arc::new(Mutex::new(Some(stored_key(raw)))),
            admin_key: Arc::new(Mutex::new(None)),
            services: Arc::new(Mutex::new(Vec::new())),
            openai_routes: Arc::new(Mutex::new(default_openai_routes())),
            operator_tokens: Arc::new(Mutex::new(vec![TEST_OPERATOR_TOKEN.to_owned()])),
            events: Arc::new(Mutex::new(Vec::new())),
            audit_events: Arc::new(Mutex::new(Vec::new())),
            postgres_ready: true,
            studio_connection: Arc::new(Mutex::new(None)),
        };

        let app = router_with_state(test_state(store.clone()));
        let response = request(app, "/v1/chat/completions").await;

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert!(store.events.lock().expect("lock poisoned").is_empty());
    }

    #[tokio::test]
    async fn guardrail_list_requires_virtual_key_and_returns_allowed_definitions() {
        let raw = "rk_live_guardrailtest1";
        let stored = stored_key(raw);
        let store = default_store();
        *store.key.lock().expect("lock poisoned") = Some(stored.clone());
        *store.admin_key.lock().expect("lock poisoned") = Some(admin_key_for(
            &stored,
            gateway_core::GuardrailPolicy {
                mandatory_guardrails: vec!["pii-redact".to_owned()],
                ..gateway_core::GuardrailPolicy::default()
            },
        ));
        let app = router_with_state(test_state(store));

        let unauthorized = request(app.clone(), "/admin-ui/v1/guardrails").await;
        assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

        let response = admin_get(app, "/admin-ui/v1/guardrails", Some(raw)).await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let value: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(value["guardrails"][0]["name"], "pii-redact");
    }

    #[tokio::test]
    async fn guardrail_test_runs_pii_redact_without_provider_call() {
        let raw = "rk_live_guardrailtest2";
        let stored = stored_key(raw);
        let store = default_store();
        *store.key.lock().expect("lock poisoned") = Some(stored.clone());
        *store.admin_key.lock().expect("lock poisoned") = Some(admin_key_for(
            &stored,
            gateway_core::GuardrailPolicy {
                optional_guardrails: vec!["pii-redact".to_owned()],
                ..gateway_core::GuardrailPolicy::default()
            },
        ));
        let app = router_with_state(test_state(store.clone()));
        let response = admin_post(
            app,
            "/admin-ui/v1/guardrails/test",
            Some(raw),
            r#"{"guardrails":["pii-redact"],"mode":"pre_call","input":{"messages":[{"role":"user","content":"email john@example.com"}]}}"#,
        )
        .await;

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let value: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert!(value.to_string().contains("[EMAIL_1]"));
        assert!(!value.to_string().contains("john@example.com"));
        assert!(store.events.lock().expect("lock poisoned").is_empty());
    }

    #[tokio::test]
    async fn admin_guardrail_catalog_requires_operator_token() {
        let app = router_with_state(test_state(default_store()));

        let unauthorized = admin_get(app.clone(), "/admin-ui/admin/guardrails", None).await;
        assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

        let response =
            admin_get(app, "/admin-ui/admin/guardrails", Some(TEST_OPERATOR_TOKEN)).await;
        assert_eq!(response.status(), StatusCode::OK);
        let value = response_json(response).await;
        assert_eq!(value["guardrails"][0]["name"], "pii-redact");
        assert!(value["guardrails"][0]["endpoint_url"].is_null());
    }

    #[tokio::test]
    async fn admin_guardrail_create_redacts_http_provider_secret() {
        let app = router_with_state(test_state(default_store()));
        let response = admin_post(
            app,
            "/admin-ui/admin/guardrails",
            Some(TEST_OPERATOR_TOKEN),
            r#"{"name":"custom-check","description":"Custom check","endpoint_url":"https://guardrail.example/check","modes":["pre_call"],"failure_policy":"fail_open","bearer_token":"secret"}"#,
        )
        .await;

        assert_eq!(response.status(), StatusCode::OK);
        let value = response_json(response).await;
        assert_eq!(value["name"], "custom-check");
        assert_eq!(value["provider_kind"], "http");
        assert_eq!(value["token_configured"], true);
        assert!(value.get("bearer_token").is_none());
        assert_eq!(value["runtime_config"], serde_json::json!({}));
        assert_eq!(value["endpoint_url"], "https://guardrail.example/check");
        assert_eq!(value["timeout_ms"], 1500);
    }

    #[tokio::test]
    async fn admin_guardrail_patch_allows_builtin_safe_fields_only() {
        let app = router_with_state(test_state(default_store()));
        let response = admin_patch(
            app.clone(),
            "/admin-ui/admin/guardrails/pii-redact",
            Some(TEST_OPERATOR_TOKEN),
            r#"{"enabled":false,"default_on":true,"failure_policy":"dry_run","modes":["pre_call"],"config_schema":{"restore_output":"boolean"},"runtime_config":{"restore_output":false}}"#,
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let value = response_json(response).await;
        assert_eq!(value["provider_kind"], "built_in");
        assert_eq!(value["enabled"], false);
        assert_eq!(value["default_on"], true);
        assert_eq!(value["runtime_config"]["restore_output"], false);

        let rejected = admin_patch(
            app,
            "/admin-ui/admin/guardrails/pii-redact",
            Some(TEST_OPERATOR_TOKEN),
            r#"{"endpoint_url":"https://guardrail.example/check"}"#,
        )
        .await;
        assert_eq!(rejected.status(), StatusCode::BAD_REQUEST);
        let value = response_json(rejected).await;
        assert_eq!(value["error"]["code"], "invalid_guardrail_request");
    }

    #[tokio::test]
    async fn admin_guardrail_delete_rejects_builtin_and_cleans_key_policy() {
        let raw = "rk_live_guardraildelete";
        let stored = stored_key(raw);
        let store = default_store();
        *store.admin_key.lock().expect("lock poisoned") = Some(admin_key_for(
            &stored,
            gateway_core::GuardrailPolicy {
                mandatory_guardrails: vec!["custom-check".to_owned(), "pii-redact".to_owned()],
                optional_guardrails: vec!["custom-check".to_owned()],
                forbidden_guardrails: vec!["custom-check".to_owned()],
                guardrail_config_overrides: std::collections::BTreeMap::from([(
                    "custom-check".to_owned(),
                    serde_json::json!({ "threshold": 0.9 }),
                )]),
            },
        ));
        let app = router_with_state(test_state(store.clone()));

        let rejected = admin_delete(
            app.clone(),
            "/admin-ui/admin/guardrails/pii-redact",
            Some(TEST_OPERATOR_TOKEN),
        )
        .await;
        assert_eq!(rejected.status(), StatusCode::BAD_REQUEST);

        let deleted = admin_delete(
            app,
            "/admin-ui/admin/guardrails/custom-check",
            Some(TEST_OPERATOR_TOKEN),
        )
        .await;
        assert_eq!(deleted.status(), StatusCode::NO_CONTENT);

        let key = store
            .admin_key
            .lock()
            .expect("lock poisoned")
            .clone()
            .expect("admin key");
        assert_eq!(
            key.guardrail_policy.mandatory_guardrails,
            vec!["pii-redact"]
        );
        assert!(key.guardrail_policy.optional_guardrails.is_empty());
        assert!(key.guardrail_policy.forbidden_guardrails.is_empty());
        assert!(key.guardrail_policy.guardrail_config_overrides.is_empty());
    }

    #[tokio::test]
    async fn readyz_returns_unavailable_when_store_is_not_ready() {
        let raw = "rk_live_1234567890abcdef";
        let store = MemoryStore {
            key: Arc::new(Mutex::new(Some(stored_key(raw)))),
            admin_key: Arc::new(Mutex::new(None)),
            services: Arc::new(Mutex::new(Vec::new())),
            openai_routes: Arc::new(Mutex::new(default_openai_routes())),
            operator_tokens: Arc::new(Mutex::new(vec![TEST_OPERATOR_TOKEN.to_owned()])),
            events: Arc::new(Mutex::new(Vec::new())),
            audit_events: Arc::new(Mutex::new(Vec::new())),
            postgres_ready: false,
            studio_connection: Arc::new(Mutex::new(None)),
        };

        let app = router_with_state(test_state(store));
        let response = request(app, "/admin-ui/readyz").await;

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn admin_create_key_requires_admin_token() {
        let store = MemoryStore {
            key: Arc::new(Mutex::new(None)),
            admin_key: Arc::new(Mutex::new(None)),
            services: Arc::new(Mutex::new(Vec::new())),
            openai_routes: Arc::new(Mutex::new(default_openai_routes())),
            operator_tokens: Arc::new(Mutex::new(vec![TEST_OPERATOR_TOKEN.to_owned()])),
            events: Arc::new(Mutex::new(Vec::new())),
            audit_events: Arc::new(Mutex::new(Vec::new())),
            postgres_ready: true,
            studio_connection: Arc::new(Mutex::new(None)),
        };
        let app = router_with_state(test_state(store));
        let project_id = Uuid::new_v4();
        let response = admin_post(
            app,
            "/admin-ui/admin/keys",
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
            openai_routes: Arc::new(Mutex::new(default_openai_routes())),
            operator_tokens: Arc::new(Mutex::new(vec![TEST_OPERATOR_TOKEN.to_owned()])),
            events: Arc::new(Mutex::new(Vec::new())),
            audit_events: Arc::new(Mutex::new(Vec::new())),
            postgres_ready: true,
            studio_connection: Arc::new(Mutex::new(None)),
        };
        let app = router_with_state(test_state(store));
        let project_id = Uuid::new_v4();
        let response = admin_post(
            app,
            "/admin-ui/admin/keys",
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
    async fn admin_create_key_denies_operator_without_key_scope() {
        let store = MemoryStore {
            operator_tokens: Arc::new(Mutex::new(vec![TEST_USAGE_OPERATOR_TOKEN.to_owned()])),
            ..default_store()
        };
        let app = router_with_state(test_state(store));
        let project_id = Uuid::new_v4();
        let response = admin_post(
            app,
            "/admin-ui/admin/keys",
            Some(TEST_USAGE_OPERATOR_TOKEN),
            &format!(r#"{{"project_id":"{project_id}"}}"#),
        )
        .await;

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let value: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(value["error"]["code"], "insufficient_operator_scope");
        assert_eq!(value["error"]["request_id"], "req_test");
    }

    #[tokio::test]
    async fn admin_create_key_applies_safe_preset_and_lifecycle_metadata() {
        let app = router_with_state(test_state(default_store()));
        let project_id = Uuid::new_v4();
        let response = admin_post(
            app,
            "/admin-ui/admin/keys",
            Some(TEST_OPERATOR_TOKEN),
            &format!(
                r#"{{"project_id":"{project_id}","preset":"external_partner","rotation_due_at":"2030-01-01T00:00:00Z"}}"#
            ),
        )
        .await;

        assert_eq!(response.status(), StatusCode::OK);
        let value = response_json(response).await;
        assert_eq!(value["key"]["rotation_due_at"], "2030-01-01T00:00:00Z");
        assert_eq!(value["key"]["last_used_at"], serde_json::Value::Null);
        assert_eq!(value["key"]["policy"]["rpm_limit"], 30);
        assert_eq!(value["key"]["policy"]["max_cost_per_request"], 0.25);
        assert_eq!(value["key"]["policy"]["max_request_body_bytes"], 262144);
        assert!(value["raw_key"].as_str().is_some());
        assert!(value["key"].get("raw_key").is_none());
    }

    #[tokio::test]
    async fn admin_policy_simulator_explains_denied_streaming_request() {
        let app = router_with_state(test_state(default_store()));
        let response = admin_post(
            app,
            "/admin-ui/admin/policy/simulate",
            Some(TEST_OPERATOR_TOKEN),
            r#"{"path":"/v1/chat/completions","body":{"model":"gpt-4.1-mini","stream":true}}"#,
        )
        .await;

        assert_eq!(response.status(), StatusCode::OK);
        let value = response_json(response).await;
        assert_eq!(value["auth"]["source"], "default_policy");
        assert_eq!(value["route_match"]["route"], "/v1/chat/completions");
        assert_eq!(value["route_match"]["provider"], "litellm");
        assert_eq!(value["policy_merge"]["policy_version"], 1);
        assert_eq!(value["final_decision"]["allowed"], false);
        assert_eq!(value["final_decision"]["error_code"], "policy_denied");
    }

    #[tokio::test]
    async fn admin_policy_simulator_accepts_unsaved_policy_patch() {
        let app = router_with_state(test_state(default_store()));
        let response = admin_post(
            app,
            "/admin-ui/admin/policy/simulate",
            Some(TEST_OPERATOR_TOKEN),
            r#"{"path":"/v1/chat/completions","body":{"model":"gpt-4.1-mini","stream":true},"policy":{"allow_streaming":true}}"#,
        )
        .await;

        assert_eq!(response.status(), StatusCode::OK);
        let value = response_json(response).await;
        assert_eq!(value["final_decision"]["allowed"], true);
    }

    #[tokio::test]
    async fn admin_policy_layers_can_be_upserted() {
        let app = router_with_state(test_state(default_store()));
        let response = admin_post(
            app,
            "/admin-ui/admin/policy-layers",
            Some(TEST_OPERATOR_TOKEN),
            r#"{"kind":"route","scope_id":"/v1/chat/completions","policy":{"max_response_body_bytes":1024,"allow_streaming":true,"allow_tools":true}}"#,
        )
        .await;

        assert_eq!(response.status(), StatusCode::OK);
        let value = response_json(response).await;
        assert_eq!(value["kind"], "route");
        assert_eq!(value["scope_id"], "/v1/chat/completions");
        assert_eq!(value["policy"]["max_response_body_bytes"], 1024);
        assert!(value.get("raw_key").is_none());
    }

    #[tokio::test]
    async fn admin_create_key_writes_audit_event_without_raw_key() {
        let store = default_store();
        let audit_events = store.audit_events.clone();
        let app = router_with_state(test_state(store));
        let project_id = Uuid::new_v4();
        let response = admin_post(
            app.clone(),
            "/admin-ui/admin/keys",
            Some(TEST_OPERATOR_TOKEN),
            &format!(r#"{{"project_id":"{project_id}"}}"#),
        )
        .await;

        assert_eq!(response.status(), StatusCode::OK);
        {
            let events = audit_events.lock().expect("lock poisoned");
            assert_eq!(events.len(), 1);
            let event = &events[0];
            assert_eq!(event.action, "keys:create");
            assert_eq!(event.target_type, "key");
            assert_eq!(event.request_id, "req_test");
            let after = event.after.as_ref().expect("after json");
            assert!(after.get("key_hash").is_none());
            assert!(after.get("raw_key").is_none());
        }

        let response = admin_get(
            app,
            "/admin-ui/admin/audit-events",
            Some(TEST_OPERATOR_TOKEN),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let value: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(value[0]["action"], "keys:create");
        assert_eq!(value[0]["target_type"], "key");
    }

    #[tokio::test]
    async fn admin_key_create_returns_guardrail_config_overrides() {
        let store = MemoryStore {
            key: Arc::new(Mutex::new(None)),
            admin_key: Arc::new(Mutex::new(None)),
            services: Arc::new(Mutex::new(Vec::new())),
            openai_routes: Arc::new(Mutex::new(default_openai_routes())),
            operator_tokens: Arc::new(Mutex::new(vec![TEST_OPERATOR_TOKEN.to_owned()])),
            events: Arc::new(Mutex::new(Vec::new())),
            audit_events: Arc::new(Mutex::new(Vec::new())),
            postgres_ready: true,
            studio_connection: Arc::new(Mutex::new(None)),
        };
        let app = router_with_state(test_state(store));
        let project_id = Uuid::new_v4();
        let response = admin_post(
            app,
            "/admin-ui/admin/keys",
            Some(TEST_OPERATOR_TOKEN),
            &format!(
                r#"{{"project_id":"{project_id}","guardrail_policy":{{"mandatory_guardrails":["pii-redact"],"guardrail_config_overrides":{{"pii-redact":{{"restore_output":false}}}}}}}}"#
            ),
        )
        .await;

        assert_eq!(response.status(), StatusCode::OK);
        let value = response_json(response).await;
        assert_eq!(
            value["key"]["guardrail_policy"]["guardrail_config_overrides"]["pii-redact"]
                ["restore_output"],
            false
        );
    }

    #[tokio::test]
    async fn admin_key_create_and_patch_support_no_expiration() {
        let store = MemoryStore {
            key: Arc::new(Mutex::new(None)),
            admin_key: Arc::new(Mutex::new(None)),
            services: Arc::new(Mutex::new(Vec::new())),
            openai_routes: Arc::new(Mutex::new(default_openai_routes())),
            operator_tokens: Arc::new(Mutex::new(vec![TEST_OPERATOR_TOKEN.to_owned()])),
            events: Arc::new(Mutex::new(Vec::new())),
            audit_events: Arc::new(Mutex::new(Vec::new())),
            postgres_ready: true,
            studio_connection: Arc::new(Mutex::new(None)),
        };
        let app = router_with_state(test_state(store));
        let project_id = Uuid::new_v4();
        let response = admin_post(
            app.clone(),
            "/admin-ui/admin/keys",
            Some(TEST_OPERATOR_TOKEN),
            &format!(r#"{{"project_id":"{project_id}","expires_at":null}}"#),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let value: serde_json::Value = serde_json::from_slice(&body).expect("json");
        let key_id = value["key"]["id"].as_str().expect("key id");
        assert!(value["key"]["expires_at"].is_null());

        let response = admin_patch(
            app.clone(),
            &format!("/admin-ui/admin/keys/{key_id}"),
            Some(TEST_OPERATOR_TOKEN),
            r#"{"expires_at":"2030-01-01T00:00:00Z"}"#,
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let value: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(value["expires_at"], "2030-01-01T00:00:00Z");

        let response = admin_patch(
            app,
            &format!("/admin-ui/admin/keys/{key_id}"),
            Some(TEST_OPERATOR_TOKEN),
            r#"{"expires_at":null}"#,
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let value: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert!(value["key"]["expires_at"].is_null());
    }

    #[tokio::test]
    async fn admin_key_patch_requires_key_disable_scope_for_disabled_field() {
        let raw = "rk_live_patchdisabled";
        let stored = stored_key(raw);
        let store = MemoryStore {
            key: Arc::new(Mutex::new(Some(stored.clone()))),
            admin_key: Arc::new(Mutex::new(Some(admin_key_for(
                &stored,
                gateway_core::GuardrailPolicy::default(),
            )))),
            services: Arc::new(Mutex::new(Vec::new())),
            openai_routes: Arc::new(Mutex::new(default_openai_routes())),
            operator_tokens: Arc::new(Mutex::new(vec![TEST_POLICY_OPERATOR_TOKEN.to_owned()])),
            events: Arc::new(Mutex::new(Vec::new())),
            audit_events: Arc::new(Mutex::new(Vec::new())),
            postgres_ready: true,
            studio_connection: Arc::new(Mutex::new(None)),
        };
        let app = router_with_state(test_state(store));
        let response = admin_patch(
            app,
            &format!("/admin-ui/admin/keys/{}", stored.id),
            Some(TEST_POLICY_OPERATOR_TOKEN),
            r#"{"disabled":true}"#,
        )
        .await;

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let value = response_json(response).await;
        assert_eq!(value["error"]["code"], "insufficient_operator_scope");
    }

    #[tokio::test]
    async fn admin_project_create_returns_generated_uuid() {
        let store = MemoryStore {
            key: Arc::new(Mutex::new(None)),
            admin_key: Arc::new(Mutex::new(None)),
            services: Arc::new(Mutex::new(Vec::new())),
            openai_routes: Arc::new(Mutex::new(default_openai_routes())),
            operator_tokens: Arc::new(Mutex::new(vec![TEST_OPERATOR_TOKEN.to_owned()])),
            events: Arc::new(Mutex::new(Vec::new())),
            audit_events: Arc::new(Mutex::new(Vec::new())),
            postgres_ready: true,
            studio_connection: Arc::new(Mutex::new(None)),
        };
        let app = router_with_state(test_state(store));
        let response = admin_post(
            app,
            "/admin-ui/admin/projects",
            Some(TEST_OPERATOR_TOKEN),
            r#"{"name":"Studio"}"#,
        )
        .await;

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let value: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(value["name"], "Studio");
        assert!(Uuid::parse_str(value["id"].as_str().expect("project id")).is_ok());
    }

    #[tokio::test]
    async fn admin_provider_create_redacts_master_key() {
        let store = MemoryStore {
            key: Arc::new(Mutex::new(None)),
            admin_key: Arc::new(Mutex::new(None)),
            services: Arc::new(Mutex::new(Vec::new())),
            openai_routes: Arc::new(Mutex::new(default_openai_routes())),
            operator_tokens: Arc::new(Mutex::new(vec![TEST_OPERATOR_TOKEN.to_owned()])),
            events: Arc::new(Mutex::new(Vec::new())),
            audit_events: Arc::new(Mutex::new(Vec::new())),
            postgres_ready: true,
            studio_connection: Arc::new(Mutex::new(None)),
        };
        let app = router_with_state(test_state(store));
        let response = admin_post(
            app,
            "/admin-ui/admin/providers",
            Some(TEST_OPERATOR_TOKEN),
            r#"{"provider":"litellm","name":"LiteLLM","base_url":"http://litellm:4000","credential":"sk-master","enabled":true}"#,
        )
        .await;

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let value: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(value["provider"], "litellm");
        assert_eq!(value["credential_configured"], true);
        assert!(value.get("credential").is_none());
    }

    #[tokio::test]
    async fn admin_list_keys_returns_database_backed_key_metadata() {
        let store = MemoryStore {
            key: Arc::new(Mutex::new(None)),
            admin_key: Arc::new(Mutex::new(None)),
            services: Arc::new(Mutex::new(Vec::new())),
            openai_routes: Arc::new(Mutex::new(default_openai_routes())),
            operator_tokens: Arc::new(Mutex::new(vec![TEST_OPERATOR_TOKEN.to_owned()])),
            events: Arc::new(Mutex::new(Vec::new())),
            audit_events: Arc::new(Mutex::new(Vec::new())),
            postgres_ready: true,
            studio_connection: Arc::new(Mutex::new(None)),
        };
        let app = router_with_state(test_state(store));
        let project_id = Uuid::new_v4();
        let response = admin_post(
            app.clone(),
            "/admin-ui/admin/keys",
            Some(TEST_OPERATOR_TOKEN),
            &format!(r#"{{"project_id":"{project_id}"}}"#),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);

        let response = admin_get(app, "/admin-ui/admin/keys", Some(TEST_OPERATOR_TOKEN)).await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let value: serde_json::Value = serde_json::from_slice(&body).expect("json");
        let keys = value.as_array().expect("keys");

        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0]["project_id"], project_id.to_string());
        assert!(keys[0]["key_hash"].is_null());
        assert!(keys[0]["raw_key"].is_null());
    }

    #[tokio::test]
    async fn admin_key_lifecycle_enable_disable_and_revoke_are_persisted() {
        let store = MemoryStore {
            key: Arc::new(Mutex::new(None)),
            admin_key: Arc::new(Mutex::new(None)),
            services: Arc::new(Mutex::new(Vec::new())),
            openai_routes: Arc::new(Mutex::new(default_openai_routes())),
            operator_tokens: Arc::new(Mutex::new(vec![TEST_OPERATOR_TOKEN.to_owned()])),
            events: Arc::new(Mutex::new(Vec::new())),
            audit_events: Arc::new(Mutex::new(Vec::new())),
            postgres_ready: true,
            studio_connection: Arc::new(Mutex::new(None)),
        };
        let app = router_with_state(test_state(store));
        let project_id = Uuid::new_v4();
        let response = admin_post(
            app.clone(),
            "/admin-ui/admin/keys",
            Some(TEST_OPERATOR_TOKEN),
            &format!(r#"{{"project_id":"{project_id}"}}"#),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let value: serde_json::Value = serde_json::from_slice(&body).expect("json");
        let key_id = value["key"]["id"].as_str().expect("key id");

        let response = admin_post(
            app.clone(),
            &format!("/admin-ui/admin/keys/{key_id}/disable"),
            Some(TEST_OPERATOR_TOKEN),
            "{}",
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let value: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(value["disabled"], true);

        let response = admin_post(
            app.clone(),
            &format!("/admin-ui/admin/keys/{key_id}/enable"),
            Some(TEST_OPERATOR_TOKEN),
            "{}",
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let value: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(value["disabled"], false);

        let response = admin_post(
            app.clone(),
            &format!("/admin-ui/admin/keys/{key_id}/revoke"),
            Some(TEST_OPERATOR_TOKEN),
            "{}",
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let value: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(value["disabled"], true);
        assert!(value["revoked_at"].as_str().is_some());

        let response = admin_post(
            app,
            &format!("/admin-ui/admin/keys/{key_id}/enable"),
            Some(TEST_OPERATOR_TOKEN),
            "{}",
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let value: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(value["disabled"], true);
        assert!(value["revoked_at"].as_str().is_some());
    }

    #[tokio::test]
    async fn metrics_endpoint_is_scrapeable_without_admin_token() {
        let store = MemoryStore {
            key: Arc::new(Mutex::new(None)),
            admin_key: Arc::new(Mutex::new(None)),
            services: Arc::new(Mutex::new(Vec::new())),
            openai_routes: Arc::new(Mutex::new(default_openai_routes())),
            operator_tokens: Arc::new(Mutex::new(vec![TEST_OPERATOR_TOKEN.to_owned()])),
            events: Arc::new(Mutex::new(Vec::new())),
            audit_events: Arc::new(Mutex::new(Vec::new())),
            postgres_ready: true,
            studio_connection: Arc::new(Mutex::new(None)),
        };
        let app = router_with_state(test_state(store));
        let response = request(app, "/admin-ui/metrics").await;

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn old_root_control_paths_are_not_registered() {
        let app = router_with_state(test_state(default_store()));

        for route in [
            "/healthz",
            "/readyz",
            "/metrics",
            "/admin/keys",
            "/v1/guardrails",
        ] {
            let response = request(app.clone(), route).await;
            assert_eq!(response.status(), StatusCode::NOT_FOUND, "{route}");
        }
    }

    #[tokio::test]
    async fn task_usage_requires_admin_token_and_returns_summary() {
        let store = MemoryStore {
            key: Arc::new(Mutex::new(None)),
            admin_key: Arc::new(Mutex::new(None)),
            services: Arc::new(Mutex::new(Vec::new())),
            openai_routes: Arc::new(Mutex::new(default_openai_routes())),
            operator_tokens: Arc::new(Mutex::new(vec![TEST_OPERATOR_TOKEN.to_owned()])),
            events: Arc::new(Mutex::new(Vec::new())),
            audit_events: Arc::new(Mutex::new(Vec::new())),
            postgres_ready: true,
            studio_connection: Arc::new(Mutex::new(None)),
        };
        let app = router_with_state(test_state(store));
        let response = admin_get(
            app,
            "/admin-ui/admin/tasks/task-1/usage",
            Some(TEST_OPERATOR_TOKEN),
        )
        .await;

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn usage_export_json_and_csv_filter_by_status() {
        let store = default_store();
        let key_id = Uuid::new_v4();
        let project_id = Uuid::new_v4();
        store.events.lock().expect("events lock").extend([
            UsageEvent {
                request_id: "req-success".to_owned(),
                key_id,
                project_id: Some(project_id),
                route: Route::ChatCompletions,
                model: Some("gpt-test".to_owned()),
                provider: gateway_core::Provider::LiteLlm,
                status: UsageStatus::Success,
                status_code: 200,
                latency_ms: 25,
                input_tokens: Some(3),
                output_tokens: Some(4),
                total_tokens: Some(7),
                estimated_cost_usd: Some(0.25),
                service_name: None,
                task_id: Some("task-1".to_owned()),
                run_id: Some("run-1".to_owned()),
                fallback_count: 1,
                created_at: Utc::now(),
            },
            UsageEvent {
                request_id: "=req-failure".to_owned(),
                key_id,
                project_id: Some(project_id),
                route: Route::ChatCompletions,
                model: Some("gpt-test".to_owned()),
                provider: gateway_core::Provider::LiteLlm,
                status: UsageStatus::Failure,
                status_code: 502,
                latency_ms: 50,
                input_tokens: None,
                output_tokens: None,
                total_tokens: None,
                estimated_cost_usd: None,
                service_name: None,
                task_id: Some("task-1".to_owned()),
                run_id: Some("run-2".to_owned()),
                fallback_count: 0,
                created_at: Utc::now(),
            },
        ]);
        let app = router_with_state(test_state(store));

        let response = admin_get(
            app.clone(),
            "/admin-ui/admin/usage/export.json?status=success",
            Some(TEST_OPERATOR_TOKEN),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let value: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(value["summary"]["request_count"], 1);
        assert_eq!(value["summary"]["estimated_cost_usd"], 0.25);
        assert_eq!(value["rows"][0]["request_id"], "req-success");

        let response = admin_get(
            app,
            "/admin-ui/admin/usage/export.csv?status=failure",
            Some(TEST_OPERATOR_TOKEN),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let csv = String::from_utf8(body.to_vec()).expect("csv");
        assert!(csv.starts_with("request_id,key_id,project_id"));
        assert!(csv.contains("'=req-failure"));
        assert!(!csv.contains("req-success"));
    }

    #[tokio::test]
    async fn admin_ui_assets_are_served_without_exposing_operator_token() {
        let store = MemoryStore {
            key: Arc::new(Mutex::new(None)),
            admin_key: Arc::new(Mutex::new(None)),
            services: Arc::new(Mutex::new(Vec::new())),
            openai_routes: Arc::new(Mutex::new(default_openai_routes())),
            operator_tokens: Arc::new(Mutex::new(vec![TEST_OPERATOR_TOKEN.to_owned()])),
            events: Arc::new(Mutex::new(Vec::new())),
            audit_events: Arc::new(Mutex::new(Vec::new())),
            postgres_ready: true,
            studio_connection: Arc::new(Mutex::new(None)),
        };
        let app = router_with_state(test_state(store));
        let response = request(app.clone(), "/admin-ui").await;

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let body = String::from_utf8(body.to_vec()).expect("utf8");
        assert!(body.contains("Relayna Gateway Admin"));
        assert!(!body.contains(TEST_OPERATOR_TOKEN));

        let response = request(app.clone(), "/admin-ui/app.js").await;
        assert_eq!(response.status(), StatusCode::OK);

        let response = request(app, "/admin-ui/app.css").await;
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn operator_token_rotation_returns_new_raw_token_once_and_invalidates_old_token() {
        let store = MemoryStore {
            key: Arc::new(Mutex::new(None)),
            admin_key: Arc::new(Mutex::new(None)),
            services: Arc::new(Mutex::new(Vec::new())),
            openai_routes: Arc::new(Mutex::new(default_openai_routes())),
            operator_tokens: Arc::new(Mutex::new(vec![TEST_OPERATOR_TOKEN.to_owned()])),
            events: Arc::new(Mutex::new(Vec::new())),
            audit_events: Arc::new(Mutex::new(Vec::new())),
            postgres_ready: true,
            studio_connection: Arc::new(Mutex::new(None)),
        };
        let app = router_with_state(test_state(store));
        let response = admin_post(
            app.clone(),
            "/admin-ui/admin/operator-token/rotate",
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
            "/admin-ui/admin/usage/summary",
            Some(TEST_OPERATOR_TOKEN),
        )
        .await;
        assert_eq!(old_response.status(), StatusCode::UNAUTHORIZED);
        let new_response = admin_get(app, "/admin-ui/admin/usage/summary", Some(new_token)).await;
        assert_eq!(new_response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn studio_connection_requires_operator_token() {
        let app = router_with_state(test_state(default_store()));
        let response = admin_get(app, "/admin-ui/admin/studio/connection", None).await;

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn studio_connection_patch_redacts_token_and_overrides_environment() {
        let app = router_with_state(test_state_with_studio_env(
            default_store(),
            StudioConnectionEnv {
                base_url: Some("http://env-studio.example".to_owned()),
                token: Some("env-token".to_owned()),
            },
        ));

        let response = admin_get(
            app.clone(),
            "/admin-ui/admin/studio/connection",
            Some(TEST_OPERATOR_TOKEN),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let value: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(value["source"], "environment");
        assert_eq!(value["base_url"], "http://env-studio.example");
        assert_eq!(value["token_configured"], true);
        assert!(value.get("token").is_none());

        let response = admin_patch(
            app.clone(),
            "/admin-ui/admin/studio/connection",
            Some(TEST_OPERATOR_TOKEN),
            r#"{"base_url":"http://persisted-studio.example/","token":"persisted-token"}"#,
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let value: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(value["source"], "persisted");
        assert_eq!(value["base_url"], "http://persisted-studio.example");
        assert_eq!(value["token_configured"], true);
        assert!(value.get("token").is_none());

        let response = admin_patch(
            app,
            "/admin-ui/admin/studio/connection",
            Some(TEST_OPERATOR_TOKEN),
            r#"{"base_url":null}"#,
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let value: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(value["source"], "environment");
        assert_eq!(value["base_url"], "http://env-studio.example");
        assert_eq!(value["token_configured"], true);
        assert!(value.get("token").is_none());
    }

    #[tokio::test]
    async fn studio_connection_rejects_invalid_base_url() {
        let app = router_with_state(test_state(default_store()));
        let response = admin_patch(
            app,
            "/admin-ui/admin/studio/connection",
            Some(TEST_OPERATOR_TOKEN),
            r#"{"base_url":"ftp://studio.example"}"#,
        )
        .await;

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let value: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(value["error"]["code"], "invalid_studio_connection_payload");
    }

    #[tokio::test]
    async fn studio_services_reports_missing_connection_config() {
        let app = router_with_state(test_state(default_store()));
        let response = admin_get(
            app,
            "/admin-ui/admin/studio/services",
            Some(TEST_OPERATOR_TOKEN),
        )
        .await;

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let value: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(value["error"]["code"], "invalid_configuration");
    }

    #[tokio::test]
    async fn openai_route_settings_can_be_listed_and_toggled() {
        let store = MemoryStore {
            key: Arc::new(Mutex::new(None)),
            admin_key: Arc::new(Mutex::new(None)),
            services: Arc::new(Mutex::new(Vec::new())),
            openai_routes: Arc::new(Mutex::new(default_openai_routes())),
            operator_tokens: Arc::new(Mutex::new(vec![TEST_OPERATOR_TOKEN.to_owned()])),
            events: Arc::new(Mutex::new(Vec::new())),
            audit_events: Arc::new(Mutex::new(Vec::new())),
            postgres_ready: true,
            studio_connection: Arc::new(Mutex::new(None)),
        };
        let app = router_with_state(test_state(store));

        let response = admin_get(
            app.clone(),
            "/admin-ui/admin/openai-routes",
            Some(TEST_OPERATOR_TOKEN),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let value: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(value.as_array().expect("routes").len(), 2);

        let response = admin_post(
            app.clone(),
            "/admin-ui/admin/openai-routes/chat-completions/disable",
            Some(TEST_OPERATOR_TOKEN),
            "{}",
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let value: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(value["route_id"], "chat-completions");
        assert_eq!(value["enabled"], false);

        let response = admin_post(
            app,
            "/admin-ui/admin/openai-routes/chat-completions/enable",
            Some(TEST_OPERATOR_TOKEN),
            "{}",
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let value: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(value["enabled"], true);
    }

    #[tokio::test]
    async fn admin_service_create_redacts_raw_credential() {
        let store = MemoryStore {
            key: Arc::new(Mutex::new(None)),
            admin_key: Arc::new(Mutex::new(None)),
            services: Arc::new(Mutex::new(Vec::new())),
            openai_routes: Arc::new(Mutex::new(default_openai_routes())),
            operator_tokens: Arc::new(Mutex::new(vec![TEST_OPERATOR_TOKEN.to_owned()])),
            events: Arc::new(Mutex::new(Vec::new())),
            audit_events: Arc::new(Mutex::new(Vec::new())),
            postgres_ready: true,
            studio_connection: Arc::new(Mutex::new(None)),
        };
        let app = router_with_state(test_state(store));
        let response = admin_post(
            app,
            "/admin-ui/admin/services",
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
            openai_routes: Arc::new(Mutex::new(default_openai_routes())),
            operator_tokens: Arc::new(Mutex::new(vec![TEST_OPERATOR_TOKEN.to_owned()])),
            events: Arc::new(Mutex::new(Vec::new())),
            audit_events: Arc::new(Mutex::new(Vec::new())),
            postgres_ready: true,
            studio_connection: Arc::new(Mutex::new(None)),
        };
        let app = router_with_state(test_state(store));
        let response = admin_post(
            app.clone(),
            "/admin-ui/admin/services/import",
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
            "/admin-ui/admin/services/translation/sync-status",
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
    async fn admin_service_reimport_preserves_gateway_owned_runtime_fields() {
        let store = MemoryStore {
            key: Arc::new(Mutex::new(None)),
            admin_key: Arc::new(Mutex::new(None)),
            services: Arc::new(Mutex::new(Vec::new())),
            openai_routes: Arc::new(Mutex::new(default_openai_routes())),
            operator_tokens: Arc::new(Mutex::new(vec![TEST_OPERATOR_TOKEN.to_owned()])),
            events: Arc::new(Mutex::new(Vec::new())),
            audit_events: Arc::new(Mutex::new(Vec::new())),
            postgres_ready: true,
            studio_connection: Arc::new(Mutex::new(None)),
        };
        let app = router_with_state(test_state(store));
        let response = admin_post(
            app.clone(),
            "/admin-ui/admin/services/import",
            Some(TEST_OPERATOR_TOKEN),
            r#"{
                "studio_service_id":"svc_1",
                "name":"translation",
                "route_pattern":"/services/translation/*",
                "upstream_base_url":"http://studio-suggested.internal:8080",
                "allowed_methods":["POST"]
            }"#,
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);

        let response = admin_patch(
            app.clone(),
            "/admin-ui/admin/services/translation",
            Some(TEST_OPERATOR_TOKEN),
            r#"{
                "route_pattern":"/services/local-translation/*",
                "upstream_base_url":"http://gateway-owned.internal:8080",
                "credential":"token",
                "enabled":true,
                "allowed_methods":["POST"]
            }"#,
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);

        let response = admin_post(
            app,
            "/admin-ui/admin/services/import",
            Some(TEST_OPERATOR_TOKEN),
            r#"{
                "studio_service_id":"svc_1",
                "name":"translation",
                "route_pattern":"/services/studio-updated/*",
                "upstream_base_url":"http://studio-updated.internal:8080",
                "allowed_methods":["GET"]
            }"#,
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let value: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(value["route_pattern"], "/services/local-translation/*");
        assert_eq!(
            value["upstream_base_url"],
            "http://gateway-owned.internal:8080"
        );
        assert_eq!(value["allowed_methods"][0], "POST");
        assert_eq!(value["enabled"], true);
        assert_eq!(value["sync_status"], "synced");
    }

    #[tokio::test]
    async fn admin_service_patch_can_configure_imported_service() {
        let store = MemoryStore {
            key: Arc::new(Mutex::new(None)),
            admin_key: Arc::new(Mutex::new(None)),
            services: Arc::new(Mutex::new(Vec::new())),
            openai_routes: Arc::new(Mutex::new(default_openai_routes())),
            operator_tokens: Arc::new(Mutex::new(vec![TEST_OPERATOR_TOKEN.to_owned()])),
            events: Arc::new(Mutex::new(Vec::new())),
            audit_events: Arc::new(Mutex::new(Vec::new())),
            postgres_ready: true,
            studio_connection: Arc::new(Mutex::new(None)),
        };
        let app = router_with_state(test_state(store));
        let _ = admin_post(
            app.clone(),
            "/admin-ui/admin/services/import",
            Some(TEST_OPERATOR_TOKEN),
            r#"{"studio_service_id":"svc_1","name":"translation","route_pattern":"/translation"}"#,
        )
        .await;
        let response = admin_patch(
            app,
            "/admin-ui/admin/services/translation",
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
