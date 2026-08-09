use crate::portal::{
    constant_time_eq, pkce_challenge, random_opaque_token, safe_return_to, token_hash,
    PortalOidcRuntime,
};
use async_trait::async_trait;
use axum::{
    body::Body,
    extract::{OriginalUri, Path, Query, State},
    http::{header, HeaderMap, HeaderName, HeaderValue, Method, StatusCode},
    response::{IntoResponse, Redirect, Response},
    routing::{any, delete, get, patch, post},
    Json, Router,
};
use bytes::Bytes;
use chrono::{Duration as ChronoDuration, Utc};
use gateway_core::CircuitBreakerState;
use gateway_core::EntraJwtVerifier;
use gateway_core::{
    auth::{Authenticator, VirtualKeyLookup},
    default_operator_scopes, evaluate_policy, evaluate_policy_limits, extract_generation_features,
    guardrail_executor_for_definitions, is_relayna_default_endpoint, merge_endpoint_pricing_rules,
    resolve_guardrail_plan, validate_openapi_endpoints, validate_openapi_source_path,
    AdminAuditStore, AdminGatewayAuthSettingsStore, AdminGuardrailDefinitionResponse,
    AdminKeyCreate, AdminKeyPatch, AdminKeyResponse, AdminKeyStore, AdminOpenAiRouteStore,
    AdminPolicyLayerStore, AdminPolicyLayerUpsert, AdminProjectStore, AdminProviderConfigStore,
    AdminServiceStore, AdminStudioConnectionStore, AuditEvent, AuditEventCreate, AuditEventQuery,
    CreatedAdminKeyResponse, CreatedOperatorTokenResponse, CredentialHeaderMode,
    CredentialHeaderValueFormat, EffectiveGatewayAuthSettings, EffectiveStudioConnection,
    GatewayAuthEnv, GatewayAuthSettingsPatchRequest, GatewayError, GatewayResult,
    GuardrailAdminCreateRequest, GuardrailAdminPatchRequest, GuardrailDefinitionResponse,
    GuardrailEventQuery, GuardrailExecutionEvent, GuardrailExecutionSummary, GuardrailMode,
    GuardrailObservabilityStore, GuardrailPlanRequest, GuardrailPolicySet, GuardrailStore,
    GuardrailTestRequest, GuardrailTestResponse, KeyPolicy, LiteLlmCredentialMappingResponse,
    LiteLlmCredentialMappingUpsertRequest, LiteLlmPassthroughSettingsPatchRequest,
    ManagedIdentityCreateRequest, ManagedIdentityPatchRequest, MemberPatchRequest, MemberStatus,
    NewPortalSession, OidcLoginTransaction, OpenAiRouteConfigPatchRequest, OpenAiRouteMode,
    OperatorAuthorization, OperatorTokenMaterial, OperatorTokenStore, PolicyLookup,
    PortalAccessStore, PortalMember, ProjectCreateRequest, ProjectPatchRequest, ProjectResponse,
    Provider, ProviderConfigCreateRequest, ProviderConfigLookup, ProviderConfigPatchRequest,
    ProviderConfigResponse, ProviderHealthState, ProviderHealthStatus, ProviderIntelligenceStore,
    Route, ServiceCreateRequest, ServiceImportDiff, ServiceImportValidationIssue,
    ServiceMemberRole, ServiceMembership, ServiceMembershipUpsertRequest, ServiceOpenApiEndpoint,
    ServiceOpenApiPreview, ServiceOpenApiPreviewRequest, ServiceOpenApiSyncRequest,
    ServicePatchRequest, ServiceRegistrySnapshot, ServiceResponse, SharedGatewayAuthRuntime,
    StudioConnectionEnv, StudioConnectionPatchRequest, StudioConnectionTestResponse,
    StudioServiceCatalogResponse, StudioServiceImportPreview, StudioServiceImportRequest,
    UsageBreakdownDimension, UsageEvent, UsageExport, UsageFilterValuesQuery, UsageQuery,
    UsageQueryStore, VirtualKeyMaterial, SCOPE_AUDIT_READ, SCOPE_GUARDRAILS_UPDATE,
    SCOPE_KEYS_CREATE, SCOPE_KEYS_DISABLE, SCOPE_OPERATORS_MANAGE, SCOPE_POLICIES_UPDATE,
    SCOPE_PROVIDERS_UPDATE, SCOPE_SERVICES_UPDATE, SCOPE_SETTINGS_UPDATE, SCOPE_USAGE_EXPORT,
    SCOPE_USAGE_READ,
};
use gateway_store::{PostgresStore, RedisReadiness};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
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
    + ProviderConfigLookup
    + AdminServiceStore
    + AdminStudioConnectionStore
    + AdminGatewayAuthSettingsStore
    + GuardrailStore
    + GuardrailObservabilityStore
    + ProviderIntelligenceStore
    + OperatorTokenStore
    + AdminAuditStore
    + UsageQueryStore
    + PortalAccessStore
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
    auth_env: GatewayAuthEnv,
    auth_runtime: SharedGatewayAuthRuntime,
    litellm_base_url: String,
    litellm_service_key: String,
    litellm_ui_client: reqwest::Client,
    portal_oidc: Option<Arc<PortalOidcRuntime>>,
    owner_entra_verifier: Option<Arc<EntraJwtVerifier>>,
}

const STUDIO_CATALOG_TIMEOUT: Duration = Duration::from_secs(8);
const SERVICE_OPENAPI_TIMEOUT: Duration = Duration::from_secs(8);
const SERVICE_OPENAPI_MAX_BYTES: usize = 1024 * 1024;
const DEFAULT_LITELLM_BASE_URL: &str = "http://127.0.0.1:4000";
const LITELLM_UI_HTML_REWRITE_LIMIT: usize = 2 * 1024 * 1024;
const LITELLM_UI_PROXY_PREFIX: &str = "/admin-ui/litellm-ui";
const LITELLM_UI_OPERATOR_COOKIE: &str = "relayna_litellm_ui_operator";
const LITELLM_UI_OPERATOR_COOKIE_MAX_AGE_SECONDS: u64 = 60 * 60;
const PORTAL_SESSION_COOKIE: &str = "relayna_portal_session";
const PORTAL_CSRF_COOKIE: &str = "relayna_portal_csrf";
const PORTAL_LOGIN_COOKIE: &str = "relayna_portal_login";
const LITELLM_UI_ROOT_PROXY_PREFIXES: &[&str] = &[
    "v1/agents",
    "v2/",
    "v3/",
    "get/",
    "get_image",
    "public/",
    "config/",
    "health/",
    "in_product_nudges",
    "key/",
    "model/",
    "model_group/",
    "models",
    "models/",
    "organization/",
    "policies/",
    "project/",
    "prompts/",
    "sso/",
    "tag/",
    "team/",
    "user/",
];

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

fn litellm_ui_client() -> reqwest::Client {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("LiteLLM UI proxy client")
}

pub fn router(store: PostgresStore, redis: RedisReadiness) -> Router {
    let auth_env = GatewayAuthEnv::default();
    let auth_runtime = SharedGatewayAuthRuntime::new(
        EffectiveGatewayAuthSettings::from_sources(None, &auth_env)
            .expect("default auth settings")
            .runtime_config(),
    )
    .expect("default auth runtime");
    router_with_state(AppState {
        store: Arc::new(store),
        redis,
        studio_env: StudioConnectionEnv::default(),
        auth_env,
        auth_runtime,
        litellm_base_url: DEFAULT_LITELLM_BASE_URL.to_owned(),
        litellm_service_key: String::new(),
        litellm_ui_client: litellm_ui_client(),
        portal_oidc: None,
        owner_entra_verifier: None,
    })
}

pub fn router_with_studio(
    store: PostgresStore,
    redis: RedisReadiness,
    studio: Option<StudioCatalogClient>,
) -> Router {
    let auth_env = GatewayAuthEnv::default();
    let auth_runtime = SharedGatewayAuthRuntime::new(
        EffectiveGatewayAuthSettings::from_sources(None, &auth_env)
            .expect("default auth settings")
            .runtime_config(),
    )
    .expect("default auth runtime");
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
        auth_env,
        auth_runtime,
        litellm_base_url: DEFAULT_LITELLM_BASE_URL.to_owned(),
        litellm_service_key: String::new(),
        litellm_ui_client: litellm_ui_client(),
        portal_oidc: None,
        owner_entra_verifier: None,
    })
}

pub fn router_with_studio_and_auth(
    store: PostgresStore,
    redis: RedisReadiness,
    studio: Option<StudioCatalogClient>,
    auth_env: GatewayAuthEnv,
    auth_runtime: SharedGatewayAuthRuntime,
) -> Router {
    router_with_studio_auth_and_litellm(
        store,
        redis,
        studio,
        auth_env,
        auth_runtime,
        DEFAULT_LITELLM_BASE_URL.to_owned(),
        String::new(),
    )
}

pub fn router_with_studio_auth_and_litellm(
    store: PostgresStore,
    redis: RedisReadiness,
    studio: Option<StudioCatalogClient>,
    auth_env: GatewayAuthEnv,
    auth_runtime: SharedGatewayAuthRuntime,
    litellm_base_url: String,
    litellm_service_key: String,
) -> Router {
    router_with_identity(
        store,
        redis,
        studio,
        auth_env,
        auth_runtime,
        litellm_base_url,
        litellm_service_key,
        None,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn router_with_identity(
    store: PostgresStore,
    redis: RedisReadiness,
    studio: Option<StudioCatalogClient>,
    auth_env: GatewayAuthEnv,
    auth_runtime: SharedGatewayAuthRuntime,
    litellm_base_url: String,
    litellm_service_key: String,
    portal_oidc: Option<Arc<PortalOidcRuntime>>,
    owner_entra_verifier: Option<Arc<EntraJwtVerifier>>,
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
        auth_env,
        auth_runtime,
        litellm_base_url,
        litellm_service_key,
        litellm_ui_client: litellm_ui_client(),
        portal_oidc,
        owner_entra_verifier,
    })
}

pub fn router_with_state(state: AppState) -> Router {
    Router::new()
        .route("/admin-ui/healthz", get(healthz))
        .route("/admin-ui/readyz", get(readyz))
        .route("/admin-ui/auth/config", get(portal_auth_config))
        .route("/admin-ui/auth/login", get(portal_auth_login))
        .route("/admin-ui/auth/callback", get(portal_auth_callback))
        .route("/admin-ui/auth/session", get(portal_auth_session))
        .route("/admin-ui/auth/logout", post(portal_auth_logout))
        .route("/admin-ui/admin/members", get(admin_members))
        .route(
            "/admin-ui/admin/members/{member_id}",
            patch(admin_patch_member),
        )
        .route(
            "/admin-ui/admin/members/{member_id}/services/{service_name}",
            axum::routing::put(admin_upsert_service_membership)
                .delete(admin_delete_service_membership),
        )
        .route(
            "/admin-ui/admin/managed-identities",
            get(admin_managed_identities).post(admin_create_managed_identity),
        )
        .route(
            "/admin-ui/admin/managed-identities/{identity_id}",
            patch(admin_patch_managed_identity).delete(admin_delete_managed_identity),
        )
        .route("/owner/v1/services", get(owner_services))
        .route("/owner/v1/services/{service_name}", get(owner_service))
        .route(
            "/owner/v1/services/{service_name}/dashboard",
            get(owner_service_dashboard),
        )
        .route(
            "/owner/v1/services/{service_name}/events",
            get(owner_service_events),
        )
        .route(
            "/owner/v1/services/{service_name}/errors",
            get(owner_service_errors),
        )
        .route(
            "/owner/v1/services/{service_name}/logs",
            get(owner_service_logs),
        )
        .route(
            "/owner/v1/services/{service_name}/endpoints",
            get(owner_service_endpoints),
        )
        .route(
            "/owner/v1/services/{service_name}/export.json",
            get(owner_service_export_json),
        )
        .route(
            "/owner/v1/services/{service_name}/export.csv",
            get(owner_service_export_csv),
        )
        .route("/admin-ui/litellm-ui", any(litellm_ui_proxy_root))
        .route("/admin-ui/litellm-ui/", any(litellm_ui_proxy_root))
        .route(
            "/admin-ui/litellm-ui/litellm/.well-known/litellm-ui-config",
            any(litellm_ui_proxy_litellm_config),
        )
        .route(
            "/admin-ui/litellm-ui/litellm/{*path}",
            any(litellm_ui_proxy_litellm_prefix),
        )
        .route("/admin-ui/litellm-ui/{*path}", any(litellm_ui_proxy))
        .route(
            "/litellm-asset-prefix/{*path}",
            any(litellm_ui_proxy_root_emitted_path),
        )
        .route(
            "/litellm/.well-known/litellm-ui-config",
            any(litellm_ui_proxy_root_emitted_path),
        )
        .route("/litellm/{*path}", any(litellm_ui_proxy_root_emitted_path))
        .route("/v1/agents", any(litellm_ui_proxy_root_emitted_path))
        .route("/v2/{*path}", any(litellm_ui_proxy_root_emitted_path))
        .route("/v3/{*path}", any(litellm_ui_proxy_root_emitted_path))
        .route("/get_image", any(litellm_ui_proxy_root_emitted_path))
        .route("/get/{*path}", any(litellm_ui_proxy_root_emitted_path))
        .route("/public/{*path}", any(litellm_ui_proxy_root_emitted_path))
        .route("/config/{*path}", any(litellm_ui_proxy_root_emitted_path))
        .route("/health/{*path}", any(litellm_ui_proxy_root_emitted_path))
        .route("/key/{*path}", any(litellm_ui_proxy_root_emitted_path))
        .route(
            "/in_product_nudges",
            any(litellm_ui_proxy_root_emitted_path),
        )
        .route("/model/{*path}", any(litellm_ui_proxy_root_emitted_path))
        .route(
            "/model_group/{*path}",
            any(litellm_ui_proxy_root_emitted_path),
        )
        .route("/models", any(litellm_ui_proxy_root_emitted_path))
        .route("/models/{*path}", any(litellm_ui_proxy_root_emitted_path))
        .route(
            "/organization/{*path}",
            any(litellm_ui_proxy_root_emitted_path),
        )
        .route("/policies/{*path}", any(litellm_ui_proxy_root_emitted_path))
        .route("/project/{*path}", any(litellm_ui_proxy_root_emitted_path))
        .route("/prompts/{*path}", any(litellm_ui_proxy_root_emitted_path))
        .route("/sso/{*path}", any(litellm_ui_proxy_root_emitted_path))
        .route("/tag/{*path}", any(litellm_ui_proxy_root_emitted_path))
        .route("/team/{*path}", any(litellm_ui_proxy_root_emitted_path))
        .route("/user/{*path}", any(litellm_ui_proxy_root_emitted_path))
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
            "/admin-ui/admin/providers/litellm-credentials",
            post(upsert_litellm_credential_mapping).get(list_litellm_credential_mappings),
        )
        .route(
            "/admin-ui/admin/providers/litellm-credentials/{mapping_id}",
            delete(delete_litellm_credential_mapping),
        )
        .route(
            "/admin-ui/admin/providers/litellm-credentials/{mapping_id}/disable",
            post(disable_litellm_credential_mapping),
        )
        .route(
            "/admin-ui/admin/providers/litellm-credentials/{mapping_id}/enable",
            post(enable_litellm_credential_mapping),
        )
        .route(
            "/admin-ui/admin/providers/litellm-passthrough",
            get(get_litellm_passthrough_settings).patch(patch_litellm_passthrough_settings),
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
            "/admin-ui/admin/anthropic-routes",
            get(list_anthropic_routes),
        )
        .route(
            "/admin-ui/admin/openai-routes/{route_id}/disable",
            post(disable_openai_route),
        )
        .route(
            "/admin-ui/admin/openai-routes/{route_id}/enable",
            post(enable_openai_route),
        )
        .route(
            "/admin-ui/admin/openai-routes/{route_id}/mode",
            patch(patch_openai_route_mode),
        )
        .route(
            "/admin-ui/admin/openai-routes/{route_id}/config",
            patch(patch_openai_route_config),
        )
        .route(
            "/admin-ui/admin/anthropic-routes/{route_id}/disable",
            post(disable_anthropic_route),
        )
        .route(
            "/admin-ui/admin/anthropic-routes/{route_id}/enable",
            post(enable_anthropic_route),
        )
        .route(
            "/admin-ui/admin/anthropic-routes/{route_id}/mode",
            patch(patch_anthropic_route_mode),
        )
        .route(
            "/admin-ui/admin/anthropic-routes/{route_id}/config",
            patch(patch_anthropic_route_config),
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
            "/admin-ui/admin/auth/front-door",
            get(get_gateway_auth_settings).patch(patch_gateway_auth_settings),
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
            "/admin-ui/admin/services/{service_name}/openapi/preview",
            post(preview_service_openapi),
        )
        .route(
            "/admin-ui/admin/services/{service_name}/openapi/sync",
            post(sync_service_openapi),
        )
        .route(
            "/admin-ui/admin/projects/{project_id}/usage",
            get(project_usage),
        )
        .route("/admin-ui/admin/usage/summary", get(usage_summary))
        .route("/admin-ui/admin/usage/dashboard", get(usage_dashboard))
        .route("/admin-ui/admin/usage/timeseries", get(usage_timeseries))
        .route("/admin-ui/admin/usage/by-key", get(usage_by_key))
        .route("/admin-ui/admin/usage/by-project", get(usage_by_project))
        .route("/admin-ui/admin/usage/by-model", get(usage_by_model))
        .route("/admin-ui/admin/usage/by-provider", get(usage_by_provider))
        .route("/admin-ui/admin/usage/by-service", get(usage_by_service))
        .route("/admin-ui/admin/usage/by-task", get(usage_by_task))
        .route("/admin-ui/admin/usage/events", get(usage_events))
        .route(
            "/admin-ui/admin/usage/filter-values",
            get(usage_filter_values),
        )
        .route("/admin-ui/admin/usage/unused-keys", get(usage_unused_keys))
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

#[derive(Debug, Serialize)]
struct PortalAuthConfigBody {
    enabled: bool,
    login_url: &'static str,
    break_glass_available: bool,
}

async fn portal_auth_config(State(state): State<AppState>) -> Json<PortalAuthConfigBody> {
    Json(PortalAuthConfigBody {
        enabled: state.portal_oidc.is_some(),
        login_url: "/admin-ui/auth/login",
        break_glass_available: true,
    })
}

#[derive(Debug, Deserialize)]
struct PortalLoginQuery {
    return_to: Option<String>,
}

async fn portal_auth_login(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<PortalLoginQuery>,
) -> Response {
    let Some(oidc) = state.portal_oidc.as_ref() else {
        return error_response(&headers, GatewayError::OidcUnavailable);
    };
    let raw_state = random_opaque_token();
    let raw_binding = random_opaque_token();
    let nonce = random_opaque_token();
    let verifier = random_opaque_token();
    let authorization_url = match oidc
        .authorization_url(&raw_state, &nonce, &pkce_challenge(&verifier))
        .await
    {
        Ok(url) => url,
        Err(error) => return error_response(&headers, error),
    };
    let transaction = OidcLoginTransaction {
        state_hash: token_hash(&raw_state),
        binding_hash: token_hash(&raw_binding),
        nonce: nonce.clone(),
        pkce_verifier: verifier.clone(),
        return_to: safe_return_to(query.return_to.as_deref()),
        expires_at: Utc::now() + ChronoDuration::seconds(oidc.config.login_ttl_seconds),
    };
    if let Err(error) = state.store.create_oidc_login_transaction(transaction).await {
        return error_response(&headers, error);
    }
    let mut response = Redirect::temporary(&authorization_url).into_response();
    append_portal_login_cookie(
        response.headers_mut(),
        &raw_binding,
        oidc.config.login_ttl_seconds,
        oidc.config.cookie_secure,
    );
    response
}

#[derive(Debug, Deserialize)]
struct PortalCallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

async fn portal_auth_callback(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<PortalCallbackQuery>,
) -> Response {
    let Some(oidc) = state.portal_oidc.as_ref() else {
        return error_response(&headers, GatewayError::OidcUnavailable);
    };
    if query.error.is_some() {
        return error_response(&headers, GatewayError::InvalidOidcTransaction);
    }
    let (Some(code), Some(raw_state)) = (query.code.as_deref(), query.state.as_deref()) else {
        return error_response(&headers, GatewayError::InvalidOidcTransaction);
    };
    let Some(raw_binding) = cookie_value(&headers, PORTAL_LOGIN_COOKIE) else {
        return error_response(&headers, GatewayError::InvalidOidcTransaction);
    };
    let transaction = match state
        .store
        .consume_oidc_login_transaction(
            &token_hash(raw_state),
            &token_hash(raw_binding),
            Utc::now(),
        )
        .await
    {
        Ok(Some(transaction)) => transaction,
        Ok(None) => return error_response(&headers, GatewayError::InvalidOidcTransaction),
        Err(error) => return error_response(&headers, error),
    };
    let identity = match oidc
        .exchange_code(code, &transaction.pkce_verifier, Utc::now())
        .await
    {
        Ok(identity) => identity,
        Err(error) => return error_response(&headers, error),
    };
    if identity.nonce.as_deref() != Some(transaction.nonce.as_str()) {
        return error_response(&headers, GatewayError::InvalidOidcTransaction);
    }
    let Some(object_id) = identity
        .object_id
        .as_deref()
        .or(identity.subject.as_deref())
    else {
        return error_response(&headers, GatewayError::InvalidEntraToken);
    };
    let member = match state
        .store
        .upsert_oidc_member(
            &identity.tenant_id,
            object_id,
            identity.email.as_deref(),
            identity.display_name.as_deref(),
            Utc::now(),
        )
        .await
    {
        Ok(member) => member,
        Err(error) => return error_response(&headers, error),
    };
    let raw_session = random_opaque_token();
    let raw_csrf = random_opaque_token();
    if let Err(error) = state
        .store
        .create_portal_session(NewPortalSession {
            session_hash: token_hash(&raw_session),
            member_id: member.id,
            csrf_hash: token_hash(&raw_csrf),
            expires_at: Utc::now() + ChronoDuration::seconds(oidc.config.session_ttl_seconds),
        })
        .await
    {
        return error_response(&headers, error);
    }
    let mut response = Redirect::to(&transaction.return_to).into_response();
    append_portal_cookies(
        response.headers_mut(),
        &raw_session,
        &raw_csrf,
        oidc.config.session_ttl_seconds,
        oidc.config.cookie_secure,
    );
    clear_portal_login_cookie(response.headers_mut(), oidc.config.cookie_secure);
    response
}

#[derive(Debug, Serialize)]
struct PortalSessionBody {
    authenticated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    member: Option<PortalMember>,
    service_memberships: Vec<ServiceMembership>,
    #[serde(skip_serializing_if = "Option::is_none")]
    csrf_token: Option<String>,
}

async fn portal_auth_session(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let Some(raw_session) = cookie_value(&headers, PORTAL_SESSION_COOKIE) else {
        return Json(PortalSessionBody {
            authenticated: false,
            member: None,
            service_memberships: Vec::new(),
            csrf_token: None,
        })
        .into_response();
    };
    let Some(raw_csrf) = cookie_value(&headers, PORTAL_CSRF_COOKIE) else {
        return error_response(&headers, GatewayError::InvalidPortalSession);
    };
    let session = match state
        .store
        .resolve_portal_session(&token_hash(raw_session), Utc::now())
        .await
    {
        Ok(Some(session)) if constant_time_eq(&session.csrf_hash, &token_hash(raw_csrf)) => session,
        Ok(_) => return error_response(&headers, GatewayError::InvalidPortalSession),
        Err(error) => return error_response(&headers, error),
    };
    match state
        .store
        .list_service_memberships(session.member.id)
        .await
    {
        Ok(service_memberships) => Json(PortalSessionBody {
            authenticated: true,
            member: Some(session.member),
            service_memberships,
            csrf_token: Some(raw_csrf.to_owned()),
        })
        .into_response(),
        Err(error) => error_response(&headers, error),
    }
}

async fn portal_auth_logout(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let session = match require_portal_session(&state, &headers, true).await {
        Ok(session) => session,
        Err(response) => return response,
    };
    if let Err(error) = state
        .store
        .delete_portal_session(&session.session_hash)
        .await
    {
        return error_response(&headers, error);
    }
    let (secure, logout_url) = match state.portal_oidc.as_ref() {
        Some(oidc) => (oidc.config.cookie_secure, oidc.end_session_url().await.ok()),
        None => (true, None),
    };
    let mut response = Json(serde_json::json!({ "logout_url": logout_url })).into_response();
    clear_portal_cookies(response.headers_mut(), secure);
    clear_portal_login_cookie(response.headers_mut(), secure);
    response
}

#[derive(Debug, Serialize)]
struct MemberAccessBody {
    member: PortalMember,
    service_memberships: Vec<ServiceMembership>,
}

async fn admin_members(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(response) = require_admin_scope(&state, &headers, SCOPE_OPERATORS_MANAGE).await {
        return response;
    }
    let members = match state.store.list_members().await {
        Ok(members) => members,
        Err(error) => return error_response(&headers, error),
    };
    let mut response = Vec::with_capacity(members.len());
    for member in members {
        let service_memberships = match state.store.list_service_memberships(member.id).await {
            Ok(memberships) => memberships,
            Err(error) => return error_response(&headers, error),
        };
        response.push(MemberAccessBody {
            member,
            service_memberships,
        });
    }
    Json(response).into_response()
}

async fn admin_patch_member(
    State(state): State<AppState>,
    Path(member_id): Path<uuid::Uuid>,
    headers: HeaderMap,
    Json(patch): Json<MemberPatchRequest>,
) -> Response {
    let actor = match require_admin_scope(&state, &headers, SCOPE_OPERATORS_MANAGE).await {
        Ok(actor) => actor,
        Err(response) => return response,
    };
    let before = match state.store.get_member(member_id).await {
        Ok(before) => before,
        Err(error) => return error_response(&headers, error),
    };
    match state.store.patch_member(member_id, patch).await {
        Ok(Some(member)) => {
            if let Err(error) = record_admin_audit(
                &state,
                &headers,
                &actor,
                "members:update",
                "portal_member",
                Some(member_id.to_string()),
                before.as_ref().and_then(audit_json),
                audit_json(&member),
            )
            .await
            {
                return error_response(&headers, error);
            }
            Json(member).into_response()
        }
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(error) => error_response(&headers, error),
    }
}

#[derive(Debug, Deserialize)]
struct ServiceMembershipRoleBody {
    role: ServiceMemberRole,
}

async fn admin_upsert_service_membership(
    State(state): State<AppState>,
    Path((member_id, service_name)): Path<(uuid::Uuid, String)>,
    headers: HeaderMap,
    Json(body): Json<ServiceMembershipRoleBody>,
) -> Response {
    let actor = match require_admin_scope(&state, &headers, SCOPE_OPERATORS_MANAGE).await {
        Ok(actor) => actor,
        Err(response) => return response,
    };
    match state
        .store
        .upsert_service_membership(
            member_id,
            ServiceMembershipUpsertRequest {
                service_name: service_name.clone(),
                role: body.role,
            },
        )
        .await
    {
        Ok(membership) => {
            if let Err(error) = record_admin_audit(
                &state,
                &headers,
                &actor,
                "memberships:upsert",
                "service_membership",
                Some(format!("{member_id}:{service_name}")),
                None,
                audit_json(&membership),
            )
            .await
            {
                return error_response(&headers, error);
            }
            Json(membership).into_response()
        }
        Err(error) => error_response(&headers, error),
    }
}

async fn admin_delete_service_membership(
    State(state): State<AppState>,
    Path((member_id, service_name)): Path<(uuid::Uuid, String)>,
    headers: HeaderMap,
) -> Response {
    let actor = match require_admin_scope(&state, &headers, SCOPE_OPERATORS_MANAGE).await {
        Ok(actor) => actor,
        Err(response) => return response,
    };
    match state
        .store
        .delete_service_membership(member_id, &service_name)
        .await
    {
        Ok(true) => {
            if let Err(error) = record_admin_audit(
                &state,
                &headers,
                &actor,
                "memberships:delete",
                "service_membership",
                Some(format!("{member_id}:{service_name}")),
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

async fn admin_managed_identities(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(response) = require_admin_scope(&state, &headers, SCOPE_OPERATORS_MANAGE).await {
        return response;
    }
    match state.store.list_managed_identities().await {
        Ok(identities) => Json(identities).into_response(),
        Err(error) => error_response(&headers, error),
    }
}

async fn admin_create_managed_identity(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<ManagedIdentityCreateRequest>,
) -> Response {
    let actor = match require_admin_scope(&state, &headers, SCOPE_OPERATORS_MANAGE).await {
        Ok(actor) => actor,
        Err(response) => return response,
    };
    match state.store.create_managed_identity(request).await {
        Ok(identity) => {
            if let Err(error) = record_admin_audit(
                &state,
                &headers,
                &actor,
                "managed_identities:create",
                "managed_identity_binding",
                Some(identity.id.to_string()),
                None,
                audit_json(&identity),
            )
            .await
            {
                return error_response(&headers, error);
            }
            (StatusCode::CREATED, Json(identity)).into_response()
        }
        Err(error) => error_response(&headers, error),
    }
}

async fn admin_patch_managed_identity(
    State(state): State<AppState>,
    Path(identity_id): Path<uuid::Uuid>,
    headers: HeaderMap,
    Json(patch): Json<ManagedIdentityPatchRequest>,
) -> Response {
    let actor = match require_admin_scope(&state, &headers, SCOPE_OPERATORS_MANAGE).await {
        Ok(actor) => actor,
        Err(response) => return response,
    };
    match state.store.patch_managed_identity(identity_id, patch).await {
        Ok(Some(identity)) => {
            if let Err(error) = record_admin_audit(
                &state,
                &headers,
                &actor,
                "managed_identities:update",
                "managed_identity_binding",
                Some(identity_id.to_string()),
                None,
                audit_json(&identity),
            )
            .await
            {
                return error_response(&headers, error);
            }
            Json(identity).into_response()
        }
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(error) => error_response(&headers, error),
    }
}

async fn admin_delete_managed_identity(
    State(state): State<AppState>,
    Path(identity_id): Path<uuid::Uuid>,
    headers: HeaderMap,
) -> Response {
    let actor = match require_admin_scope(&state, &headers, SCOPE_OPERATORS_MANAGE).await {
        Ok(actor) => actor,
        Err(response) => return response,
    };
    match state.store.delete_managed_identity(identity_id).await {
        Ok(true) => {
            if let Err(error) = record_admin_audit(
                &state,
                &headers,
                &actor,
                "managed_identities:delete",
                "managed_identity_binding",
                Some(identity_id.to_string()),
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

#[derive(Debug, Serialize)]
struct OwnerServiceBody {
    service: ServiceResponse,
    role: ServiceMemberRole,
}

async fn owner_services(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let session = match require_active_portal_session(&state, &headers).await {
        Ok(session) => session,
        Err(response) => return response,
    };
    let memberships = match state
        .store
        .list_service_memberships(session.member.id)
        .await
    {
        Ok(memberships) => memberships,
        Err(error) => return error_response(&headers, error),
    };
    let mut services = Vec::new();
    for membership in memberships {
        match state.store.get_service(&membership.service_name).await {
            Ok(Some(service)) if service.enabled => services.push(OwnerServiceBody {
                service,
                role: membership.role,
            }),
            Ok(_) => {}
            Err(error) => {
                return error_response(&headers, error);
            }
        }
    }
    Json(services).into_response()
}

async fn owner_service(
    State(state): State<AppState>,
    Path(service_name): Path<String>,
    headers: HeaderMap,
) -> Response {
    let role = match require_owner_service_access(&state, &headers, &service_name).await {
        Ok(role) => role,
        Err(response) => return response,
    };
    match state.store.get_service(&service_name).await {
        Ok(Some(service)) if service.enabled => {
            Json(OwnerServiceBody { service, role }).into_response()
        }
        Ok(_) => StatusCode::NOT_FOUND.into_response(),
        Err(error) => error_response(&headers, error),
    }
}

#[derive(Debug, Serialize)]
struct OwnerDashboardBody {
    service_name: String,
    role: ServiceMemberRole,
    summary: gateway_core::UsageSummary,
    timeseries: Vec<gateway_core::UsageTimeseriesPoint>,
    endpoints: Vec<gateway_core::UsageBreakdown>,
    providers: Vec<gateway_core::UsageBreakdown>,
    models: Vec<gateway_core::UsageBreakdown>,
}

async fn owner_service_dashboard(
    State(state): State<AppState>,
    Path(service_name): Path<String>,
    headers: HeaderMap,
    Query(mut query): Query<UsageQuery>,
) -> Response {
    let role = match require_owner_service_access(&state, &headers, &service_name).await {
        Ok(role) => role,
        Err(response) => return response,
    };
    query.service = Some(service_name.clone());
    let summary = match state.store.usage_summary(query.clone()).await {
        Ok(value) => value,
        Err(error) => return error_response(&headers, error),
    };
    let timeseries = match state.store.usage_timeseries(query.clone()).await {
        Ok(value) => value,
        Err(error) => return error_response(&headers, error),
    };
    let endpoints = match state
        .store
        .usage_breakdown(query.clone(), UsageBreakdownDimension::Endpoint)
        .await
    {
        Ok(value) => value,
        Err(error) => return error_response(&headers, error),
    };
    let providers = match state
        .store
        .usage_breakdown(query.clone(), UsageBreakdownDimension::Provider)
        .await
    {
        Ok(value) => value,
        Err(error) => return error_response(&headers, error),
    };
    let models = match state
        .store
        .usage_breakdown(query, UsageBreakdownDimension::Model)
        .await
    {
        Ok(value) => value,
        Err(error) => return error_response(&headers, error),
    };
    Json(OwnerDashboardBody {
        service_name,
        role,
        summary,
        timeseries,
        endpoints,
        providers,
        models,
    })
    .into_response()
}

async fn owner_service_events(
    State(state): State<AppState>,
    Path(service_name): Path<String>,
    headers: HeaderMap,
    Query(mut query): Query<UsageQuery>,
) -> Response {
    if let Err(response) = require_owner_service_access(&state, &headers, &service_name).await {
        return response;
    }
    query.service = Some(service_name);
    match state.store.usage_events(query).await {
        Ok(events) => Json(events).into_response(),
        Err(error) => error_response(&headers, error),
    }
}

async fn owner_service_errors(
    State(state): State<AppState>,
    Path(service_name): Path<String>,
    headers: HeaderMap,
    Query(mut query): Query<UsageQuery>,
) -> Response {
    if let Err(response) = require_owner_service_access(&state, &headers, &service_name).await {
        return response;
    }
    query.service = Some(service_name);
    query.status = Some("failure".to_owned());
    match state.store.usage_events(query).await {
        Ok(events) => Json(events).into_response(),
        Err(error) => error_response(&headers, error),
    }
}

async fn owner_service_logs(
    state: State<AppState>,
    path: Path<String>,
    headers: HeaderMap,
    query: Query<UsageQuery>,
) -> Response {
    owner_service_events(state, path, headers, query).await
}

async fn owner_service_endpoints(
    State(state): State<AppState>,
    Path(service_name): Path<String>,
    headers: HeaderMap,
    Query(mut query): Query<UsageQuery>,
) -> Response {
    if let Err(response) = require_owner_service_access(&state, &headers, &service_name).await {
        return response;
    }
    query.service = Some(service_name);
    match state
        .store
        .usage_breakdown(query, UsageBreakdownDimension::Endpoint)
        .await
    {
        Ok(endpoints) => Json(endpoints).into_response(),
        Err(error) => error_response(&headers, error),
    }
}

async fn owner_service_export_json(
    State(state): State<AppState>,
    Path(service_name): Path<String>,
    headers: HeaderMap,
    Query(mut query): Query<UsageQuery>,
) -> Response {
    if let Err(response) = require_owner_service_access(&state, &headers, &service_name).await {
        return response;
    }
    query.service = Some(service_name.clone());
    match state.store.usage_export(query).await {
        Ok(export) => {
            let mut response = Json(export).into_response();
            response.headers_mut().insert(
                header::CONTENT_DISPOSITION,
                HeaderValue::from_str(&format!(
                    "attachment; filename=\"relayna-{service_name}-usage.json\""
                ))
                .unwrap_or_else(|_| HeaderValue::from_static("attachment")),
            );
            response
        }
        Err(error) => error_response(&headers, error),
    }
}

async fn owner_service_export_csv(
    State(state): State<AppState>,
    Path(service_name): Path<String>,
    headers: HeaderMap,
    Query(mut query): Query<UsageQuery>,
) -> Response {
    if let Err(response) = require_owner_service_access(&state, &headers, &service_name).await {
        return response;
    }
    query.service = Some(service_name.clone());
    match state.store.usage_export(query).await {
        Ok(export) => {
            let mut response = usage_export_csv_body(&export).into_response();
            response.headers_mut().insert(
                header::CONTENT_TYPE,
                HeaderValue::from_static("text/csv; charset=utf-8"),
            );
            response.headers_mut().insert(
                header::CONTENT_DISPOSITION,
                HeaderValue::from_str(&format!(
                    "attachment; filename=\"relayna-{service_name}-usage.csv\""
                ))
                .unwrap_or_else(|_| HeaderValue::from_static("attachment")),
            );
            response
        }
        Err(error) => error_response(&headers, error),
    }
}

async fn litellm_ui_proxy_root(
    State(state): State<AppState>,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
    method: Method,
    body: Bytes,
) -> Response {
    let path = if uri.path().ends_with('/') { "/" } else { "" };
    litellm_ui_proxy_inner(state, headers, method, path, uri.query(), body).await
}

async fn litellm_ui_proxy(
    State(state): State<AppState>,
    Path(path): Path<String>,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
    method: Method,
    body: Bytes,
) -> Response {
    litellm_ui_proxy_inner(state, headers, method, &path, uri.query(), body).await
}

async fn litellm_ui_proxy_litellm_prefix(
    State(state): State<AppState>,
    Path(path): Path<String>,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
    method: Method,
    body: Bytes,
) -> Response {
    litellm_ui_proxy_inner(
        state,
        headers,
        method,
        &format!("litellm/{path}"),
        uri.query(),
        body,
    )
    .await
}

async fn litellm_ui_proxy_litellm_config(
    State(state): State<AppState>,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
    method: Method,
    body: Bytes,
) -> Response {
    litellm_ui_proxy_inner(
        state,
        headers,
        method,
        "litellm/.well-known/litellm-ui-config",
        uri.query(),
        body,
    )
    .await
}

async fn litellm_ui_proxy_root_emitted_path(
    State(state): State<AppState>,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
    method: Method,
    body: Bytes,
) -> Response {
    let path = uri.path().trim_start_matches('/');
    litellm_ui_proxy_inner(state, headers, method, path, uri.query(), body).await
}

async fn litellm_ui_proxy_inner(
    state: AppState,
    headers: HeaderMap,
    method: Method,
    path: &str,
    query: Option<&str>,
    body: Bytes,
) -> Response {
    let operator_cookie =
        match require_litellm_ui_operator_scope(&state, &headers, SCOPE_PROVIDERS_UPDATE).await {
            Ok(operator_cookie) => operator_cookie,
            Err(response) => return response,
        };

    let upstream = match resolve_litellm_ui_upstream(&state).await {
        Ok(upstream) => upstream,
        Err(error) => return error_response(&headers, error),
    };
    let url = match litellm_ui_upstream_url(&upstream.base_url, path, query) {
        Ok(url) => url,
        Err(error) => return error_response(&headers, error),
    };
    let reqwest_method = match reqwest::Method::from_bytes(method.as_str().as_bytes()) {
        Ok(method) => method,
        Err(_) => return error_response(&headers, GatewayError::UnsupportedRoute),
    };

    let mut request = state
        .litellm_ui_client
        .request(reqwest_method, url)
        .headers(litellm_ui_forward_headers(&headers, &state, &upstream))
        .body(body);
    request = match upstream.credential_header_mode {
        CredentialHeaderMode::AuthorizationBearer => request.bearer_auth(&upstream.credential),
        CredentialHeaderMode::CustomHeader => {
            let Some(header_name) = upstream.credential_header_name.as_deref() else {
                return error_response(&headers, GatewayError::InvalidConfiguration);
            };
            request.header(header_name, litellm_ui_custom_header_credential(&upstream))
        }
    };

    let response = match request.send().await {
        Ok(response) => response,
        Err(_) => return error_response(&headers, GatewayError::UpstreamConnection),
    };
    litellm_ui_response(response, &upstream.base_url, operator_cookie).await
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
    service_name: Option<String>,
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
    warnings: Vec<String>,
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
    applied_layers: Vec<gateway_core::PolicyLayerTrace>,
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
        features.service_name = request.service_name.clone();
    }
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
    let applied_layers = effective.applied_layers;
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
    let warnings = policy_simulation_warnings(
        &policy,
        route_match.route,
        provider,
        &features,
        applied_layers.len(),
    );

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
            applied_layers,
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
        warnings,
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

fn policy_simulation_warnings(
    policy: &KeyPolicy,
    route: Route,
    provider: Provider,
    features: &gateway_core::GenerationFeatures,
    applied_layer_count: usize,
) -> Vec<String> {
    let mut warnings = Vec::new();
    if policy.deny {
        if applied_layer_count > 1 {
            warnings.push(
                "Effective policy is denied after merging inherited layers; check for disjoint route, provider, model, service, or hour intersections."
                    .to_owned(),
            );
        } else {
            warnings.push("Effective policy is explicitly denied.".to_owned());
        }
    }
    if !policy.allowed_routes.is_empty() && !policy.allowed_routes.contains(&route) {
        warnings.push(format!(
            "Effective route allowlist excludes {}; inherited route allowlists intersect restrictively.",
            route.as_str()
        ));
    }
    if !policy.allowed_providers.is_empty() && !policy.allowed_providers.contains(&provider) {
        warnings.push(format!(
            "Effective provider allowlist excludes {}; inherited provider allowlists intersect restrictively.",
            provider.as_str()
        ));
    }
    if let Some(model) = features.model.as_deref() {
        if !policy.allowed_models.is_empty()
            && !policy
                .allowed_models
                .iter()
                .any(|allowed_model| allowed_model == model)
        {
            warnings.push(format!(
                "Effective model allowlist excludes {model}; inherited model allowlists intersect restrictively."
            ));
        }
    }
    if let Some(service_name) = features.service_name.as_deref() {
        if !policy.allowed_services.is_empty()
            && !policy
                .allowed_services
                .iter()
                .any(|allowed_service| allowed_service == service_name)
        {
            warnings.push(format!(
                "Effective service allowlist excludes {service_name}; inherited service allowlists intersect restrictively."
            ));
        }
    }
    warnings
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
                "/v1/embeddings" => Ok(Route::LiteLlmEmbeddings),
                "/v1/messages" => Ok(Route::AnthropicMessages),
                "/v1/messages/count_tokens" => Ok(Route::AnthropicMessagesCountTokens),
                "/v1/messages/batches" => Ok(Route::AnthropicMessageBatches),
                "/v1/messages/batches/*" => Ok(Route::AnthropicMessageBatch),
                "/v1/messages/batches/*/results" => Ok(Route::AnthropicMessageBatchResults),
                "/v1/messages/batches/*/cancel" => Ok(Route::AnthropicMessageBatchCancel),
                "/v1/models" => Ok(Route::AnthropicModels),
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

async fn upsert_litellm_credential_mapping(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<LiteLlmCredentialMappingUpsertRequest>,
) -> Response {
    admin_mutation(
        headers,
        &state,
        SCOPE_PROVIDERS_UPDATE,
        "litellm_credentials:upsert",
        "litellm_credential_mapping",
        |mapping: &LiteLlmCredentialMappingResponse| Some(mapping.id.to_string()),
        |store| async move { store.upsert_litellm_credential_mapping(request).await },
    )
    .await
}

async fn list_litellm_credential_mappings(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    admin_query(headers, &state, SCOPE_USAGE_READ, |store| async move {
        store.list_litellm_credential_mappings().await
    })
    .await
}

async fn get_litellm_passthrough_settings(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    admin_query(headers, &state, SCOPE_USAGE_READ, |store| async move {
        store.get_litellm_passthrough_settings().await
    })
    .await
}

async fn patch_litellm_passthrough_settings(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(patch): Json<LiteLlmPassthroughSettingsPatchRequest>,
) -> Response {
    let actor = match require_admin_scope(&state, &headers, SCOPE_PROVIDERS_UPDATE).await {
        Ok(actor) => actor,
        Err(response) => return response,
    };
    let before = match state.store.get_litellm_passthrough_settings().await {
        Ok(settings) => Some(settings),
        Err(error) => return error_response(&headers, error),
    };
    match state.store.patch_litellm_passthrough_settings(patch).await {
        Ok(settings) => {
            if let Err(error) = record_admin_audit(
                &state,
                &headers,
                &actor,
                "providers:litellm_passthrough_update",
                "litellm_passthrough_settings",
                Some("singleton".to_owned()),
                before.as_ref().and_then(audit_json),
                audit_json(&settings),
            )
            .await
            {
                return error_response(&headers, error);
            }
            Json(settings).into_response()
        }
        Err(error) => error_response(&headers, error),
    }
}

async fn delete_litellm_credential_mapping(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(mapping_id): Path<uuid::Uuid>,
) -> Response {
    let actor = match require_admin_scope(&state, &headers, SCOPE_PROVIDERS_UPDATE).await {
        Ok(actor) => actor,
        Err(response) => return response,
    };
    match state
        .store
        .delete_litellm_credential_mapping(mapping_id)
        .await
    {
        Ok(true) => {
            if let Err(error) = record_admin_audit(
                &state,
                &headers,
                &actor,
                "litellm_credentials:delete",
                "litellm_credential_mapping",
                Some(mapping_id.to_string()),
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

async fn disable_litellm_credential_mapping(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(mapping_id): Path<uuid::Uuid>,
) -> Response {
    mutate_litellm_credential_mapping_enabled(state, headers, mapping_id, false).await
}

async fn enable_litellm_credential_mapping(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(mapping_id): Path<uuid::Uuid>,
) -> Response {
    mutate_litellm_credential_mapping_enabled(state, headers, mapping_id, true).await
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

async fn list_anthropic_routes(State(state): State<AppState>, headers: HeaderMap) -> Response {
    admin_query(headers, &state, SCOPE_USAGE_READ, |store| async move {
        store.list_anthropic_route_settings().await
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

async fn disable_anthropic_route(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(route_id): Path<String>,
) -> Response {
    mutate_anthropic_route_enabled(state, headers, route_id, false).await
}

async fn enable_anthropic_route(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(route_id): Path<String>,
) -> Response {
    mutate_anthropic_route_enabled(state, headers, route_id, true).await
}

#[derive(Debug, Deserialize)]
struct OpenAiRouteModePatchRequest {
    mode: OpenAiRouteMode,
}

async fn patch_openai_route_mode(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(route_id): Path<String>,
    Json(request): Json<OpenAiRouteModePatchRequest>,
) -> Response {
    let actor = match require_admin_scope(&state, &headers, SCOPE_POLICIES_UPDATE).await {
        Ok(actor) => actor,
        Err(response) => return response,
    };

    match state
        .store
        .set_openai_route_mode(&route_id, request.mode)
        .await
    {
        Ok(Some(setting)) => {
            if let Err(error) = record_admin_audit(
                &state,
                &headers,
                &actor,
                "policies:route_mode_update",
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

async fn patch_openai_route_config(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(route_id): Path<String>,
    Json(request): Json<OpenAiRouteConfigPatchRequest>,
) -> Response {
    let actor = match require_admin_scope(&state, &headers, SCOPE_POLICIES_UPDATE).await {
        Ok(actor) => actor,
        Err(response) => return response,
    };

    match state
        .store
        .patch_openai_route_config(&route_id, request)
        .await
    {
        Ok(Some(setting)) => {
            if let Err(error) = record_admin_audit(
                &state,
                &headers,
                &actor,
                "policies:route_config_update",
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

async fn patch_anthropic_route_mode(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(route_id): Path<String>,
    Json(request): Json<OpenAiRouteModePatchRequest>,
) -> Response {
    let actor = match require_admin_scope(&state, &headers, SCOPE_POLICIES_UPDATE).await {
        Ok(actor) => actor,
        Err(response) => return response,
    };

    match state
        .store
        .set_anthropic_route_mode(&route_id, request.mode)
        .await
    {
        Ok(Some(setting)) => {
            if let Err(error) = record_admin_audit(
                &state,
                &headers,
                &actor,
                "policies:route_mode_update",
                "anthropic_route",
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

async fn patch_anthropic_route_config(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(route_id): Path<String>,
    Json(request): Json<OpenAiRouteConfigPatchRequest>,
) -> Response {
    let actor = match require_admin_scope(&state, &headers, SCOPE_POLICIES_UPDATE).await {
        Ok(actor) => actor,
        Err(response) => return response,
    };

    match state
        .store
        .patch_anthropic_route_config(&route_id, request)
        .await
    {
        Ok(Some(setting)) => {
            if let Err(error) = record_admin_audit(
                &state,
                &headers,
                &actor,
                "policies:route_config_update",
                "anthropic_route",
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

async fn preview_service_openapi(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(service_name): Path<String>,
    Json(request): Json<ServiceOpenApiPreviewRequest>,
) -> Response {
    if let Err(response) = require_admin_scope(&state, &headers, SCOPE_SERVICES_UPDATE).await {
        return response;
    }
    let service = match state.store.get_service(&service_name).await {
        Ok(Some(service)) => service,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(error) => return error_response(&headers, error),
    };
    match fetch_service_openapi(&service, &request.source_path).await {
        Ok(preview) => Json(preview).into_response(),
        Err(error) => error_response(&headers, error),
    }
}

async fn sync_service_openapi(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(service_name): Path<String>,
    Json(request): Json<ServiceOpenApiSyncRequest>,
) -> Response {
    let actor = match require_admin_scope(&state, &headers, SCOPE_SERVICES_UPDATE).await {
        Ok(actor) => actor,
        Err(response) => return response,
    };
    let before = match state.store.get_service(&service_name).await {
        Ok(Some(service)) => service,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(error) => return error_response(&headers, error),
    };
    let preview = match fetch_service_openapi(&before, &request.source_path).await {
        Ok(preview) => preview,
        Err(error) => return error_response(&headers, error),
    };
    if preview.schema_hash != request.expected_schema_hash {
        return error_response(&headers, GatewayError::ServiceOpenApiChanged);
    }

    let endpoint_pricing_rules = merge_endpoint_pricing_rules(
        &preview.endpoints,
        &before.endpoint_pricing_rules,
        before.cost_mode,
        before.estimated_cost_usd,
    );
    let patch = ServicePatchRequest {
        openapi_source_path: Some(Some(preview.source_path.clone())),
        openapi_schema_hash: Some(Some(preview.schema_hash.clone())),
        openapi_synced_at: Some(Some(Utc::now())),
        openapi_endpoints: Some(preview.endpoints),
        endpoint_pricing_rules: Some(endpoint_pricing_rules),
        ..ServicePatchRequest::default()
    };
    match state.store.patch_service(&service_name, patch).await {
        Ok(Some(service)) => {
            if let Err(error) = record_admin_audit(
                &state,
                &headers,
                &actor,
                "services:openapi_sync",
                "service",
                Some(service.name.clone()),
                audit_json(&before),
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

async fn fetch_service_openapi(
    service: &ServiceResponse,
    source_path: &str,
) -> GatewayResult<ServiceOpenApiPreview> {
    validate_openapi_source_path(source_path)?;
    let upstream = service
        .upstream_base_url
        .as_deref()
        .ok_or(GatewayError::IncompleteService)?;
    let mut url =
        reqwest::Url::parse(upstream).map_err(|_| GatewayError::InvalidServiceUpstream)?;
    if !url.username().is_empty() || url.password().is_some() {
        return Err(GatewayError::InvalidServiceUpstream);
    }
    url.set_path(source_path);
    url.set_query(None);
    url.set_fragment(None);

    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(SERVICE_OPENAPI_TIMEOUT)
        .build()
        .map_err(|_| GatewayError::ServiceOpenApiUnavailable)?;
    let mut response = client
        .get(url)
        .header(header::ACCEPT, "application/json")
        .send()
        .await
        .map_err(|_| GatewayError::ServiceOpenApiUnavailable)?;
    if !response.status().is_success() {
        return Err(GatewayError::ServiceOpenApiUnavailable);
    }
    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if !content_type.contains("json") {
        return Err(GatewayError::InvalidServiceOpenApi);
    }
    if response
        .content_length()
        .is_some_and(|length| length > SERVICE_OPENAPI_MAX_BYTES as u64)
    {
        return Err(GatewayError::InvalidServiceOpenApi);
    }

    let mut document = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| GatewayError::ServiceOpenApiUnavailable)?
    {
        if document.len().saturating_add(chunk.len()) > SERVICE_OPENAPI_MAX_BYTES {
            return Err(GatewayError::InvalidServiceOpenApi);
        }
        document.extend_from_slice(&chunk);
    }
    parse_service_openapi(service, source_path, &document)
}

fn parse_service_openapi(
    service: &ServiceResponse,
    source_path: &str,
    document: &[u8],
) -> GatewayResult<ServiceOpenApiPreview> {
    let value: Value =
        serde_json::from_slice(document).map_err(|_| GatewayError::InvalidServiceOpenApi)?;
    if !value
        .get("openapi")
        .and_then(Value::as_str)
        .is_some_and(|version| version.starts_with("3."))
    {
        return Err(GatewayError::InvalidServiceOpenApi);
    }
    let paths = value
        .get("paths")
        .and_then(Value::as_object)
        .ok_or(GatewayError::InvalidServiceOpenApi)?;
    const METHODS: &[&str] = &["get", "post", "put", "patch", "delete", "head", "options"];
    let mut endpoints = Vec::new();
    for (path_template, path_item) in paths {
        let Some(path_item) = path_item.as_object() else {
            return Err(GatewayError::InvalidServiceOpenApi);
        };
        for method in METHODS {
            let Some(operation) = path_item.get(*method) else {
                continue;
            };
            let operation = operation
                .as_object()
                .ok_or(GatewayError::InvalidServiceOpenApi)?;
            endpoints.push(ServiceOpenApiEndpoint {
                method: method.to_ascii_uppercase(),
                path_template: path_template.clone(),
                operation_id: operation
                    .get("operationId")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned),
                summary: operation
                    .get("summary")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned),
                relayna_default: is_relayna_default_endpoint(path_template),
            });
        }
    }
    endpoints.sort_by(|left, right| {
        left.path_template
            .cmp(&right.path_template)
            .then(left.method.cmp(&right.method))
    });
    validate_openapi_endpoints(&endpoints).map_err(|_| GatewayError::InvalidServiceOpenApi)?;

    let previous = &service.openapi_endpoints;
    let added = endpoints
        .iter()
        .filter(|endpoint| !contains_openapi_endpoint(previous, endpoint))
        .cloned()
        .collect();
    let removed = previous
        .iter()
        .filter(|endpoint| !contains_openapi_endpoint(&endpoints, endpoint))
        .cloned()
        .collect();
    let schema_hash = format!("sha256:{:x}", Sha256::digest(document));
    Ok(ServiceOpenApiPreview {
        source_path: source_path.to_owned(),
        schema_hash,
        title: value
            .pointer("/info/title")
            .and_then(Value::as_str)
            .map(|value| value.chars().take(256).collect()),
        version: value
            .pointer("/info/version")
            .and_then(Value::as_str)
            .map(|value| value.chars().take(64).collect()),
        endpoints,
        added,
        removed,
    })
}

fn contains_openapi_endpoint(
    endpoints: &[ServiceOpenApiEndpoint],
    target: &ServiceOpenApiEndpoint,
) -> bool {
    endpoints.iter().any(|endpoint| {
        endpoint.method.eq_ignore_ascii_case(&target.method)
            && endpoint.path_template == target.path_template
    })
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

async fn get_gateway_auth_settings(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Some(response) = require_admin(&state, &headers).await {
        return response;
    }

    match effective_gateway_auth_settings(&state).await {
        Ok(settings) => Json(settings.response()).into_response(),
        Err(error) => error_response(&headers, error),
    }
}

async fn patch_gateway_auth_settings(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(patch): Json<GatewayAuthSettingsPatchRequest>,
) -> Response {
    let actor = match require_admin_scope(&state, &headers, SCOPE_SETTINGS_UPDATE).await {
        Ok(actor) => actor,
        Err(response) => return response,
    };
    let before = match effective_gateway_auth_settings(&state).await {
        Ok(settings) => Some(settings.response()),
        Err(error) => return error_response(&headers, error),
    };

    match state.store.patch_gateway_auth_settings(patch).await {
        Ok(_) => match effective_gateway_auth_settings(&state).await {
            Ok(settings) => {
                let response = settings.response();
                if let Err(error) = state.auth_runtime.update(settings.runtime_config()) {
                    return error_response(&headers, error);
                }
                if let Err(error) = record_admin_audit(
                    &state,
                    &headers,
                    &actor,
                    "settings:gateway_auth_update",
                    "gateway_auth_settings",
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

async fn usage_dashboard(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<UsageQuery>,
) -> Response {
    admin_query(headers, &state, SCOPE_USAGE_READ, |store| async move {
        store.usage_dashboard(query).await
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

async fn usage_events(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<UsageQuery>,
) -> Response {
    admin_query(headers, &state, SCOPE_USAGE_READ, |store| async move {
        store.usage_events(query).await
    })
    .await
}

async fn usage_filter_values(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<UsageFilterValuesQuery>,
) -> Response {
    admin_query(headers, &state, SCOPE_USAGE_READ, |store| async move {
        store.usage_filter_values(query).await
    })
    .await
}

async fn usage_export_json(
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
            [
                ("content-type", "application/json"),
                (
                    "content-disposition",
                    "attachment; filename=\"relayna-usage.json\"",
                ),
            ],
            Json(export),
        )
            .into_response(),
        Err(error) => error_response(&headers, error),
    }
}

async fn usage_unused_keys(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<UsageQuery>,
) -> Response {
    admin_query(headers, &state, SCOPE_USAGE_READ, |store| async move {
        store.unused_keys(query).await
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
            [
                ("content-type", "text/csv; charset=utf-8"),
                (
                    "content-disposition",
                    "attachment; filename=\"relayna-usage.csv\"",
                ),
            ],
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
    let mut csv = "request_id,key_id,project_id,route,model,provider,status,status_code,latency_ms,input_tokens,output_tokens,total_tokens,estimated_cost_usd,service_name,task_id,run_id,trace_id,fallback_count,guardrail_action_count,created_at,cost_source,cost_mode,pricing_rule_name,http_method,endpoint_path,endpoint_template\n".to_owned();
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
            row.trace_id.clone().unwrap_or_default(),
            row.fallback_count.to_string(),
            row.guardrail_action_count.to_string(),
            row.created_at.to_rfc3339(),
            row.cost_source.clone().unwrap_or_default(),
            row.cost_mode.clone().unwrap_or_default(),
            row.pricing_rule_name.clone().unwrap_or_default(),
            row.http_method.clone().unwrap_or_default(),
            row.endpoint_path.clone().unwrap_or_default(),
            row.endpoint_template.clone().unwrap_or_default(),
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
                    target.health_check_path.as_deref(),
                    &target.health_check_method,
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
        Ok(None) => error_response(&headers, GatewayError::MissingDebugBundle),
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
            actor_token_id: actor.member_id.is_none().then_some(actor.token_id),
            actor_member_id: actor.member_id,
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
        "microsoft-sign-in.svg" => static_response(
            "image/svg+xml",
            include_str!("static/admin-ui/microsoft-sign-in.svg"),
        ),
        "admin-ui-tabler-icons.woff2" => static_binary_response(
            "font/woff2",
            include_bytes!("static/admin-ui/admin-ui-tabler-icons.woff2"),
        ),
        "admin-ui-tabler-icons.woff" => static_binary_response(
            "font/woff",
            include_bytes!("static/admin-ui/admin-ui-tabler-icons.woff"),
        ),
        "admin-ui-tabler-icons.ttf" => static_binary_response(
            "font/ttf",
            include_bytes!("static/admin-ui/admin-ui-tabler-icons.ttf"),
        ),
        _ => StatusCode::NOT_FOUND.into_response(),
    }
}

fn static_response(content_type: &'static str, body: &'static str) -> Response {
    (StatusCode::OK, [("content-type", content_type)], body).into_response()
}

fn static_binary_response(content_type: &'static str, body: &'static [u8]) -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CACHE_CONTROL, "public, max-age=31536000, immutable")
        .body(Body::from(body))
        .expect("static asset response is valid")
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

async fn mutate_anthropic_route_enabled(
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
        .set_anthropic_route_enabled(&route_id, enabled)
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
                "anthropic_route",
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

async fn mutate_litellm_credential_mapping_enabled(
    state: AppState,
    headers: HeaderMap,
    mapping_id: uuid::Uuid,
    enabled: bool,
) -> Response {
    admin_mutation(
        headers,
        &state,
        SCOPE_PROVIDERS_UPDATE,
        if enabled {
            "litellm_credentials:enable"
        } else {
            "litellm_credentials:disable"
        },
        "litellm_credential_mapping",
        |mapping: &LiteLlmCredentialMappingResponse| Some(mapping.id.to_string()),
        |store| async move {
            store
                .set_litellm_credential_mapping_enabled(mapping_id, enabled)
                .await?
                .ok_or(GatewayError::MissingProviderConfig)
        },
    )
    .await
}

async fn effective_studio_connection(state: &AppState) -> GatewayResult<EffectiveStudioConnection> {
    let stored = state.store.studio_connection_settings().await?;
    Ok(EffectiveStudioConnection::from_sources(
        stored,
        &state.studio_env,
    ))
}

async fn effective_gateway_auth_settings(
    state: &AppState,
) -> GatewayResult<EffectiveGatewayAuthSettings> {
    let stored = state.store.gateway_auth_settings().await?;
    EffectiveGatewayAuthSettings::from_sources(stored, &state.auth_env)
}

async fn effective_studio_client(state: &AppState) -> GatewayResult<StudioCatalogClient> {
    let connection = effective_studio_connection(state).await?;
    let base_url = connection
        .base_url
        .ok_or(GatewayError::InvalidConfiguration)?;
    Ok(StudioCatalogClient::new(base_url, connection.token))
}

async fn require_portal_session(
    state: &AppState,
    headers: &HeaderMap,
    require_csrf: bool,
) -> Result<gateway_core::StoredPortalSession, Response> {
    let Some(raw_session) = cookie_value(headers, PORTAL_SESSION_COOKIE) else {
        return Err(error_response(headers, GatewayError::InvalidPortalSession));
    };
    let session = match state
        .store
        .resolve_portal_session(&token_hash(raw_session), Utc::now())
        .await
    {
        Ok(Some(session)) => session,
        Ok(None) => return Err(error_response(headers, GatewayError::InvalidPortalSession)),
        Err(error) => return Err(error_response(headers, error)),
    };
    if require_csrf {
        let csrf = headers
            .get("x-csrf-token")
            .and_then(|value| value.to_str().ok())
            .filter(|value| !value.is_empty());
        if csrf.is_none_or(|csrf| !constant_time_eq(&session.csrf_hash, &token_hash(csrf))) {
            return Err(error_response(headers, GatewayError::InvalidCsrfToken));
        }
    }
    Ok(session)
}

async fn require_active_portal_session(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<gateway_core::StoredPortalSession, Response> {
    let session = require_portal_session(state, headers, false).await?;
    match session.member.status {
        MemberStatus::Active => Ok(session),
        MemberStatus::Pending => Err(error_response(headers, GatewayError::PendingPortalMember)),
        MemberStatus::Blocked => Err(error_response(headers, GatewayError::BlockedPortalMember)),
    }
}

async fn require_owner_service_access(
    state: &AppState,
    headers: &HeaderMap,
    service_name: &str,
) -> Result<ServiceMemberRole, Response> {
    if cookie_value(headers, PORTAL_SESSION_COOKIE).is_some() {
        let session = require_active_portal_session(state, headers).await?;
        return match state
            .store
            .member_service_role(session.member.id, service_name)
            .await
        {
            Ok(Some(role)) => Ok(role),
            Ok(None) => Err(error_response(
                headers,
                GatewayError::InsufficientPortalAccess,
            )),
            Err(error) => Err(error_response(headers, error)),
        };
    }

    let Some(verifier) = state.owner_entra_verifier.as_ref() else {
        return Err(error_response(
            headers,
            GatewayError::MissingEntraAuthorization,
        ));
    };
    let authorization = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok());
    let identity = match verifier
        .verify_authorization(authorization, Utc::now())
        .await
    {
        Ok(identity) => identity,
        Err(error) => return Err(error_response(headers, error)),
    };
    let Some(client_id) = identity
        .app_id
        .as_deref()
        .or(identity.authorized_party.as_deref())
    else {
        return Err(error_response(headers, GatewayError::InvalidEntraToken));
    };
    match state
        .store
        .workload_service_binding(
            &identity.tenant_id,
            client_id,
            identity.object_id.as_deref(),
            service_name,
            &identity.roles,
        )
        .await
    {
        Ok(Some(_)) => Ok(ServiceMemberRole::Viewer),
        Ok(None) => Err(error_response(
            headers,
            GatewayError::InsufficientPortalAccess,
        )),
        Err(error) => Err(error_response(headers, error)),
    }
}

fn cookie_value<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get(header::COOKIE)
        .and_then(|value| value.to_str().ok())
        .and_then(|cookies| {
            cookies.split(';').find_map(|cookie| {
                let (cookie_name, value) = cookie.trim().split_once('=')?;
                (cookie_name == name && !value.is_empty()).then_some(value)
            })
        })
}

fn append_portal_cookies(
    headers: &mut HeaderMap,
    raw_session: &str,
    raw_csrf: &str,
    max_age_seconds: i64,
    secure: bool,
) {
    let secure = if secure { "; Secure" } else { "" };
    let session = format!(
        "{PORTAL_SESSION_COOKIE}={raw_session}; HttpOnly; SameSite=Lax; Path=/; Max-Age={max_age_seconds}{secure}"
    );
    let csrf = format!(
        "{PORTAL_CSRF_COOKIE}={raw_csrf}; SameSite=Lax; Path=/; Max-Age={max_age_seconds}{secure}"
    );
    if let Ok(value) = HeaderValue::from_str(&session) {
        headers.append(header::SET_COOKIE, value);
    }
    if let Ok(value) = HeaderValue::from_str(&csrf) {
        headers.append(header::SET_COOKIE, value);
    }
}

fn append_portal_login_cookie(
    headers: &mut HeaderMap,
    raw_binding: &str,
    max_age_seconds: i64,
    secure: bool,
) {
    let secure = if secure { "; Secure" } else { "" };
    let cookie = format!(
        "{PORTAL_LOGIN_COOKIE}={raw_binding}; HttpOnly; SameSite=Lax; Path=/admin-ui/auth; Max-Age={max_age_seconds}{secure}"
    );
    if let Ok(value) = HeaderValue::from_str(&cookie) {
        headers.append(header::SET_COOKIE, value);
    }
}

fn clear_portal_login_cookie(headers: &mut HeaderMap, secure: bool) {
    let secure = if secure { "; Secure" } else { "" };
    let cookie = format!(
        "{PORTAL_LOGIN_COOKIE}=; HttpOnly; SameSite=Lax; Path=/admin-ui/auth; Max-Age=0{secure}"
    );
    if let Ok(value) = HeaderValue::from_str(&cookie) {
        headers.append(header::SET_COOKIE, value);
    }
}

fn clear_portal_cookies(headers: &mut HeaderMap, secure: bool) {
    let secure = if secure { "; Secure" } else { "" };
    for cookie in [
        format!("{PORTAL_SESSION_COOKIE}=; HttpOnly; SameSite=Lax; Path=/; Max-Age=0{secure}"),
        format!("{PORTAL_CSRF_COOKIE}=; SameSite=Lax; Path=/; Max-Age=0{secure}"),
    ] {
        if let Ok(value) = HeaderValue::from_str(&cookie) {
            headers.append(header::SET_COOKIE, value);
        }
    }
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
    if headers.contains_key(header::AUTHORIZATION) {
        let token = bearer_token(headers).map_err(|error| error_response(headers, error))?;
        return match state.store.verify_operator_token(token, Utc::now()).await {
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
        };
    }

    let session = require_portal_session(state, headers, true).await?;
    match session.member.status {
        MemberStatus::Pending => {
            return Err(error_response(headers, GatewayError::PendingPortalMember));
        }
        MemberStatus::Blocked => {
            return Err(error_response(headers, GatewayError::BlockedPortalMember));
        }
        MemberStatus::Active => {}
    }
    if !session.member.is_admin() {
        return Err(error_response(
            headers,
            GatewayError::InsufficientPortalAccess,
        ));
    }
    Ok(OperatorAuthorization {
        token_id: session.member.id,
        member_id: Some(session.member.id),
        token_prefix: session
            .member
            .email
            .clone()
            .unwrap_or_else(|| "entra-member".to_owned()),
        roles: session.member.roles,
        scopes: default_operator_scopes(),
    })
}

async fn require_litellm_ui_operator_scope(
    state: &AppState,
    headers: &HeaderMap,
    required_scope: &str,
) -> Result<Option<String>, Response> {
    match bearer_token(headers) {
        Ok(token) => match verify_operator_scope(state, token, &[required_scope]).await {
            Ok(()) => Ok(Some(token.to_owned())),
            Err(error) => Err(error_response(headers, error)),
        },
        Err(GatewayError::MissingAuthorization) => {
            let Some(token) = litellm_ui_operator_cookie(headers) else {
                return Err(error_response(headers, GatewayError::MissingAuthorization));
            };
            match verify_operator_scope(state, token, &[required_scope]).await {
                Ok(()) => Ok(None),
                Err(error) => Err(error_response(headers, error)),
            }
        }
        Err(error) => Err(error_response(headers, error)),
    }
}

async fn verify_operator_scope(
    state: &AppState,
    token: &str,
    required_scopes: &[&str],
) -> GatewayResult<()> {
    match state.store.verify_operator_token(token, Utc::now()).await {
        Ok(authorization)
            if required_scopes
                .iter()
                .all(|required_scope| authorization.has_scope(required_scope)) =>
        {
            Ok(())
        }
        Ok(_) => Err(GatewayError::InsufficientOperatorScope),
        Err(error) => Err(error),
    }
}

fn litellm_ui_operator_cookie(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::COOKIE)
        .and_then(|value| value.to_str().ok())
        .and_then(|cookies| {
            cookies.split(';').find_map(|cookie| {
                let (name, value) = cookie.trim().split_once('=')?;
                (name == LITELLM_UI_OPERATOR_COOKIE && !value.is_empty()).then_some(value)
            })
        })
}

fn litellm_ui_operator_set_cookie(raw_token: &str) -> String {
    format!(
        "{LITELLM_UI_OPERATOR_COOKIE}={raw_token}; HttpOnly; SameSite=Lax; Path=/; Max-Age={LITELLM_UI_OPERATOR_COOKIE_MAX_AGE_SECONDS}"
    )
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
    health_check_path: Option<&str>,
    health_check_method: &str,
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
    let Ok(url) = health_check_url(&base_url, health_check_path) else {
        return ActiveHealthCheck {
            ok: false,
            latency_ms: None,
            error_code: Some("invalid_upstream_url".to_owned()),
            checked_at,
        };
    };
    let started = std::time::Instant::now();
    let method = if health_check_method.eq_ignore_ascii_case("HEAD") {
        reqwest::Method::HEAD
    } else {
        reqwest::Method::GET
    };
    let mut request = client.request(method, url);
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

fn health_check_url(base_url: &str, health_check_path: Option<&str>) -> Result<reqwest::Url, ()> {
    let mut url = reqwest::Url::parse(base_url).map_err(|_| ())?;
    if let Some(path) = health_check_path.filter(|value| !value.trim().is_empty()) {
        url.set_path(path);
        url.set_query(None);
    }
    Ok(url)
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

#[derive(Debug, Clone)]
struct LiteLlmUiUpstream {
    base_url: String,
    credential: String,
    credential_header_mode: CredentialHeaderMode,
    credential_header_name: Option<String>,
    credential_header_value_format: CredentialHeaderValueFormat,
}

async fn resolve_litellm_ui_upstream(state: &AppState) -> GatewayResult<LiteLlmUiUpstream> {
    let active = state.store.active_litellm_config().await?;
    let base_url = active
        .as_ref()
        .map(|config| config.base_url.clone())
        .unwrap_or_else(|| state.litellm_base_url.clone());
    let credential = active
        .as_ref()
        .and_then(|config| config.credential.clone())
        .unwrap_or_else(|| state.litellm_service_key.clone());
    if credential.trim().is_empty() {
        return Err(GatewayError::InvalidConfiguration);
    }
    let credential_header_mode = active
        .as_ref()
        .map(|config| config.credential_header_mode)
        .unwrap_or(CredentialHeaderMode::AuthorizationBearer);
    let credential_header_name = active
        .as_ref()
        .and_then(|config| config.credential_header_name.clone());
    let credential_header_value_format = active
        .as_ref()
        .map(|config| config.credential_header_value_format)
        .unwrap_or(CredentialHeaderValueFormat::Raw);
    if credential_header_mode == CredentialHeaderMode::CustomHeader
        && credential_header_name.as_deref().is_none()
    {
        return Err(GatewayError::InvalidConfiguration);
    }

    Ok(LiteLlmUiUpstream {
        base_url,
        credential,
        credential_header_mode,
        credential_header_name,
        credential_header_value_format,
    })
}

fn litellm_ui_custom_header_credential(upstream: &LiteLlmUiUpstream) -> String {
    match upstream.credential_header_value_format {
        CredentialHeaderValueFormat::Raw => upstream.credential.clone(),
        CredentialHeaderValueFormat::Bearer => format!("Bearer {}", upstream.credential),
    }
}

fn litellm_ui_upstream_url(
    base_url: &str,
    path: &str,
    query: Option<&str>,
) -> GatewayResult<reqwest::Url> {
    let mut url = reqwest::Url::parse(base_url).map_err(|_| GatewayError::InvalidConfiguration)?;
    let wants_trailing_slash = path == "/";
    let path = path.trim_start_matches('/');
    if wants_trailing_slash {
        url.set_path("/ui/");
    } else if litellm_ui_maps_to_upstream_root(path) {
        url.set_path(&format!("/{path}"));
    } else if path.is_empty() {
        url.set_path("/ui");
    } else {
        url.set_path(&format!("/ui/{path}"));
    }
    url.set_query(query);
    Ok(url)
}

fn litellm_ui_maps_to_upstream_root(path: &str) -> bool {
    path.starts_with("litellm-asset-prefix/")
        || path.starts_with("litellm/")
        || LITELLM_UI_ROOT_PROXY_PREFIXES
            .iter()
            .any(|prefix| path.starts_with(prefix))
}

fn litellm_ui_forward_headers(
    headers: &HeaderMap,
    state: &AppState,
    upstream: &LiteLlmUiUpstream,
) -> HeaderMap {
    let relayna_key_header = state
        .auth_runtime
        .snapshot()
        .ok()
        .map(|snapshot| snapshot.config.relayna_key_header)
        .unwrap_or_else(|| state.auth_env.relayna_key_header.clone());
    let custom_litellm_header = upstream
        .credential_header_name
        .as_deref()
        .and_then(|name| HeaderName::from_bytes(name.as_bytes()).ok());
    let mut forwarded = HeaderMap::new();
    for (name, value) in headers {
        if litellm_ui_skips_request_header(
            name,
            &relayna_key_header,
            custom_litellm_header.as_ref(),
        ) {
            continue;
        }
        forwarded.append(name.clone(), value.clone());
    }
    forwarded
}

fn litellm_ui_skips_request_header(
    name: &HeaderName,
    relayna_key_header: &str,
    custom_litellm_header: Option<&HeaderName>,
) -> bool {
    name == header::AUTHORIZATION
        || name.as_str().eq_ignore_ascii_case("proxy-authorization")
        || name == header::HOST
        || name == header::COOKIE
        || name == header::CONTENT_LENGTH
        || name == header::TRANSFER_ENCODING
        || name == header::CONNECTION
        || name == header::UPGRADE
        || name.as_str().eq_ignore_ascii_case(relayna_key_header)
        || name.as_str().eq_ignore_ascii_case("x-relayna-key")
        || name.as_str().eq_ignore_ascii_case("x-litellm-api-key")
        || name.as_str().eq_ignore_ascii_case("x-litellm-key")
        || name.as_str().eq_ignore_ascii_case("x-aih-api-key")
        || name.as_str().eq_ignore_ascii_case("x-api-key")
        || name.as_str().eq_ignore_ascii_case("x-relayna-worker-token")
        || name
            .as_str()
            .eq_ignore_ascii_case("x-apigee-entra-identity")
        || name
            .as_str()
            .eq_ignore_ascii_case("x-apigee-entra-signature")
        || custom_litellm_header.is_some_and(|custom| name == custom)
}

async fn litellm_ui_response(
    response: reqwest::Response,
    upstream_base_url: &str,
    operator_cookie: Option<String>,
) -> Response {
    let status = response.status();
    let headers = response.headers().clone();
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    let is_html = content_type
        .split(';')
        .any(|part| part.trim().eq_ignore_ascii_case("text/html"));
    let is_javascript = content_type.split(';').any(|part| {
        let part = part.trim();
        part.eq_ignore_ascii_case("text/javascript")
            || part.eq_ignore_ascii_case("application/javascript")
    });
    let is_json = content_type
        .split(';')
        .any(|part| part.trim().eq_ignore_ascii_case("application/json"));
    let body = match response.bytes().await {
        Ok(body) => body,
        Err(_) => return error_response(&HeaderMap::new(), GatewayError::UpstreamConnection),
    };
    let body = if (is_html || is_javascript) && body.len() <= LITELLM_UI_HTML_REWRITE_LIMIT {
        Bytes::from(rewrite_litellm_ui_text(
            &String::from_utf8_lossy(&body),
            upstream_base_url,
        ))
    } else if is_json && body.len() <= LITELLM_UI_HTML_REWRITE_LIMIT {
        rewrite_litellm_ui_json_body(&body, upstream_base_url).unwrap_or(body)
    } else {
        body
    };

    let mut builder = Response::builder().status(status);
    if let Some(token) = operator_cookie {
        builder = builder.header(header::SET_COOKIE, litellm_ui_operator_set_cookie(&token));
    }
    for (name, value) in &headers {
        if litellm_ui_skips_response_header(name) {
            continue;
        }
        if name == header::LOCATION {
            if let Some(rewritten) = rewrite_litellm_ui_location(value, upstream_base_url) {
                builder = builder.header(name, rewritten);
            } else {
                builder = builder.header(name, value);
            }
        } else {
            builder = builder.header(name, value);
        }
    }
    builder
        .body(Body::from(body))
        .unwrap_or_else(|_| error_response(&HeaderMap::new(), GatewayError::UpstreamConnection))
}

fn litellm_ui_skips_response_header(name: &HeaderName) -> bool {
    name == header::CONTENT_LENGTH
        || name == header::TRANSFER_ENCODING
        || name == header::CONNECTION
        || name == header::UPGRADE
}

fn rewrite_litellm_ui_location(value: &HeaderValue, upstream_base_url: &str) -> Option<String> {
    let value = value.to_str().ok()?;
    if let Some(rest) = value.strip_prefix("/ui") {
        return Some(format!("{LITELLM_UI_PROXY_PREFIX}{rest}"));
    }
    let base = upstream_base_url.trim_end_matches('/');
    let absolute_prefix = format!("{base}/ui");
    if let Some(rest) = value.strip_prefix(&absolute_prefix) {
        return Some(format!("{LITELLM_UI_PROXY_PREFIX}{rest}"));
    }
    if let Ok(url) = reqwest::Url::parse(value) {
        if let Some(rest) = url.path().strip_prefix("/ui") {
            let mut rewritten = format!("{LITELLM_UI_PROXY_PREFIX}{rest}");
            if let Some(query) = url.query() {
                rewritten.push('?');
                rewritten.push_str(query);
            }
            return Some(rewritten);
        }
    }
    None
}

fn rewrite_litellm_ui_text(body: &str, upstream_base_url: &str) -> String {
    let base = upstream_base_url.trim_end_matches('/');
    body.replace(
        &format!("{base}/ui/"),
        &format!("{LITELLM_UI_PROXY_PREFIX}/"),
    )
    .replace(base, LITELLM_UI_PROXY_PREFIX)
    .replace("\"/ui/\"", &format!("\"{LITELLM_UI_PROXY_PREFIX}/\""))
    .replace("'/ui/'", &format!("'{LITELLM_UI_PROXY_PREFIX}/'"))
    .replace("\"/ui\"", &format!("\"{LITELLM_UI_PROXY_PREFIX}\""))
    .replace("'/ui'", &format!("'{LITELLM_UI_PROXY_PREFIX}'"))
    .replace("\"/ui/", &format!("\"{LITELLM_UI_PROXY_PREFIX}/"))
    .replace("'/ui/", &format!("'{LITELLM_UI_PROXY_PREFIX}/"))
    .replace("=/ui/", &format!("={LITELLM_UI_PROXY_PREFIX}/"))
    .replace(
        "\"/litellm-asset-prefix/",
        &format!("\"{LITELLM_UI_PROXY_PREFIX}/litellm-asset-prefix/"),
    )
    .replace(
        "'/litellm-asset-prefix/",
        &format!("'{LITELLM_UI_PROXY_PREFIX}/litellm-asset-prefix/"),
    )
    .replace(
        "=/litellm-asset-prefix/",
        &format!("={LITELLM_UI_PROXY_PREFIX}/litellm-asset-prefix/"),
    )
    .replace(
        "\"/litellm/",
        &format!("\"{LITELLM_UI_PROXY_PREFIX}/litellm/"),
    )
    .replace(
        "'/litellm/",
        &format!("'{LITELLM_UI_PROXY_PREFIX}/litellm/"),
    )
    .replace(
        "=/litellm/",
        &format!("={LITELLM_UI_PROXY_PREFIX}/litellm/"),
    )
    .replace("\"/v2/", &format!("\"{LITELLM_UI_PROXY_PREFIX}/v2/"))
    .replace("'/v2/", &format!("'{LITELLM_UI_PROXY_PREFIX}/v2/"))
    .replace("`/v2/", &format!("`{LITELLM_UI_PROXY_PREFIX}/v2/"))
    .replace("\"/v3/", &format!("\"{LITELLM_UI_PROXY_PREFIX}/v3/"))
    .replace("'/v3/", &format!("'{LITELLM_UI_PROXY_PREFIX}/v3/"))
    .replace("`/v3/", &format!("`{LITELLM_UI_PROXY_PREFIX}/v3/"))
    .replace("\"/get/", &format!("\"{LITELLM_UI_PROXY_PREFIX}/get/"))
    .replace("'/get/", &format!("'{LITELLM_UI_PROXY_PREFIX}/get/"))
    .replace("`/get/", &format!("`{LITELLM_UI_PROXY_PREFIX}/get/"))
    .replace(
        "\"/get_image",
        &format!("\"{LITELLM_UI_PROXY_PREFIX}/get_image"),
    )
    .replace(
        "'/get_image",
        &format!("'{LITELLM_UI_PROXY_PREFIX}/get_image"),
    )
    .replace(
        "`/get_image",
        &format!("`{LITELLM_UI_PROXY_PREFIX}/get_image"),
    )
    .replace(
        "\"/public/",
        &format!("\"{LITELLM_UI_PROXY_PREFIX}/public/"),
    )
    .replace("'/public/", &format!("'{LITELLM_UI_PROXY_PREFIX}/public/"))
    .replace("`/public/", &format!("`{LITELLM_UI_PROXY_PREFIX}/public/"))
}

fn rewrite_litellm_ui_json_body(body: &[u8], upstream_base_url: &str) -> Option<Bytes> {
    let mut value: serde_json::Value = serde_json::from_slice(body).ok()?;
    rewrite_litellm_ui_json_value(&mut value, upstream_base_url);
    serde_json::to_vec(&value).ok().map(Bytes::from)
}

fn rewrite_litellm_ui_json_value(value: &mut serde_json::Value, upstream_base_url: &str) {
    match value {
        serde_json::Value::String(text) => {
            if let Ok(header_value) = HeaderValue::from_str(text) {
                if let Some(rewritten) =
                    rewrite_litellm_ui_location(&header_value, upstream_base_url)
                {
                    *text = rewritten;
                }
            }
        }
        serde_json::Value::Array(values) => {
            for value in values {
                rewrite_litellm_ui_json_value(value, upstream_base_url);
            }
        }
        serde_json::Value::Object(values) => {
            for value in values.values_mut() {
                rewrite_litellm_ui_json_value(value, upstream_base_url);
            }
        }
        serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use gateway_core::{
        admin::{AdminKeyUsageSummary, AdminPolicyResponse, ProjectUsageSummary},
        auth::StoredVirtualKey,
        default_operator_roles, default_operator_scopes, AuthenticatedKey, EntraAuthConfig,
        LiteLlmPassthroughSettings, OpenAiRouteConfigPatchRequest, OpenAiRouteMode,
        OpenAiRouteSetting, OperatorTokenResponse, PatchValue, ProjectCreateRequest,
        ProjectPatchRequest, ProjectResponse, ProviderConfigCreateRequest,
        ProviderConfigPatchRequest, ProviderConfigResponse, ProviderHealth, Route, ServiceCostMode,
        ServiceResponse, ServiceSource, ServiceSyncStatus, ServiceSyncStatusResponse,
        StoredGatewayAuthSettings, StoredStudioConnection, StudioConnectionPatchRequest,
        UsageBreakdown, UsageExportRow, UsagePage, UsageServiceTimeseriesPoint, UsageStatus,
        UsageSummary, UsageTimeseriesPoint,
    };
    use std::{
        collections::BTreeMap,
        io::{Read, Write},
        net::TcpListener,
        process::{Child, Command, Stdio},
        sync::{mpsc, Mutex},
        thread,
    };
    use tower::ServiceExt;
    use uuid::Uuid;

    #[derive(Clone)]
    struct MemoryStore {
        key: Arc<Mutex<Option<StoredVirtualKey>>>,
        admin_key: Arc<Mutex<Option<AdminKeyResponse>>>,
        services: Arc<Mutex<Vec<ServiceResponse>>>,
        openai_routes: Arc<Mutex<Vec<OpenAiRouteSetting>>>,
        anthropic_routes: Arc<Mutex<Vec<OpenAiRouteSetting>>>,
        operator_tokens: Arc<Mutex<Vec<String>>>,
        events: Arc<Mutex<Vec<UsageEvent>>>,
        audit_events: Arc<Mutex<Vec<AuditEvent>>>,
        studio_connection: Arc<Mutex<Option<StoredStudioConnection>>>,
        gateway_auth_settings: Arc<Mutex<Option<StoredGatewayAuthSettings>>>,
        litellm_passthrough_settings: Arc<Mutex<LiteLlmPassthroughSettings>>,
        portal_members: Arc<Mutex<Vec<PortalMember>>>,
        service_memberships: Arc<Mutex<Vec<ServiceMembership>>>,
        managed_identities: Arc<Mutex<Vec<gateway_core::ManagedIdentityBinding>>>,
        oidc_transactions: Arc<Mutex<Vec<OidcLoginTransaction>>>,
        portal_sessions: Arc<Mutex<Vec<NewPortalSession>>>,
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
    impl PortalAccessStore for MemoryStore {
        async fn upsert_oidc_member(
            &self,
            tenant_id: &str,
            object_id: &str,
            email: Option<&str>,
            display_name: Option<&str>,
            now: chrono::DateTime<Utc>,
        ) -> GatewayResult<PortalMember> {
            let mut members = self.portal_members.lock().expect("lock poisoned");
            if let Some(member) = members
                .iter_mut()
                .find(|member| member.tenant_id == tenant_id && member.object_id == object_id)
            {
                if email.is_some() {
                    member.email = email.map(ToOwned::to_owned);
                }
                if display_name.is_some() {
                    member.display_name = display_name.map(ToOwned::to_owned);
                }
                member.last_sign_in_at = Some(now);
                member.updated_at = now;
                return Ok(member.clone());
            }
            let member = PortalMember {
                id: Uuid::new_v4(),
                tenant_id: tenant_id.to_owned(),
                object_id: object_id.to_owned(),
                email: email.map(ToOwned::to_owned),
                display_name: display_name.map(ToOwned::to_owned),
                status: MemberStatus::Pending,
                roles: Vec::new(),
                last_sign_in_at: Some(now),
                created_at: now,
                updated_at: now,
            };
            members.push(member.clone());
            Ok(member)
        }

        async fn list_members(&self) -> GatewayResult<Vec<PortalMember>> {
            Ok(self.portal_members.lock().expect("lock poisoned").clone())
        }

        async fn get_member(&self, member_id: Uuid) -> GatewayResult<Option<PortalMember>> {
            Ok(self
                .portal_members
                .lock()
                .expect("lock poisoned")
                .iter()
                .find(|member| member.id == member_id)
                .cloned())
        }

        async fn patch_member(
            &self,
            member_id: Uuid,
            patch: MemberPatchRequest,
        ) -> GatewayResult<Option<PortalMember>> {
            let mut members = self.portal_members.lock().expect("lock poisoned");
            let Some(member) = members.iter_mut().find(|member| member.id == member_id) else {
                return Ok(None);
            };
            if let Some(status) = patch.status {
                member.status = status;
            }
            if let Some(admin) = patch.admin {
                member
                    .roles
                    .retain(|role| role != gateway_core::PORTAL_ROLE_ADMIN);
                if admin {
                    member
                        .roles
                        .push(gateway_core::PORTAL_ROLE_ADMIN.to_owned());
                }
            }
            member.updated_at = Utc::now();
            Ok(Some(member.clone()))
        }

        async fn list_service_memberships(
            &self,
            member_id: Uuid,
        ) -> GatewayResult<Vec<ServiceMembership>> {
            Ok(self
                .service_memberships
                .lock()
                .expect("lock poisoned")
                .iter()
                .filter(|membership| membership.member_id == member_id)
                .cloned()
                .collect())
        }

        async fn upsert_service_membership(
            &self,
            member_id: Uuid,
            request: ServiceMembershipUpsertRequest,
        ) -> GatewayResult<ServiceMembership> {
            if self.get_member(member_id).await?.is_none()
                || !self
                    .services
                    .lock()
                    .expect("lock poisoned")
                    .iter()
                    .any(|service| service.name == request.service_name)
            {
                return Err(GatewayError::InvalidAccessPayload);
            }
            let now = Utc::now();
            let mut memberships = self.service_memberships.lock().expect("lock poisoned");
            if let Some(membership) = memberships.iter_mut().find(|membership| {
                membership.member_id == member_id && membership.service_name == request.service_name
            }) {
                membership.role = request.role;
                membership.updated_at = now;
                return Ok(membership.clone());
            }
            let membership = ServiceMembership {
                member_id,
                service_name: request.service_name,
                role: request.role,
                created_at: now,
                updated_at: now,
            };
            memberships.push(membership.clone());
            Ok(membership)
        }

        async fn delete_service_membership(
            &self,
            member_id: Uuid,
            service_name: &str,
        ) -> GatewayResult<bool> {
            let mut memberships = self.service_memberships.lock().expect("lock poisoned");
            let before = memberships.len();
            memberships.retain(|membership| {
                membership.member_id != member_id || membership.service_name != service_name
            });
            Ok(memberships.len() != before)
        }

        async fn list_managed_identities(
            &self,
        ) -> GatewayResult<Vec<gateway_core::ManagedIdentityBinding>> {
            Ok(self
                .managed_identities
                .lock()
                .expect("lock poisoned")
                .clone())
        }

        async fn create_managed_identity(
            &self,
            request: ManagedIdentityCreateRequest,
        ) -> GatewayResult<gateway_core::ManagedIdentityBinding> {
            let now = Utc::now();
            let identity = gateway_core::ManagedIdentityBinding {
                id: Uuid::new_v4(),
                tenant_id: request.tenant_id,
                client_id: request.client_id,
                object_id: request.object_id,
                display_name: request.display_name,
                service_name: request.service_name,
                required_role: request.required_role,
                enabled: request.enabled,
                created_at: now,
                updated_at: now,
            };
            self.managed_identities
                .lock()
                .expect("lock poisoned")
                .push(identity.clone());
            Ok(identity)
        }

        async fn patch_managed_identity(
            &self,
            identity_id: Uuid,
            patch: ManagedIdentityPatchRequest,
        ) -> GatewayResult<Option<gateway_core::ManagedIdentityBinding>> {
            let mut identities = self.managed_identities.lock().expect("lock poisoned");
            let Some(identity) = identities
                .iter_mut()
                .find(|identity| identity.id == identity_id)
            else {
                return Ok(None);
            };
            if let Some(value) = patch.display_name {
                identity.display_name = value;
            }
            if let Some(value) = patch.object_id {
                identity.object_id = value;
            }
            if let Some(value) = patch.service_name {
                identity.service_name = value;
            }
            if let Some(value) = patch.required_role {
                identity.required_role = value;
            }
            if let Some(value) = patch.enabled {
                identity.enabled = value;
            }
            identity.updated_at = Utc::now();
            Ok(Some(identity.clone()))
        }

        async fn delete_managed_identity(&self, identity_id: Uuid) -> GatewayResult<bool> {
            let mut identities = self.managed_identities.lock().expect("lock poisoned");
            let before = identities.len();
            identities.retain(|identity| identity.id != identity_id);
            Ok(identities.len() != before)
        }

        async fn create_oidc_login_transaction(
            &self,
            transaction: OidcLoginTransaction,
        ) -> GatewayResult<()> {
            let mut transactions = self.oidc_transactions.lock().expect("lock poisoned");
            transactions.retain(|stored| stored.expires_at > Utc::now());
            transactions.push(transaction);
            Ok(())
        }

        async fn consume_oidc_login_transaction(
            &self,
            state_hash: &str,
            binding_hash: &str,
            now: chrono::DateTime<Utc>,
        ) -> GatewayResult<Option<OidcLoginTransaction>> {
            let mut transactions = self.oidc_transactions.lock().expect("lock poisoned");
            let Some(position) = transactions.iter().position(|transaction| {
                transaction.state_hash == state_hash
                    && transaction.binding_hash == binding_hash
                    && transaction.expires_at > now
            }) else {
                return Ok(None);
            };
            Ok(Some(transactions.remove(position)))
        }

        async fn create_portal_session(&self, session: NewPortalSession) -> GatewayResult<()> {
            self.portal_sessions
                .lock()
                .expect("lock poisoned")
                .push(session);
            Ok(())
        }

        async fn resolve_portal_session(
            &self,
            session_hash: &str,
            now: chrono::DateTime<Utc>,
        ) -> GatewayResult<Option<gateway_core::StoredPortalSession>> {
            let session = self
                .portal_sessions
                .lock()
                .expect("lock poisoned")
                .iter()
                .find(|session| session.session_hash == session_hash && session.expires_at > now)
                .cloned();
            let Some(session) = session else {
                return Ok(None);
            };
            let Some(member) = self.get_member(session.member_id).await? else {
                return Ok(None);
            };
            Ok(Some(gateway_core::StoredPortalSession {
                session_hash: session.session_hash,
                member,
                csrf_hash: session.csrf_hash,
                expires_at: session.expires_at,
                last_seen_at: now,
            }))
        }

        async fn delete_portal_session(&self, session_hash: &str) -> GatewayResult<bool> {
            let mut sessions = self.portal_sessions.lock().expect("lock poisoned");
            let before = sessions.len();
            sessions.retain(|session| session.session_hash != session_hash);
            Ok(sessions.len() != before)
        }

        async fn member_service_role(
            &self,
            member_id: Uuid,
            service_name: &str,
        ) -> GatewayResult<Option<ServiceMemberRole>> {
            let active = self
                .get_member(member_id)
                .await?
                .is_some_and(|member| member.status == MemberStatus::Active);
            if !active {
                return Ok(None);
            }
            Ok(self
                .service_memberships
                .lock()
                .expect("lock poisoned")
                .iter()
                .find(|membership| {
                    membership.member_id == member_id && membership.service_name == service_name
                })
                .map(|membership| membership.role))
        }

        async fn workload_service_binding(
            &self,
            tenant_id: &str,
            client_id: &str,
            object_id: Option<&str>,
            service_name: &str,
            token_roles: &[String],
        ) -> GatewayResult<Option<gateway_core::ManagedIdentityBinding>> {
            Ok(self
                .managed_identities
                .lock()
                .expect("lock poisoned")
                .iter()
                .find(|identity| {
                    identity.enabled
                        && identity.tenant_id == tenant_id
                        && identity.client_id == client_id
                        && identity.service_name == service_name
                        && identity
                            .object_id
                            .as_deref()
                            .is_none_or(|value| Some(value) == object_id)
                        && token_roles
                            .iter()
                            .any(|role| role == &identity.required_role)
                })
                .cloned())
        }
    }

    #[async_trait]
    impl ProviderConfigLookup for MemoryStore {
        async fn active_litellm_config(
            &self,
        ) -> GatewayResult<Option<gateway_core::ProviderRuntimeConfig>> {
            Ok(None)
        }

        async fn litellm_credential_mapping_for_context(
            &self,
            _key_id: Uuid,
            _project_id: Option<Uuid>,
        ) -> GatewayResult<Option<gateway_core::LiteLlmCredentialMappingRuntime>> {
            Ok(None)
        }
    }

    #[async_trait]
    impl AdminAuditStore for MemoryStore {
        async fn record_audit_event(&self, event: AuditEventCreate) -> GatewayResult<AuditEvent> {
            let audit_event = AuditEvent {
                id: Uuid::new_v4(),
                actor_token_id: event.actor_token_id,
                actor_member_id: event.actor_member_id,
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
                    .is_none_or(|actor_token_id| event.actor_token_id == Some(actor_token_id))
                    && query.actor_member_id.is_none_or(|actor_member_id| {
                        event.actor_member_id == Some(actor_member_id)
                    })
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
    impl AdminGatewayAuthSettingsStore for MemoryStore {
        async fn gateway_auth_settings(&self) -> GatewayResult<Option<StoredGatewayAuthSettings>> {
            Ok(self
                .gateway_auth_settings
                .lock()
                .expect("lock poisoned")
                .clone())
        }

        async fn patch_gateway_auth_settings(
            &self,
            patch: GatewayAuthSettingsPatchRequest,
        ) -> GatewayResult<StoredGatewayAuthSettings> {
            let mut stored = self.gateway_auth_settings.lock().expect("lock poisoned");
            let mut settings = stored.clone().unwrap_or_default().apply_patch(patch)?;
            settings.updated_at = Some(Utc::now());
            *stored = Some(settings.clone());
            Ok(settings)
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
                        "/v1/embeddings" => Some(Route::LiteLlmEmbeddings),
                        "/v1/messages" => Some(Route::AnthropicMessages),
                        "/v1/messages/count_tokens" => Some(Route::AnthropicMessagesCountTokens),
                        "/v1/messages/batches" => Some(Route::AnthropicMessageBatches),
                        "/v1/messages/batches/*" => Some(Route::AnthropicMessageBatch),
                        "/v1/messages/batches/*/results" => {
                            Some(Route::AnthropicMessageBatchResults)
                        }
                        "/v1/messages/batches/*/cancel" => Some(Route::AnthropicMessageBatchCancel),
                        "/v1/models" => Some(Route::AnthropicModels),
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

        async fn list_anthropic_route_settings(&self) -> GatewayResult<Vec<OpenAiRouteSetting>> {
            Ok(self.anthropic_routes.lock().expect("lock poisoned").clone())
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

        async fn set_openai_route_mode(
            &self,
            route_id: &str,
            mode: OpenAiRouteMode,
        ) -> GatewayResult<Option<OpenAiRouteSetting>> {
            let mut routes = self.openai_routes.lock().expect("lock poisoned");
            let Some(route) = routes.iter_mut().find(|route| route.route_id == route_id) else {
                return Ok(None);
            };
            route.mode = mode;
            route.updated_at = Utc::now();
            Ok(Some(route.clone()))
        }

        async fn patch_openai_route_config(
            &self,
            route_id: &str,
            patch: OpenAiRouteConfigPatchRequest,
        ) -> GatewayResult<Option<OpenAiRouteSetting>> {
            patch_route_config(&self.openai_routes, route_id, patch)
        }

        async fn set_anthropic_route_enabled(
            &self,
            route_id: &str,
            enabled: bool,
        ) -> GatewayResult<Option<OpenAiRouteSetting>> {
            let mut routes = self.anthropic_routes.lock().expect("lock poisoned");
            let Some(route) = routes.iter_mut().find(|route| route.route_id == route_id) else {
                return Ok(None);
            };
            route.enabled = enabled;
            route.updated_at = Utc::now();
            Ok(Some(route.clone()))
        }

        async fn set_anthropic_route_mode(
            &self,
            route_id: &str,
            mode: OpenAiRouteMode,
        ) -> GatewayResult<Option<OpenAiRouteSetting>> {
            let mut routes = self.anthropic_routes.lock().expect("lock poisoned");
            let Some(route) = routes.iter_mut().find(|route| route.route_id == route_id) else {
                return Ok(None);
            };
            route.mode = mode;
            route.updated_at = Utc::now();
            Ok(Some(route.clone()))
        }

        async fn patch_anthropic_route_config(
            &self,
            route_id: &str,
            patch: OpenAiRouteConfigPatchRequest,
        ) -> GatewayResult<Option<OpenAiRouteSetting>> {
            patch_route_config(&self.anthropic_routes, route_id, patch)
        }

        async fn get_litellm_passthrough_settings(
            &self,
        ) -> GatewayResult<LiteLlmPassthroughSettings> {
            Ok(self
                .litellm_passthrough_settings
                .lock()
                .expect("lock poisoned")
                .clone())
        }

        async fn patch_litellm_passthrough_settings(
            &self,
            patch: LiteLlmPassthroughSettingsPatchRequest,
        ) -> GatewayResult<LiteLlmPassthroughSettings> {
            patch.validate()?;
            let mut settings = self
                .litellm_passthrough_settings
                .lock()
                .expect("lock poisoned");
            if let Some(enabled) = patch.enabled {
                settings.enabled = enabled;
            }
            if let Some(paths) = patch.allowed_paths {
                settings.allowed_paths = paths;
            }
            if let Some(methods) = patch.allowed_methods {
                settings.allowed_methods = methods
                    .into_iter()
                    .map(|method| method.trim().to_ascii_uppercase())
                    .collect();
            }
            if let Some(exposure) = patch.ui_exposure {
                settings.ui_exposure = exposure;
            }
            if let Some(exposure) = patch.admin_api_exposure {
                settings.admin_api_exposure = exposure;
            }
            if let Some(timeout_ms) = patch.timeout_ms {
                settings.timeout_ms = timeout_ms;
            }
            if let Some(max_request_body_bytes) = patch.max_request_body_bytes {
                settings.max_request_body_bytes = max_request_body_bytes;
            }
            if let Some(max_response_body_bytes) = patch.max_response_body_bytes {
                settings.max_response_body_bytes = max_response_body_bytes;
            }
            settings.updated_at = Utc::now();
            Ok(settings.clone())
        }
    }

    fn patch_route_config(
        routes: &Arc<Mutex<Vec<OpenAiRouteSetting>>>,
        route_id: &str,
        patch: OpenAiRouteConfigPatchRequest,
    ) -> GatewayResult<Option<OpenAiRouteSetting>> {
        patch.validate()?;
        let mut routes = routes.lock().expect("lock poisoned");
        let Some(route) = routes.iter_mut().find(|route| route.route_id == route_id) else {
            return Ok(None);
        };
        if let Some(mode) = patch.mode {
            route.mode = mode;
        }
        if let Some(timeout_ms) = patch.timeout_ms {
            route.timeout_ms = timeout_ms;
        }
        if let Some(max_request_body_bytes) = patch.max_request_body_bytes {
            route.max_request_body_bytes = max_request_body_bytes;
        }
        if let Some(max_response_body_bytes) = patch.max_response_body_bytes {
            route.max_response_body_bytes = max_response_body_bytes;
        }
        route.updated_at = Utc::now();
        Ok(Some(route.clone()))
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
                credential_header_mode: request.credential_header_mode,
                credential_header_name: request.credential_header_name,
                credential_header_value_format: request.credential_header_value_format,
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

        async fn upsert_litellm_credential_mapping(
            &self,
            request: LiteLlmCredentialMappingUpsertRequest,
        ) -> GatewayResult<LiteLlmCredentialMappingResponse> {
            if request
                .credential
                .as_deref()
                .is_some_and(|value| value.trim().is_empty())
            {
                return Err(GatewayError::InvalidProviderConfigPayload);
            }
            let now = Utc::now();
            Ok(LiteLlmCredentialMappingResponse {
                id: Uuid::new_v4(),
                scope: request.scope,
                target_id: request.target_id,
                target_label: None,
                enabled: request.enabled,
                credential_configured: request.credential.is_some(),
                created_at: now,
                updated_at: now,
            })
        }

        async fn list_litellm_credential_mappings(
            &self,
        ) -> GatewayResult<Vec<LiteLlmCredentialMappingResponse>> {
            Ok(Vec::new())
        }

        async fn delete_litellm_credential_mapping(
            &self,
            _mapping_id: Uuid,
        ) -> GatewayResult<bool> {
            Ok(false)
        }

        async fn set_litellm_credential_mapping_enabled(
            &self,
            _mapping_id: Uuid,
            _enabled: bool,
        ) -> GatewayResult<Option<LiteLlmCredentialMappingResponse>> {
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
                health_check_path: request.health_check_path.clone(),
                health_check_method: request.health_check_method.clone(),
                enabled: request.enabled,
                allowed_methods: request.allowed_methods.clone(),
                credential_configured: request.credential.is_some(),
                timeout_ms: request.timeout_ms,
                max_body_bytes: request.max_body_bytes,
                cost_mode: request.cost_mode,
                estimated_cost_usd: request.estimated_cost_usd,
                pricing_rules: request.pricing_rules.clone(),
                openapi_source_path: request.openapi_source_path.clone(),
                openapi_schema_hash: None,
                openapi_synced_at: None,
                openapi_endpoints: request.openapi_endpoints.clone(),
                endpoint_pricing_rules: request.endpoint_pricing_rules.clone(),
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
            if name == "store-error" {
                return Err(GatewayError::StoreUnavailable);
            }
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
            if let Some(health_check_path) = patch.health_check_path {
                service.health_check_path = health_check_path;
            }
            if let Some(health_check_method) = patch.health_check_method {
                service.health_check_method = health_check_method;
            }
            if let Some(allowed_methods) = patch.allowed_methods {
                service.allowed_methods = allowed_methods;
            }
            if let Some(timeout_ms) = patch.timeout_ms {
                service.timeout_ms = timeout_ms;
            }
            if let Some(cost_mode) = patch.cost_mode {
                service.cost_mode = cost_mode;
            }
            if let Some(estimated_cost_usd) = patch.estimated_cost_usd {
                service.estimated_cost_usd = estimated_cost_usd;
            }
            if let Some(pricing_rules) = patch.pricing_rules {
                service.pricing_rules = pricing_rules;
            }
            if let Some(openapi_source_path) = patch.openapi_source_path {
                service.openapi_source_path = openapi_source_path;
            }
            if let Some(openapi_schema_hash) = patch.openapi_schema_hash {
                service.openapi_schema_hash = openapi_schema_hash;
            }
            if let Some(openapi_synced_at) = patch.openapi_synced_at {
                service.openapi_synced_at = openapi_synced_at;
            }
            if let Some(openapi_endpoints) = patch.openapi_endpoints {
                service.openapi_endpoints = openapi_endpoints;
            }
            if let Some(endpoint_pricing_rules) = patch.endpoint_pricing_rules {
                service.endpoint_pricing_rules = endpoint_pricing_rules;
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
                if service.health_check_path.is_none() && request.health_check_path.is_some() {
                    service.health_check_path = request.health_check_path;
                    service.health_check_method = request.health_check_method;
                }
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
                health_check_path: request.health_check_path.clone(),
                health_check_method: request.health_check_method.clone(),
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
                pricing_rules: request
                    .default_pricing
                    .as_ref()
                    .map(|pricing| pricing.pricing_rules.clone())
                    .unwrap_or_default(),
                openapi_source_path: None,
                openapi_schema_hash: None,
                openapi_synced_at: None,
                openapi_endpoints: Vec::new(),
                endpoint_pricing_rules: Vec::new(),
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
                    member_id: None,
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
            query: UsageQuery,
        ) -> GatewayResult<Vec<UsageTimeseriesPoint>> {
            let mut export_query = query;
            export_query.limit = Some(10_000);
            export_query.offset = Some(0);
            let rows = self.usage_export(export_query.clone()).await?.rows;
            Ok(usage_timeseries_from_rows(
                &rows,
                export_query.interval.as_deref(),
            ))
        }

        async fn usage_breakdown(
            &self,
            mut query: UsageQuery,
            dimension: UsageBreakdownDimension,
        ) -> GatewayResult<Vec<UsageBreakdown>> {
            if dimension != UsageBreakdownDimension::Endpoint {
                return Ok(Vec::new());
            }
            let limit = query.breakdown_limit.unwrap_or(20).clamp(1, 500) as usize;
            query.limit = Some(10_000);
            query.offset = Some(0);
            let mut grouped = BTreeMap::<String, Vec<UsageExportRow>>::new();
            for row in self.usage_export(query).await?.rows {
                let Some(path) = row
                    .endpoint_template
                    .as_deref()
                    .or(row.endpoint_path.as_deref())
                else {
                    continue;
                };
                let name = format!("{} {path}", row.http_method.as_deref().unwrap_or("UNKNOWN"));
                grouped.entry(name).or_default().push(row);
            }
            let mut breakdowns = grouped
                .into_iter()
                .map(|(name, rows)| UsageBreakdown {
                    name,
                    summary: usage_summary_from_rows(&rows),
                })
                .collect::<Vec<_>>();
            breakdowns.sort_by(|left, right| {
                right
                    .summary
                    .request_count
                    .cmp(&left.summary.request_count)
                    .then_with(|| left.name.cmp(&right.name))
            });
            breakdowns.truncate(limit);
            Ok(breakdowns)
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
                    cost_source: event.cost_source.clone(),
                    cost_mode: event
                        .cost_mode
                        .map(|mode| serde_json::to_value(mode).unwrap_or_default())
                        .and_then(|value| value.as_str().map(ToOwned::to_owned)),
                    pricing_rule_name: event.pricing_rule_name.clone(),
                    service_name: event.service_name.clone(),
                    http_method: event.http_method.clone(),
                    endpoint_path: event.endpoint_path.clone(),
                    endpoint_template: event.endpoint_template.clone(),
                    task_id: event.task_id.clone(),
                    run_id: event.run_id.clone(),
                    trace_id: event.trace_id.clone(),
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

        async fn usage_dashboard(
            &self,
            query: UsageQuery,
        ) -> GatewayResult<gateway_core::UsageDashboard> {
            let mut export_query = query.clone();
            export_query.limit = Some(10_000);
            export_query.offset = Some(0);
            let export = self.usage_export(export_query).await?;
            let timeseries = paginate_usage_rows(
                usage_timeseries_from_rows(&export.rows, query.interval.as_deref()),
                query.timeseries_limit,
                query.timeseries_offset,
            );
            let service_timeseries = paginate_usage_rows(
                usage_service_timeseries_from_rows(&export.rows, query.interval.as_deref()),
                query.service_timeseries_limit,
                query.service_timeseries_offset,
            );
            Ok(gateway_core::UsageDashboard {
                summary: self.usage_summary(query.clone()).await?,
                breakdowns: gateway_core::UsageDashboardBreakdowns {
                    projects: self
                        .usage_breakdown(query.clone(), UsageBreakdownDimension::Project)
                        .await?,
                    keys: self
                        .usage_breakdown(query.clone(), UsageBreakdownDimension::Key)
                        .await?,
                    services: self
                        .usage_breakdown(query.clone(), UsageBreakdownDimension::Service)
                        .await?,
                    endpoints: self
                        .usage_breakdown(query.clone(), UsageBreakdownDimension::Endpoint)
                        .await?,
                    providers: self
                        .usage_breakdown(query.clone(), UsageBreakdownDimension::Provider)
                        .await?,
                    models: self
                        .usage_breakdown(query.clone(), UsageBreakdownDimension::Model)
                        .await?,
                    tasks: self
                        .usage_breakdown(query.clone(), UsageBreakdownDimension::Task)
                        .await?,
                },
                timeseries: timeseries.rows,
                service_timeseries: service_timeseries.rows,
                timeseries_page: timeseries.page,
                service_timeseries_page: service_timeseries.page,
                unused_keys: self.unused_keys(query).await?,
            })
        }

        async fn usage_events(
            &self,
            query: UsageQuery,
        ) -> GatewayResult<gateway_core::UsageEventsPage> {
            let limit = query.limit.unwrap_or(50).clamp(1, 500);
            let offset = query.offset.unwrap_or_default().max(0);
            let mut export_query = query;
            export_query.limit = Some(limit + 1);
            export_query.offset = Some(offset);
            let mut rows = self.usage_export(export_query).await?.rows;
            let has_more = rows.len() > limit as usize;
            rows.truncate(limit as usize);
            Ok(gateway_core::UsageEventsPage {
                rows,
                limit,
                offset,
                has_more,
            })
        }

        async fn usage_filter_values(
            &self,
            query: UsageFilterValuesQuery,
        ) -> GatewayResult<gateway_core::UsageFilterValues> {
            if !matches!(
                query.field.as_str(),
                "route" | "provider" | "service" | "endpoint" | "model" | "task_id" | "run_id"
            ) {
                return Err(GatewayError::InvalidUsageQuery);
            }
            let limit = query.usage.limit.unwrap_or(50).clamp(1, 100) as usize;
            let mut values = self
                .usage_export(query.usage)
                .await?
                .rows
                .into_iter()
                .filter_map(|row| match query.field.as_str() {
                    "route" => Some(row.route),
                    "provider" => Some(row.provider),
                    "service" => row.service_name,
                    "endpoint" => row.endpoint_template.or(row.endpoint_path),
                    "model" => row.model,
                    "task_id" => row.task_id,
                    "run_id" => row.run_id,
                    _ => None,
                })
                .filter(|value| {
                    query
                        .q
                        .as_deref()
                        .is_none_or(|prefix| value.starts_with(prefix))
                })
                .collect::<Vec<_>>();
            values.sort();
            values.dedup();
            values.truncate(limit);
            Ok(gateway_core::UsageFilterValues {
                field: query.field,
                values,
            })
        }

        async fn provider_health(&self, _query: UsageQuery) -> GatewayResult<Vec<ProviderHealth>> {
            Ok(Vec::new())
        }

        async fn unused_keys(
            &self,
            _query: UsageQuery,
        ) -> GatewayResult<Vec<gateway_core::UnusedKey>> {
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
        if query.method.as_deref().is_some_and(|method| {
            event.http_method.as_deref() != Some(method.to_ascii_uppercase().as_str())
        }) {
            return false;
        }
        if query.endpoint.as_deref().is_some_and(|endpoint| {
            event
                .endpoint_template
                .as_deref()
                .or(event.endpoint_path.as_deref())
                != Some(endpoint)
        }) {
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
        if query
            .status_code
            .is_some_and(|status_code| i32::from(event.status_code) != status_code)
        {
            return false;
        }
        true
    }

    fn usage_summary_from_rows(rows: &[UsageExportRow]) -> UsageSummary {
        let total_latency_ms = rows.iter().map(|row| row.latency_ms).sum();
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
            total_latency_ms,
            average_latency_ms: if rows.is_empty() {
                None
            } else {
                Some(total_latency_ms as f64 / rows.len() as f64)
            },
            fallback_count: rows.iter().map(|row| i64::from(row.fallback_count)).sum(),
            fallback_rate: if rows.is_empty() {
                0.0
            } else {
                rows.iter()
                    .map(|row| i64::from(row.fallback_count))
                    .sum::<i64>() as f64
                    / rows.len() as f64
            },
            ..UsageSummary::default()
        }
    }

    struct UsageRowsPage<T> {
        rows: Vec<T>,
        page: UsagePage,
    }

    fn paginate_usage_rows<T>(
        rows: Vec<T>,
        limit: Option<i64>,
        offset: Option<i64>,
    ) -> UsageRowsPage<T> {
        let Some(limit) = limit.map(|value| value.clamp(1, 500)) else {
            return UsageRowsPage {
                page: UsagePage {
                    limit: rows.len() as i64,
                    offset: 0,
                    has_more: false,
                },
                rows,
            };
        };
        let offset = offset.unwrap_or_default().max(0) as usize;
        let mut page_rows: Vec<T> = rows
            .into_iter()
            .skip(offset)
            .take(limit as usize + 1)
            .collect();
        let has_more = page_rows.len() > limit as usize;
        page_rows.truncate(limit as usize);
        UsageRowsPage {
            rows: page_rows,
            page: UsagePage {
                limit,
                offset: offset as i64,
                has_more,
            },
        }
    }

    fn usage_timeseries_from_rows(
        rows: &[UsageExportRow],
        interval: Option<&str>,
    ) -> Vec<UsageTimeseriesPoint> {
        let bucket_seconds = if interval == Some("day") {
            86_400
        } else {
            3_600
        };
        let mut grouped = BTreeMap::<chrono::DateTime<Utc>, Vec<UsageExportRow>>::new();
        for row in rows {
            let timestamp = row.created_at.timestamp();
            let bucket_timestamp = timestamp.div_euclid(bucket_seconds) * bucket_seconds;
            let bucket =
                chrono::DateTime::from_timestamp(bucket_timestamp, 0).expect("valid bucket");
            grouped.entry(bucket).or_default().push(row.clone());
        }
        grouped
            .into_iter()
            .map(|(bucket, rows)| UsageTimeseriesPoint {
                bucket,
                summary: usage_summary_from_rows(&rows),
            })
            .collect()
    }

    fn usage_service_timeseries_from_rows(
        rows: &[UsageExportRow],
        interval: Option<&str>,
    ) -> Vec<UsageServiceTimeseriesPoint> {
        let bucket_seconds = if interval == Some("day") {
            86_400
        } else {
            3_600
        };
        let mut grouped = BTreeMap::<(chrono::DateTime<Utc>, String), Vec<UsageExportRow>>::new();
        for row in rows {
            let timestamp = row.created_at.timestamp();
            let bucket_timestamp = timestamp.div_euclid(bucket_seconds) * bucket_seconds;
            let bucket =
                chrono::DateTime::from_timestamp(bucket_timestamp, 0).expect("valid bucket");
            grouped
                .entry((
                    bucket,
                    row.service_name
                        .clone()
                        .unwrap_or_else(|| "none".to_owned()),
                ))
                .or_default()
                .push(row.clone());
        }
        grouped
            .into_iter()
            .map(
                |((bucket, service_name), rows)| UsageServiceTimeseriesPoint {
                    bucket,
                    service_name,
                    summary: usage_summary_from_rows(&rows),
                },
            )
            .collect()
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
        let auth_env = GatewayAuthEnv::default();
        let auth_runtime = SharedGatewayAuthRuntime::new(
            EffectiveGatewayAuthSettings::from_sources(None, &auth_env)
                .expect("effective auth settings")
                .runtime_config(),
        )
        .expect("auth runtime");
        AppState {
            store: Arc::new(store),
            redis,
            studio_env: StudioConnectionEnv::default(),
            auth_env,
            auth_runtime,
            litellm_base_url: DEFAULT_LITELLM_BASE_URL.to_owned(),
            litellm_service_key: "test-litellm-service-key".to_owned(),
            litellm_ui_client: litellm_ui_client(),
            portal_oidc: None,
            owner_entra_verifier: None,
        }
    }

    fn test_state_with_studio_env(store: MemoryStore, studio_env: StudioConnectionEnv) -> AppState {
        let redis = RedisReadiness::new("redis://127.0.0.1:6379").expect("redis client");
        let auth_env = GatewayAuthEnv::default();
        let auth_runtime = SharedGatewayAuthRuntime::new(
            EffectiveGatewayAuthSettings::from_sources(None, &auth_env)
                .expect("effective auth settings")
                .runtime_config(),
        )
        .expect("auth runtime");
        AppState {
            store: Arc::new(store),
            redis,
            studio_env,
            auth_env,
            auth_runtime,
            litellm_base_url: DEFAULT_LITELLM_BASE_URL.to_owned(),
            litellm_service_key: "test-litellm-service-key".to_owned(),
            litellm_ui_client: litellm_ui_client(),
            portal_oidc: None,
            owner_entra_verifier: None,
        }
    }

    fn default_store() -> MemoryStore {
        MemoryStore {
            key: Arc::new(Mutex::new(None)),
            admin_key: Arc::new(Mutex::new(None)),
            services: Arc::new(Mutex::new(Vec::new())),
            openai_routes: Arc::new(Mutex::new(default_openai_routes())),
            anthropic_routes: Arc::new(Mutex::new(default_anthropic_routes())),
            operator_tokens: Arc::new(Mutex::new(vec![TEST_OPERATOR_TOKEN.to_owned()])),
            events: Arc::new(Mutex::new(Vec::new())),
            audit_events: Arc::new(Mutex::new(Vec::new())),
            studio_connection: Arc::new(Mutex::new(None)),
            gateway_auth_settings: Arc::new(Mutex::new(None)),
            litellm_passthrough_settings: Arc::new(Mutex::new(
                LiteLlmPassthroughSettings::default_with_updated_at(Utc::now()),
            )),
            portal_members: Arc::new(Mutex::new(Vec::new())),
            service_memberships: Arc::new(Mutex::new(Vec::new())),
            managed_identities: Arc::new(Mutex::new(Vec::new())),
            oidc_transactions: Arc::new(Mutex::new(Vec::new())),
            portal_sessions: Arc::new(Mutex::new(Vec::new())),
            postgres_ready: true,
        }
    }

    fn default_openai_routes() -> Vec<OpenAiRouteSetting> {
        let now = Utc::now();
        vec![
            test_route_setting("chat-completions", "/v1/chat/completions", now),
            test_route_setting("responses", "/v1/responses", now),
            test_route_setting("embeddings", "/v1/embeddings", now),
        ]
    }

    fn default_anthropic_routes() -> Vec<OpenAiRouteSetting> {
        let now = Utc::now();
        vec![
            test_route_setting("messages", "/v1/messages", now),
            test_route_setting("messages-count-tokens", "/v1/messages/count_tokens", now),
            test_route_setting("message-batches", "/v1/messages/batches", now),
            test_route_setting("message-batch", "/v1/messages/batches/*", now),
            test_route_setting(
                "message-batch-results",
                "/v1/messages/batches/*/results",
                now,
            ),
            test_route_setting("message-batch-cancel", "/v1/messages/batches/*/cancel", now),
            test_route_setting("models", "/v1/models", now),
        ]
    }

    fn test_route_setting(
        route_id: &str,
        route: &str,
        updated_at: chrono::DateTime<Utc>,
    ) -> OpenAiRouteSetting {
        OpenAiRouteSetting {
            route_id: route_id.to_owned(),
            route: route.to_owned(),
            enabled: true,
            mode: OpenAiRouteMode::ManagedByGateway,
            timeout_ms: gateway_core::DEFAULT_LITELLM_ROUTE_TIMEOUT_MS,
            max_request_body_bytes: gateway_core::DEFAULT_LITELLM_ROUTE_REQUEST_BODY_BYTES,
            max_response_body_bytes: gateway_core::DEFAULT_LITELLM_ROUTE_RESPONSE_BODY_BYTES,
            updated_at,
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

    async fn portal_request(
        app: Router,
        method: axum::http::Method,
        route: &str,
        raw_session: &str,
        raw_csrf: &str,
        csrf_header: Option<&str>,
        body: &str,
    ) -> Response {
        let mut builder = axum::http::Request::builder()
            .method(method)
            .uri(route)
            .header("x-request-id", "req_portal_test")
            .header(
                "cookie",
                format!("{PORTAL_SESSION_COOKIE}={raw_session}; {PORTAL_CSRF_COOKIE}={raw_csrf}"),
            );
        if !body.is_empty() {
            builder = builder.header("content-type", "application/json");
        }
        if let Some(csrf) = csrf_header {
            builder = builder.header("x-csrf-token", csrf);
        }
        app.oneshot(
            builder
                .body(axum::body::Body::from(body.to_owned()))
                .expect("request"),
        )
        .await
        .expect("response")
    }

    fn active_portal_member(admin: bool) -> PortalMember {
        let now = Utc::now();
        PortalMember {
            id: Uuid::new_v4(),
            tenant_id: "tenant-test".to_owned(),
            object_id: Uuid::new_v4().to_string(),
            email: Some("owner@relayna.test".to_owned()),
            display_name: Some("Relayna Owner".to_owned()),
            status: MemberStatus::Active,
            roles: if admin {
                vec![gateway_core::PORTAL_ROLE_ADMIN.to_owned()]
            } else {
                Vec::new()
            },
            last_sign_in_at: Some(now),
            created_at: now,
            updated_at: now,
        }
    }

    fn seed_portal_session(store: &MemoryStore, member: PortalMember) -> (String, String) {
        let raw_session = random_opaque_token();
        let raw_csrf = random_opaque_token();
        store
            .portal_members
            .lock()
            .expect("lock poisoned")
            .push(member.clone());
        store
            .portal_sessions
            .lock()
            .expect("lock poisoned")
            .push(NewPortalSession {
                session_hash: token_hash(&raw_session),
                member_id: member.id,
                csrf_hash: token_hash(&raw_csrf),
                expires_at: Utc::now() + ChronoDuration::hours(1),
            });
        (raw_session, raw_csrf)
    }

    struct DevOidcProcess(Child);

    impl Drop for DevOidcProcess {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    async fn start_development_oidc() -> (DevOidcProcess, String) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("reserve OIDC port");
        let port = listener.local_addr().expect("OIDC address").port();
        drop(listener);
        let issuer = format!("http://127.0.0.1:{port}");
        let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../scripts/entra/development-oidc.mjs");
        let child = Command::new("node")
            .arg(script)
            .env("RELAYNA_DEV_OIDC_PORT", port.to_string())
            .env("RELAYNA_DEV_OIDC_ISSUER", &issuer)
            .env(
                "RELAYNA_DEV_OIDC_BROWSER_REDIRECT_URI",
                "http://127.0.0.1:18381/admin-ui/auth/callback",
            )
            .env(
                "RELAYNA_DEV_OIDC_BROWSER_POST_LOGOUT_REDIRECT_URI",
                "http://127.0.0.1:18381/admin-ui",
            )
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("start development OIDC");
        let client = reqwest::Client::new();
        for _ in 0..200 {
            if client
                .get(format!("{issuer}/health"))
                .send()
                .await
                .is_ok_and(|response| response.status().is_success())
            {
                return (DevOidcProcess(child), issuer);
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        let mut child = child;
        let _ = child.kill();
        panic!("development OIDC did not become ready");
    }

    async fn response_json(response: Response) -> serde_json::Value {
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        serde_json::from_slice(&body).expect("json")
    }

    #[derive(Debug)]
    struct CapturedHttpRequest {
        request_line: String,
        headers: Vec<(String, String)>,
        body: Vec<u8>,
    }

    fn test_state_with_litellm(
        store: MemoryStore,
        litellm_base_url: String,
        litellm_service_key: &str,
    ) -> AppState {
        let mut state = test_state(store);
        state.litellm_base_url = litellm_base_url;
        state.litellm_service_key = litellm_service_key.to_owned();
        state
    }

    fn spawn_litellm_server(
        status: &str,
        response_headers: Vec<(&str, &str)>,
        response_body: &str,
    ) -> (String, mpsc::Receiver<CapturedHttpRequest>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock LiteLLM");
        let addr = listener.local_addr().expect("mock address");
        let (tx, rx) = mpsc::channel();
        let status = status.to_owned();
        let response_headers: Vec<(String, String)> = response_headers
            .into_iter()
            .map(|(name, value)| (name.to_owned(), value.to_owned()))
            .collect();
        let response_body = response_body.as_bytes().to_vec();
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept mock request");
            let mut buffer = Vec::new();
            let mut chunk = [0_u8; 1024];
            loop {
                let read = stream.read(&mut chunk).expect("read request");
                if read == 0 {
                    break;
                }
                buffer.extend_from_slice(&chunk[..read]);
                if buffer.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }
            let header_end = buffer
                .windows(4)
                .position(|window| window == b"\r\n\r\n")
                .map(|position| position + 4)
                .expect("request headers");
            let header_text = String::from_utf8_lossy(&buffer[..header_end]);
            let mut lines = header_text.split("\r\n");
            let request_line = lines.next().unwrap_or_default().to_owned();
            let headers: Vec<(String, String)> = lines
                .filter_map(|line| line.split_once(':'))
                .map(|(name, value)| (name.trim().to_owned(), value.trim().to_owned()))
                .collect();
            let content_length = headers
                .iter()
                .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                .and_then(|(_, value)| value.parse::<usize>().ok())
                .unwrap_or(0);
            while buffer.len().saturating_sub(header_end) < content_length {
                let read = stream.read(&mut chunk).expect("read request body");
                if read == 0 {
                    break;
                }
                buffer.extend_from_slice(&chunk[..read]);
            }
            let body = buffer[header_end..].to_vec();
            tx.send(CapturedHttpRequest {
                request_line,
                headers,
                body,
            })
            .expect("send captured request");

            let mut response = format!(
                "HTTP/1.1 {status}\r\nContent-Length: {}\r\n",
                response_body.len()
            );
            for (name, value) in response_headers {
                response.push_str(&format!("{name}: {value}\r\n"));
            }
            response.push_str("\r\n");
            stream
                .write_all(response.as_bytes())
                .expect("write headers");
            stream.write_all(&response_body).expect("write body");
        });
        (format!("http://{addr}"), rx)
    }

    fn openapi_test_service(upstream_base_url: String) -> ServiceResponse {
        let now = Utc::now();
        ServiceResponse {
            name: "ocr".to_owned(),
            project_id: None,
            studio_service_id: None,
            route_pattern: "/services/ocr/*".to_owned(),
            upstream_base_url: Some(upstream_base_url),
            health_check_path: Some("/health".to_owned()),
            health_check_method: "GET".to_owned(),
            enabled: true,
            allowed_methods: vec!["GET".to_owned(), "POST".to_owned()],
            credential_configured: true,
            timeout_ms: 60_000,
            max_body_bytes: 100_000_000,
            cost_mode: ServiceCostMode::Fixed,
            estimated_cost_usd: Some(0.01),
            pricing_rules: Vec::new(),
            openapi_source_path: None,
            openapi_schema_hash: None,
            openapi_synced_at: None,
            openapi_endpoints: Vec::new(),
            endpoint_pricing_rules: Vec::new(),
            fallback_services: Vec::new(),
            source: ServiceSource::Gateway,
            sync_status: ServiceSyncStatus::Local,
            last_synced_at: None,
            disabled_at: None,
            created_at: now,
            updated_at: now,
            missing_runtime_fields: Vec::new(),
        }
    }

    #[tokio::test]
    async fn service_openapi_fetch_discovers_relayna_defaults_without_credentials() {
        let document = serde_json::json!({
            "openapi": "3.1.0",
            "info": {"title": "OCR Service API", "version": "0.1.0"},
            "paths": {
                "/ocr": {"post": {"operationId": "submit_ocr", "summary": "Submit OCR"}},
                "/events/{task_id}": {"get": {"operationId": "events"}},
                "/relayna/health/workers": {"get": {"operationId": "workers"}},
                "/health": {"get": {"operationId": "health"}}
            }
        })
        .to_string();
        let (base_url, captured) = spawn_litellm_server(
            "200 OK",
            vec![("content-type", "application/json")],
            &document,
        );
        let preview = fetch_service_openapi(&openapi_test_service(base_url), "/openapi.json")
            .await
            .expect("OpenAPI preview");

        assert_eq!(preview.title.as_deref(), Some("OCR Service API"));
        assert_eq!(preview.endpoints.len(), 4);
        assert!(
            !preview
                .endpoints
                .iter()
                .find(|endpoint| endpoint.path_template == "/ocr")
                .expect("OCR endpoint")
                .relayna_default
        );
        assert!(preview
            .endpoints
            .iter()
            .filter(|endpoint| endpoint.path_template != "/ocr")
            .all(|endpoint| endpoint.relayna_default));
        let request = captured.recv().expect("captured request");
        assert_eq!(request.request_line, "GET /openapi.json HTTP/1.1");
        assert!(!request
            .headers
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case("authorization")));
    }

    #[tokio::test]
    async fn service_openapi_fetch_does_not_follow_redirects() {
        let (base_url, _captured) = spawn_litellm_server(
            "302 Found",
            vec![("location", "https://evil.example/openapi.json")],
            "",
        );
        let error = fetch_service_openapi(&openapi_test_service(base_url), "/openapi.json")
            .await
            .unwrap_err();
        assert_eq!(error, GatewayError::ServiceOpenApiUnavailable);
    }

    #[tokio::test]
    async fn service_openapi_fetch_rejects_non_json_content() {
        let (base_url, _captured) = spawn_litellm_server(
            "200 OK",
            vec![("content-type", "text/html")],
            "<html>Swagger UI</html>",
        );
        let error = fetch_service_openapi(&openapi_test_service(base_url), "/docs")
            .await
            .unwrap_err();
        assert_eq!(error, GatewayError::InvalidServiceOpenApi);
    }

    #[tokio::test]
    async fn service_openapi_fetch_rejects_upstream_url_credentials() {
        let error = fetch_service_openapi(
            &openapi_test_service("http://operator:secret@127.0.0.1:9".to_owned()),
            "/openapi.json",
        )
        .await
        .unwrap_err();
        assert_eq!(error, GatewayError::InvalidServiceUpstream);
    }

    fn captured_header<'a>(request: &'a CapturedHttpRequest, name: &str) -> Option<&'a str> {
        request
            .headers
            .iter()
            .find(|(header, _)| header.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    #[tokio::test]
    async fn healthz_returns_ok() {
        let store = MemoryStore {
            key: Arc::new(Mutex::new(None)),
            admin_key: Arc::new(Mutex::new(None)),
            services: Arc::new(Mutex::new(Vec::new())),
            openai_routes: Arc::new(Mutex::new(default_openai_routes())),
            anthropic_routes: Arc::new(Mutex::new(default_anthropic_routes())),
            operator_tokens: Arc::new(Mutex::new(vec![TEST_OPERATOR_TOKEN.to_owned()])),
            events: Arc::new(Mutex::new(Vec::new())),
            audit_events: Arc::new(Mutex::new(Vec::new())),
            portal_members: Arc::new(Mutex::new(Vec::new())),
            service_memberships: Arc::new(Mutex::new(Vec::new())),
            managed_identities: Arc::new(Mutex::new(Vec::new())),
            oidc_transactions: Arc::new(Mutex::new(Vec::new())),
            portal_sessions: Arc::new(Mutex::new(Vec::new())),
            postgres_ready: true,
            studio_connection: Arc::new(Mutex::new(None)),
            gateway_auth_settings: Arc::new(Mutex::new(None)),
            litellm_passthrough_settings: Arc::new(Mutex::new(
                LiteLlmPassthroughSettings::default_with_updated_at(Utc::now()),
            )),
        };

        let app = router_with_state(test_state_with_redis_url(store, "redis://127.0.0.1:0"));
        let response = request(app, "/admin-ui/healthz").await;

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn portal_session_reports_pending_member_without_granting_admin_access() {
        let store = default_store();
        let mut member = active_portal_member(false);
        member.status = MemberStatus::Pending;
        let (raw_session, raw_csrf) = seed_portal_session(&store, member);
        let app = router_with_state(test_state(store));

        let session = portal_request(
            app.clone(),
            axum::http::Method::GET,
            "/admin-ui/auth/session",
            &raw_session,
            &raw_csrf,
            None,
            "",
        )
        .await;
        assert_eq!(session.status(), StatusCode::OK);
        let value = response_json(session).await;
        assert_eq!(value["authenticated"], true);
        assert_eq!(value["member"]["status"], "pending");

        let denied = portal_request(
            app,
            axum::http::Method::GET,
            "/admin-ui/admin/members",
            &raw_session,
            &raw_csrf,
            Some(&raw_csrf),
            "",
        )
        .await;
        assert_eq!(denied.status(), StatusCode::FORBIDDEN);
        assert_eq!(
            response_json(denied).await["error"]["code"],
            "pending_portal_member"
        );
    }

    #[tokio::test]
    async fn portal_admin_requires_session_bound_csrf_and_can_manage_members() {
        let store = default_store();
        let admin = active_portal_member(true);
        let (raw_session, raw_csrf) = seed_portal_session(&store, admin);
        let pending = PortalAccessStore::upsert_oidc_member(
            &store,
            "tenant-test",
            "pending-object",
            Some("pending@relayna.test"),
            Some("Pending Owner"),
            Utc::now(),
        )
        .await
        .expect("pending member");
        let app = router_with_state(test_state(store));

        let bad_csrf = portal_request(
            app.clone(),
            axum::http::Method::PATCH,
            &format!("/admin-ui/admin/members/{}", pending.id),
            &raw_session,
            &raw_csrf,
            Some("wrong-csrf"),
            r#"{"status":"active"}"#,
        )
        .await;
        assert_eq!(bad_csrf.status(), StatusCode::FORBIDDEN);
        assert_eq!(
            response_json(bad_csrf).await["error"]["code"],
            "invalid_csrf_token"
        );

        let approved = portal_request(
            app,
            axum::http::Method::PATCH,
            &format!("/admin-ui/admin/members/{}", pending.id),
            &raw_session,
            &raw_csrf,
            Some(&raw_csrf),
            r#"{"status":"active"}"#,
        )
        .await;
        assert_eq!(approved.status(), StatusCode::OK);
        assert_eq!(response_json(approved).await["status"], "active");
    }

    #[tokio::test]
    async fn owner_api_enforces_exact_service_membership_server_side() {
        let store = default_store();
        let owner = active_portal_member(false);
        let owner_id = owner.id;
        let (raw_session, raw_csrf) = seed_portal_session(&store, owner);
        let mut ocr = openapi_test_service("http://ocr.internal".to_owned());
        ocr.name = "ocr".to_owned();
        let mut orders = openapi_test_service("http://orders.internal".to_owned());
        orders.name = "orders".to_owned();
        orders.route_pattern = "/services/orders/*".to_owned();
        store
            .services
            .lock()
            .expect("lock poisoned")
            .extend([ocr, orders]);
        store
            .service_memberships
            .lock()
            .expect("lock poisoned")
            .push(ServiceMembership {
                member_id: owner_id,
                service_name: "orders".to_owned(),
                role: ServiceMemberRole::Owner,
                created_at: Utc::now(),
                updated_at: Utc::now(),
            });
        let app = router_with_state(test_state(store));

        let allowed = portal_request(
            app.clone(),
            axum::http::Method::GET,
            "/owner/v1/services/orders/dashboard?service=ocr",
            &raw_session,
            &raw_csrf,
            None,
            "",
        )
        .await;
        assert_eq!(allowed.status(), StatusCode::OK);
        assert_eq!(response_json(allowed).await["service_name"], "orders");

        let denied = portal_request(
            app,
            axum::http::Method::GET,
            "/owner/v1/services/ocr/dashboard",
            &raw_session,
            &raw_csrf,
            None,
            "",
        )
        .await;
        assert_eq!(denied.status(), StatusCode::FORBIDDEN);
        assert_eq!(
            response_json(denied).await["error"]["code"],
            "insufficient_portal_access"
        );
    }

    #[tokio::test]
    async fn owner_service_list_propagates_service_store_failures() {
        let store = default_store();
        let owner = active_portal_member(false);
        let owner_id = owner.id;
        let (raw_session, raw_csrf) = seed_portal_session(&store, owner);
        store
            .service_memberships
            .lock()
            .expect("lock poisoned")
            .push(ServiceMembership {
                member_id: owner_id,
                service_name: "store-error".to_owned(),
                role: ServiceMemberRole::Viewer,
                created_at: Utc::now(),
                updated_at: Utc::now(),
            });
        let response = portal_request(
            router_with_state(test_state(store)),
            axum::http::Method::GET,
            "/owner/v1/services",
            &raw_session,
            &raw_csrf,
            None,
            "",
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        assert_eq!(
            response_json(response).await["error"]["code"],
            "store_unavailable"
        );
    }

    #[tokio::test]
    async fn portal_management_and_owner_monitoring_workflows_cover_all_public_routes() {
        let store = default_store();
        let admin = active_portal_member(true);
        let admin_id = admin.id;
        let (raw_session, raw_csrf) = seed_portal_session(&store, admin);
        let owner = active_portal_member(false);
        let owner_id = owner.id;
        store
            .portal_members
            .lock()
            .expect("lock poisoned")
            .push(owner);
        let mut service = openapi_test_service("http://orders.internal".to_owned());
        service.name = "orders".to_owned();
        service.route_pattern = "/services/orders/*".to_owned();
        store.services.lock().expect("lock poisoned").push(service);
        store.events.lock().expect("lock poisoned").extend([
            UsageEvent {
                request_id: "req-orders-ok".to_owned(),
                key_id: Uuid::new_v4(),
                project_id: Some(Uuid::new_v4()),
                route: Route::ServiceWildcard,
                model: None,
                provider: gateway_core::Provider::InternalService,
                status: UsageStatus::Success,
                status_code: 200,
                latency_ms: 12,
                input_tokens: None,
                output_tokens: None,
                total_tokens: None,
                estimated_cost_usd: Some(0.01),
                cost_source: Some("fixed".to_owned()),
                cost_mode: Some(ServiceCostMode::Fixed),
                pricing_rule_name: None,
                service_name: Some("orders".to_owned()),
                http_method: Some("GET".to_owned()),
                endpoint_path: Some("/orders/42".to_owned()),
                endpoint_template: Some("/orders/{id}".to_owned()),
                task_id: None,
                run_id: None,
                trace_id: None,
                fallback_count: 0,
                created_at: Utc::now(),
            },
            UsageEvent {
                request_id: "req-orders-failed".to_owned(),
                key_id: Uuid::new_v4(),
                project_id: None,
                route: Route::ServiceWildcard,
                model: None,
                provider: gateway_core::Provider::InternalService,
                status: UsageStatus::Failure,
                status_code: 502,
                latency_ms: 20,
                input_tokens: None,
                output_tokens: None,
                total_tokens: None,
                estimated_cost_usd: None,
                cost_source: None,
                cost_mode: None,
                pricing_rule_name: None,
                service_name: Some("orders".to_owned()),
                http_method: Some("POST".to_owned()),
                endpoint_path: Some("/orders".to_owned()),
                endpoint_template: Some("/orders".to_owned()),
                task_id: None,
                run_id: None,
                trace_id: None,
                fallback_count: 0,
                created_at: Utc::now(),
            },
        ]);
        let app = router_with_state(test_state(store.clone()));

        let auth_config = request(app.clone(), "/admin-ui/auth/config").await;
        assert_eq!(auth_config.status(), StatusCode::OK);
        assert_eq!(response_json(auth_config).await["enabled"], false);
        let anonymous_session = request(app.clone(), "/admin-ui/auth/session").await;
        assert_eq!(anonymous_session.status(), StatusCode::OK);
        assert_eq!(
            response_json(anonymous_session).await["authenticated"],
            false
        );
        let login = request(app.clone(), "/admin-ui/auth/login").await;
        assert_eq!(login.status(), StatusCode::BAD_GATEWAY);

        let membership_uri = format!("/admin-ui/admin/members/{owner_id}/services/orders");
        let assigned = portal_request(
            app.clone(),
            axum::http::Method::PUT,
            &membership_uri,
            &raw_session,
            &raw_csrf,
            Some(&raw_csrf),
            r#"{"role":"owner"}"#,
        )
        .await;
        assert_eq!(assigned.status(), StatusCode::OK);
        assert_eq!(response_json(assigned).await["role"], "owner");

        let members = portal_request(
            app.clone(),
            axum::http::Method::GET,
            "/admin-ui/admin/members",
            &raw_session,
            &raw_csrf,
            Some(&raw_csrf),
            "",
        )
        .await;
        assert_eq!(members.status(), StatusCode::OK);
        assert_eq!(response_json(members).await.as_array().unwrap().len(), 2);

        let created = portal_request(
            app.clone(),
            axum::http::Method::POST,
            "/admin-ui/admin/managed-identities",
            &raw_session,
            &raw_csrf,
            Some(&raw_csrf),
            r#"{"tenant_id":"tenant-test","client_id":"orders-client","object_id":"orders-object","display_name":"Orders monitor","service_name":"orders","required_role":"gateway.monitor.read","enabled":true}"#,
        )
        .await;
        assert_eq!(created.status(), StatusCode::CREATED);
        let identity_id = response_json(created).await["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let identities = portal_request(
            app.clone(),
            axum::http::Method::GET,
            "/admin-ui/admin/managed-identities",
            &raw_session,
            &raw_csrf,
            Some(&raw_csrf),
            "",
        )
        .await;
        assert_eq!(response_json(identities).await.as_array().unwrap().len(), 1);
        let patched = portal_request(
            app.clone(),
            axum::http::Method::PATCH,
            &format!("/admin-ui/admin/managed-identities/{identity_id}"),
            &raw_session,
            &raw_csrf,
            Some(&raw_csrf),
            r#"{"display_name":"Orders read-only","enabled":false}"#,
        )
        .await;
        assert_eq!(patched.status(), StatusCode::OK);
        assert_eq!(response_json(patched).await["enabled"], false);

        let (owner_session, owner_csrf) = {
            let owner = store
                .portal_members
                .lock()
                .expect("lock poisoned")
                .iter()
                .find(|member| member.id == owner_id)
                .cloned()
                .unwrap();
            seed_portal_session(&store, owner)
        };
        for route in [
            "/owner/v1/services",
            "/owner/v1/services/orders",
            "/owner/v1/services/orders/dashboard",
            "/owner/v1/services/orders/events",
            "/owner/v1/services/orders/errors",
            "/owner/v1/services/orders/logs",
            "/owner/v1/services/orders/endpoints",
            "/owner/v1/services/orders/export.json",
            "/owner/v1/services/orders/export.csv",
        ] {
            let response = portal_request(
                app.clone(),
                axum::http::Method::GET,
                route,
                &owner_session,
                &owner_csrf,
                None,
                "",
            )
            .await;
            assert_eq!(response.status(), StatusCode::OK, "route {route}");
        }

        let deleted_identity = portal_request(
            app.clone(),
            axum::http::Method::DELETE,
            &format!("/admin-ui/admin/managed-identities/{identity_id}"),
            &raw_session,
            &raw_csrf,
            Some(&raw_csrf),
            "",
        )
        .await;
        assert_eq!(deleted_identity.status(), StatusCode::NO_CONTENT);
        let deleted_membership = portal_request(
            app.clone(),
            axum::http::Method::DELETE,
            &membership_uri,
            &raw_session,
            &raw_csrf,
            Some(&raw_csrf),
            "",
        )
        .await;
        assert_eq!(deleted_membership.status(), StatusCode::NO_CONTENT);

        let logout = portal_request(
            app,
            axum::http::Method::POST,
            "/admin-ui/auth/logout",
            &raw_session,
            &raw_csrf,
            Some(&raw_csrf),
            "",
        )
        .await;
        assert_eq!(logout.status(), StatusCode::OK);
        assert!(response_json(logout).await["logout_url"].is_null());
        assert!(store
            .audit_events
            .lock()
            .expect("lock poisoned")
            .iter()
            .any(|event| event.actor_member_id == Some(admin_id)));
    }

    #[tokio::test]
    async fn portal_security_edges_reject_invalid_state_sessions_and_revoked_access() {
        let store = default_store();
        let admin = active_portal_member(true);
        let (raw_session, raw_csrf) = seed_portal_session(&store, admin);
        let mut state = test_state(store.clone());
        state.portal_oidc = Some(Arc::new(
            PortalOidcRuntime::new(crate::portal::PortalOidcConfig {
                tenant_id: "tenant-test".into(),
                client_id: "browser-client".into(),
                client_secret: "fixture-value".into(),
                issuer: "http://127.0.0.1:9".into(),
                discovery_url: "http://127.0.0.1:9/.well-known/openid-configuration".into(),
                redirect_uri: "http://127.0.0.1:18381/admin-ui/auth/callback".into(),
                post_logout_redirect_uri: "http://127.0.0.1:18381/admin-ui".into(),
                session_ttl_seconds: 3600,
                login_ttl_seconds: 300,
                cookie_secure: true,
            })
            .expect("portal runtime"),
        ));
        let app = router_with_state(state);

        let provider_error =
            request(app.clone(), "/admin-ui/auth/callback?error=access_denied").await;
        assert_eq!(provider_error.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            response_json(provider_error).await["error"]["code"],
            "invalid_oidc_transaction"
        );
        let missing_code = request(app.clone(), "/admin-ui/auth/callback?state=missing").await;
        assert_eq!(missing_code.status(), StatusCode::UNAUTHORIZED);
        let unknown_state = request(
            app.clone(),
            "/admin-ui/auth/callback?code=unused&state=unknown",
        )
        .await;
        assert_eq!(unknown_state.status(), StatusCode::UNAUTHORIZED);

        let login = request(
            app.clone(),
            "/admin-ui/auth/login?return_to=https%3A%2F%2Fevil.example",
        )
        .await;
        assert_eq!(login.status(), StatusCode::BAD_GATEWAY);
        assert!(store
            .oidc_transactions
            .lock()
            .expect("lock poisoned")
            .is_empty());

        let no_csrf_cookie = app
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .uri("/admin-ui/auth/session")
                    .header(
                        header::COOKIE,
                        format!("{PORTAL_SESSION_COOKIE}={raw_session}"),
                    )
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(no_csrf_cookie.status(), StatusCode::UNAUTHORIZED);
        let wrong_csrf_cookie = portal_request(
            app.clone(),
            axum::http::Method::GET,
            "/admin-ui/auth/session",
            &raw_session,
            "wrong-cookie",
            None,
            "",
        )
        .await;
        assert_eq!(wrong_csrf_cookie.status(), StatusCode::UNAUTHORIZED);

        let unknown_id = Uuid::new_v4();
        let missing_member = portal_request(
            app.clone(),
            axum::http::Method::PATCH,
            &format!("/admin-ui/admin/members/{unknown_id}"),
            &raw_session,
            &raw_csrf,
            Some(&raw_csrf),
            r#"{"status":"active"}"#,
        )
        .await;
        assert_eq!(missing_member.status(), StatusCode::NOT_FOUND);
        let missing_identity = portal_request(
            app.clone(),
            axum::http::Method::PATCH,
            &format!("/admin-ui/admin/managed-identities/{unknown_id}"),
            &raw_session,
            &raw_csrf,
            Some(&raw_csrf),
            r#"{"enabled":false}"#,
        )
        .await;
        assert_eq!(missing_identity.status(), StatusCode::NOT_FOUND);
        let missing_identity_delete = portal_request(
            app.clone(),
            axum::http::Method::DELETE,
            &format!("/admin-ui/admin/managed-identities/{unknown_id}"),
            &raw_session,
            &raw_csrf,
            Some(&raw_csrf),
            "",
        )
        .await;
        assert_eq!(missing_identity_delete.status(), StatusCode::NOT_FOUND);
        let missing_membership = portal_request(
            app.clone(),
            axum::http::Method::DELETE,
            &format!("/admin-ui/admin/members/{unknown_id}/services/missing"),
            &raw_session,
            &raw_csrf,
            Some(&raw_csrf),
            "",
        )
        .await;
        assert_eq!(missing_membership.status(), StatusCode::NOT_FOUND);

        let mut blocked = active_portal_member(false);
        blocked.status = MemberStatus::Blocked;
        let (blocked_session, blocked_csrf) = seed_portal_session(&store, blocked);
        let blocked_response = portal_request(
            app.clone(),
            axum::http::Method::GET,
            "/owner/v1/services",
            &blocked_session,
            &blocked_csrf,
            None,
            "",
        )
        .await;
        assert_eq!(blocked_response.status(), StatusCode::FORBIDDEN);
        assert_eq!(
            response_json(blocked_response).await["error"]["code"],
            "blocked_portal_member"
        );

        let anonymous_owner = request(app, "/owner/v1/services/orders").await;
        assert_eq!(anonymous_owner.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            response_json(anonymous_owner).await["error"]["code"],
            "missing_entra_authorization"
        );
    }

    #[tokio::test]
    async fn development_oidc_drives_browser_and_workload_authentication_end_to_end() {
        let (_oidc, issuer) = start_development_oidc().await;
        let store = default_store();
        let mut service = openapi_test_service("http://orders.internal".to_owned());
        service.name = "orders".to_owned();
        service.route_pattern = "/services/orders/*".to_owned();
        store.services.lock().expect("lock poisoned").push(service);
        store
            .managed_identities
            .lock()
            .expect("lock poisoned")
            .push(gateway_core::ManagedIdentityBinding {
                id: Uuid::new_v4(),
                tenant_id: "00000000-0000-0000-0000-000000000001".into(),
                client_id: "00000000-0000-0000-0000-000000000101".into(),
                object_id: Some("00000000-0000-0000-0000-000000000102".into()),
                display_name: "Development workload".into(),
                service_name: "orders".into(),
                required_role: gateway_core::OWNER_WORKLOAD_ROLE.into(),
                enabled: true,
                created_at: Utc::now(),
                updated_at: Utc::now(),
            });
        let portal_config = crate::portal::PortalOidcConfig {
            tenant_id: "00000000-0000-0000-0000-000000000001".into(),
            client_id: "relayna-gateway-local".into(),
            client_secret: "relayna-development-browser-secret".into(),
            issuer: issuer.clone(),
            discovery_url: format!("{issuer}/.well-known/openid-configuration"),
            redirect_uri: "http://127.0.0.1:18381/admin-ui/auth/callback".into(),
            post_logout_redirect_uri: "http://127.0.0.1:18381/admin-ui".into(),
            session_ttl_seconds: 3600,
            login_ttl_seconds: 300,
            cookie_secure: false,
        };
        let mut state = test_state(store.clone());
        state.portal_oidc = Some(Arc::new(
            PortalOidcRuntime::new(portal_config).expect("portal OIDC runtime"),
        ));
        state.owner_entra_verifier = Some(Arc::new(
            EntraJwtVerifier::new(EntraAuthConfig {
                tenant_id: "00000000-0000-0000-0000-000000000001".into(),
                audience: "api://relayna-gateway-owner".into(),
                issuer: issuer.clone(),
                oidc_discovery_url: format!("{issuer}/.well-known/openid-configuration"),
                required_scope: None,
                required_role: Some(gateway_core::OWNER_WORKLOAD_ROLE.into()),
                allowed_groups: Vec::new(),
                accepted_algorithms: vec!["RS256".into()],
                relayna_key_header: gateway_core::ENTRA_DEFAULT_RELAYNA_KEY_HEADER.into(),
                jwks_cache_ttl_seconds: 300,
                clock_skew_seconds: 60,
            })
            .expect("owner Entra verifier"),
        ));
        let app = router_with_state(state);

        let login = request(
            app.clone(),
            "/admin-ui/auth/login?return_to=%2Fadmin-ui%2F%23%2Fmy-services",
        )
        .await;
        assert_eq!(login.status(), StatusCode::TEMPORARY_REDIRECT);
        let login_cookie = login
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .map(|value| value.to_str().unwrap())
            .find(|value| value.starts_with(&format!("{PORTAL_LOGIN_COOKIE}=")))
            .expect("browser-bound login cookie")
            .to_owned();
        assert!(login_cookie.contains("HttpOnly"));
        assert!(login_cookie.contains("SameSite=Lax"));
        assert!(login_cookie.contains("Path=/admin-ui/auth"));
        let login_cookie_pair = login_cookie.split(';').next().unwrap().to_owned();
        let mut authorize = url::Url::parse(
            login
                .headers()
                .get(header::LOCATION)
                .unwrap()
                .to_str()
                .unwrap(),
        )
        .unwrap();
        authorize
            .query_pairs_mut()
            .append_pair("mock_user", "service_owner");
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();
        let authorization = client.get(authorize).send().await.unwrap();
        assert_eq!(authorization.status(), reqwest::StatusCode::FOUND);
        let callback = url::Url::parse(
            authorization
                .headers()
                .get(reqwest::header::LOCATION)
                .unwrap()
                .to_str()
                .unwrap(),
        )
        .unwrap();
        let callback_route = format!(
            "{}?{}",
            callback.path(),
            callback.query().expect("callback query")
        );
        let unbound_callback = request(app.clone(), &callback_route).await;
        assert_eq!(unbound_callback.status(), StatusCode::UNAUTHORIZED);
        let callback_response = app
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .uri(&callback_route)
                    .header(header::COOKIE, &login_cookie_pair)
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(callback_response.status(), StatusCode::SEE_OTHER);
        let cookie_header = callback_response
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .map(|value| value.to_str().unwrap().split(';').next().unwrap())
            .filter(|value| !value.ends_with('='))
            .collect::<Vec<_>>()
            .join("; ");
        let session = app
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .uri("/admin-ui/auth/session")
                    .header(header::COOKIE, &cookie_header)
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(session.status(), StatusCode::OK);
        let session = response_json(session).await;
        assert_eq!(session["authenticated"], true);
        assert_eq!(session["member"]["status"], "pending");
        assert_eq!(session["member"]["email"], "orders.owner@relayna.dev");
        let csrf_token = session["csrf_token"].as_str().unwrap();
        let logout = app
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .method(axum::http::Method::POST)
                    .uri("/admin-ui/auth/logout")
                    .header(header::COOKIE, &cookie_header)
                    .header("x-csrf-token", csrf_token)
                    .body(axum::body::Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(logout.status(), StatusCode::OK);
        let logout_url = response_json(logout).await["logout_url"]
            .as_str()
            .unwrap()
            .to_owned();
        let provider_logout = client.get(logout_url).send().await.unwrap();
        assert_eq!(provider_logout.status(), reqwest::StatusCode::FOUND);
        assert_eq!(
            provider_logout
                .headers()
                .get(reqwest::header::LOCATION)
                .unwrap(),
            "http://127.0.0.1:18381/admin-ui"
        );

        let workload_token = client
            .post(format!("{issuer}/token"))
            .form(&[
                ("grant_type", "client_credentials"),
                ("client_id", "00000000-0000-0000-0000-000000000101"),
                ("client_secret", "relayna-development-workload-secret"),
                ("scope", "api://relayna-gateway-owner/.default"),
            ])
            .send()
            .await
            .unwrap()
            .json::<serde_json::Value>()
            .await
            .unwrap()["access_token"]
            .as_str()
            .unwrap()
            .to_owned();
        let workload_response = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/owner/v1/services/orders")
                    .header(header::AUTHORIZATION, format!("Bearer {workload_token}"))
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(workload_response.status(), StatusCode::OK);
        assert_eq!(response_json(workload_response).await["role"], "viewer");
    }

    #[tokio::test]
    async fn litellm_ui_proxy_rejects_missing_operator_token() {
        let app = router_with_state(test_state(default_store()));

        let response = request(app, "/admin-ui/litellm-ui/").await;

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let value = response_json(response).await;
        assert_eq!(value["error"]["code"], "missing_authorization");
    }

    #[tokio::test]
    async fn litellm_ui_proxy_forwards_with_gateway_litellm_credential_only() {
        let (base_url, captured) = spawn_litellm_server(
            "200 OK",
            vec![("content-type", "application/javascript; charset=utf-8")],
            r#"self.__next_f.push(["/ui"]);fetch(`/v2/login`);fetch("/public/model_hub");fetch('/get_image');import('/litellm-asset-prefix/_next/app.js');"#,
        );
        let app = router_with_state(test_state_with_litellm(
            default_store(),
            base_url,
            "server-litellm-key",
        ));
        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .method(axum::http::Method::GET)
                    .uri("/admin-ui/litellm-ui/assets/app.js?bar=baz")
                    .header("x-request-id", "req_test")
                    .header("authorization", format!("Bearer {TEST_OPERATOR_TOKEN}"))
                    .header("x-litellm-api-key", "client-litellm-key")
                    .header("x-aih-api-key", "client-aih-key")
                    .header("x-api-key", "client-api-key")
                    .header("x-relayna-worker-token", "client-worker-token")
                    .header("x-apigee-entra-identity", "client-identity")
                    .header("x-apigee-entra-signature", "client-signature")
                    .header(
                        "cookie",
                        format!("{LITELLM_UI_OPERATOR_COOKIE}=client-operator-token; lite=client"),
                    )
                    .body(axum::body::Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let set_cookie = response
            .headers()
            .get(header::SET_COOKIE)
            .and_then(|value| value.to_str().ok())
            .expect("operator cookie");
        assert!(set_cookie.starts_with(&format!(
            "{LITELLM_UI_OPERATOR_COOKIE}={TEST_OPERATOR_TOKEN};"
        )));
        assert!(set_cookie.contains("HttpOnly"));
        assert!(set_cookie.contains("SameSite=Lax"));
        assert!(set_cookie.contains("Path=/"));
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let body = String::from_utf8(body.to_vec()).expect("utf8 body");
        assert!(body.contains(r#"self.__next_f.push(["/admin-ui/litellm-ui"])"#));
        assert!(body.contains("fetch(`/admin-ui/litellm-ui/v2/login`)"));
        assert!(body.contains(r#"fetch("/admin-ui/litellm-ui/public/model_hub")"#));
        assert!(body.contains("fetch('/admin-ui/litellm-ui/get_image')"));
        assert!(body.contains("import('/admin-ui/litellm-ui/litellm-asset-prefix/_next/app.js')"));

        let captured = captured
            .recv_timeout(std::time::Duration::from_secs(2))
            .expect("captured upstream request");
        assert_eq!(
            captured.request_line,
            "GET /ui/assets/app.js?bar=baz HTTP/1.1"
        );
        assert_eq!(
            captured_header(&captured, "authorization"),
            Some("Bearer server-litellm-key")
        );
        assert!(captured_header(&captured, "x-litellm-api-key").is_none());
        assert!(captured_header(&captured, "x-aih-api-key").is_none());
        assert!(captured_header(&captured, "x-api-key").is_none());
        assert!(captured_header(&captured, "x-relayna-worker-token").is_none());
        assert!(captured_header(&captured, "x-apigee-entra-identity").is_none());
        assert!(captured_header(&captured, "x-apigee-entra-signature").is_none());
        assert!(captured_header(&captured, "cookie").is_none());
        assert!(captured.body.is_empty());
    }

    #[tokio::test]
    async fn litellm_ui_proxy_maps_root_api_paths_to_litellm_root() {
        let (base_url, captured) = spawn_litellm_server(
            "200 OK",
            vec![("content-type", "application/json")],
            r#"{"ok":true,"redirect_url":"http://litellm:4000/ui/?login=success"}"#,
        );
        let app = router_with_state(test_state_with_litellm(
            default_store(),
            base_url,
            "server-litellm-key",
        ));
        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .method(axum::http::Method::POST)
                    .uri("/admin-ui/litellm-ui/v2/login?next=%2Fui%2Fmodels")
                    .header("authorization", format!("Bearer {TEST_OPERATOR_TOKEN}"))
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(r#"{"username":"admin"}"#))
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let value: serde_json::Value = serde_json::from_slice(&body).expect("json body");
        assert_eq!(value["redirect_url"], "/admin-ui/litellm-ui/?login=success");
        let captured = captured
            .recv_timeout(std::time::Duration::from_secs(2))
            .expect("captured upstream request");
        assert_eq!(
            captured.request_line,
            "POST /v2/login?next=%2Fui%2Fmodels HTTP/1.1"
        );
        assert_eq!(
            captured_header(&captured, "authorization"),
            Some("Bearer server-litellm-key")
        );
        assert_eq!(captured.body, br#"{"username":"admin"}"#);
    }

    #[tokio::test]
    async fn litellm_ui_proxy_root_emitted_paths_still_require_operator_token() {
        let app = router_with_state(test_state(default_store()));

        let response = request(app, "/litellm-asset-prefix/_next/static/app.js").await;

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let value = response_json(response).await;
        assert_eq!(value["error"]["code"], "missing_authorization");
    }

    #[tokio::test]
    async fn litellm_ui_proxy_forwards_root_emitted_asset_paths() {
        let (base_url, captured) = spawn_litellm_server(
            "200 OK",
            vec![("content-type", "application/javascript")],
            r#"console.log("ok")"#,
        );
        let app = router_with_state(test_state_with_litellm(
            default_store(),
            base_url,
            "server-litellm-key",
        ));
        let response = admin_get(
            app,
            "/litellm-asset-prefix/_next/static/app.js?v=1",
            Some(TEST_OPERATOR_TOKEN),
        )
        .await;

        assert_eq!(response.status(), StatusCode::OK);
        let captured = captured
            .recv_timeout(std::time::Duration::from_secs(2))
            .expect("captured upstream request");
        assert_eq!(
            captured.request_line,
            "GET /litellm-asset-prefix/_next/static/app.js?v=1 HTTP/1.1"
        );
        assert_eq!(
            captured_header(&captured, "authorization"),
            Some("Bearer server-litellm-key")
        );
    }

    #[tokio::test]
    async fn litellm_ui_proxy_accepts_operator_cookie_for_browser_subrequests() {
        let (base_url, captured) = spawn_litellm_server(
            "200 OK",
            vec![("content-type", "application/javascript")],
            r#"console.log("ok")"#,
        );
        let app = router_with_state(test_state_with_litellm(
            default_store(),
            base_url,
            "server-litellm-key",
        ));
        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .method(axum::http::Method::GET)
                    .uri("/litellm-asset-prefix/_next/static/app.js")
                    .header(
                        header::COOKIE,
                        format!("{LITELLM_UI_OPERATOR_COOKIE}={TEST_OPERATOR_TOKEN}"),
                    )
                    .body(axum::body::Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        assert!(response.headers().get(header::SET_COOKIE).is_none());
        let captured = captured
            .recv_timeout(std::time::Duration::from_secs(2))
            .expect("captured upstream request");
        assert_eq!(
            captured.request_line,
            "GET /litellm-asset-prefix/_next/static/app.js HTTP/1.1"
        );
        assert_eq!(
            captured_header(&captured, "authorization"),
            Some("Bearer server-litellm-key")
        );
        assert!(captured_header(&captured, "cookie").is_none());
    }

    #[tokio::test]
    async fn litellm_ui_proxy_rewrites_ui_redirect_locations() {
        let (base_url, captured) =
            spawn_litellm_server("302 Found", vec![("location", "/ui/login")], "");
        let app = router_with_state(test_state_with_litellm(
            default_store(),
            base_url,
            "server-litellm-key",
        ));
        let response = admin_get(app, "/admin-ui/litellm-ui", Some(TEST_OPERATOR_TOKEN)).await;

        assert_eq!(response.status(), StatusCode::FOUND);
        assert_eq!(
            response
                .headers()
                .get(header::LOCATION)
                .and_then(|value| value.to_str().ok()),
            Some("/admin-ui/litellm-ui/login")
        );
        let captured = captured
            .recv_timeout(std::time::Duration::from_secs(2))
            .expect("captured upstream request");
        assert_eq!(captured.request_line, "GET /ui HTTP/1.1");
    }

    #[test]
    fn litellm_ui_upstream_url_preserves_litellm_asset_prefix() {
        let url = litellm_ui_upstream_url(
            "http://litellm.internal:4000",
            "litellm-asset-prefix/_next/static/app.js",
            Some("v=1"),
        )
        .expect("url");

        assert_eq!(
            url.as_str(),
            "http://litellm.internal:4000/litellm-asset-prefix/_next/static/app.js?v=1"
        );
    }

    #[test]
    fn litellm_ui_upstream_url_maps_ui_pages_and_root_api_paths() {
        let root_api = litellm_ui_upstream_url(
            "http://litellm.internal:4000",
            "v2/login",
            Some("redirect_to=%2Fui%2Fmodels"),
        )
        .expect("root api url");
        assert_eq!(
            root_api.as_str(),
            "http://litellm.internal:4000/v2/login?redirect_to=%2Fui%2Fmodels"
        );

        let page = litellm_ui_upstream_url("http://litellm.internal:4000", "model_hub/", None)
            .expect("page url");
        assert_eq!(page.as_str(), "http://litellm.internal:4000/ui/model_hub/");
    }

    #[tokio::test]
    async fn generation_routes_are_not_served_by_axum_control_api() {
        let raw = "rk_live_1234567890abcdef";
        let store = MemoryStore {
            key: Arc::new(Mutex::new(Some(stored_key(raw)))),
            admin_key: Arc::new(Mutex::new(None)),
            services: Arc::new(Mutex::new(Vec::new())),
            openai_routes: Arc::new(Mutex::new(default_openai_routes())),
            anthropic_routes: Arc::new(Mutex::new(default_anthropic_routes())),
            operator_tokens: Arc::new(Mutex::new(vec![TEST_OPERATOR_TOKEN.to_owned()])),
            events: Arc::new(Mutex::new(Vec::new())),
            audit_events: Arc::new(Mutex::new(Vec::new())),
            portal_members: Arc::new(Mutex::new(Vec::new())),
            service_memberships: Arc::new(Mutex::new(Vec::new())),
            managed_identities: Arc::new(Mutex::new(Vec::new())),
            oidc_transactions: Arc::new(Mutex::new(Vec::new())),
            portal_sessions: Arc::new(Mutex::new(Vec::new())),
            postgres_ready: true,
            studio_connection: Arc::new(Mutex::new(None)),
            gateway_auth_settings: Arc::new(Mutex::new(None)),
            litellm_passthrough_settings: Arc::new(Mutex::new(
                LiteLlmPassthroughSettings::default_with_updated_at(Utc::now()),
            )),
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
            anthropic_routes: Arc::new(Mutex::new(default_anthropic_routes())),
            operator_tokens: Arc::new(Mutex::new(vec![TEST_OPERATOR_TOKEN.to_owned()])),
            events: Arc::new(Mutex::new(Vec::new())),
            audit_events: Arc::new(Mutex::new(Vec::new())),
            portal_members: Arc::new(Mutex::new(Vec::new())),
            service_memberships: Arc::new(Mutex::new(Vec::new())),
            managed_identities: Arc::new(Mutex::new(Vec::new())),
            oidc_transactions: Arc::new(Mutex::new(Vec::new())),
            portal_sessions: Arc::new(Mutex::new(Vec::new())),
            postgres_ready: false,
            studio_connection: Arc::new(Mutex::new(None)),
            gateway_auth_settings: Arc::new(Mutex::new(None)),
            litellm_passthrough_settings: Arc::new(Mutex::new(
                LiteLlmPassthroughSettings::default_with_updated_at(Utc::now()),
            )),
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
            anthropic_routes: Arc::new(Mutex::new(default_anthropic_routes())),
            operator_tokens: Arc::new(Mutex::new(vec![TEST_OPERATOR_TOKEN.to_owned()])),
            events: Arc::new(Mutex::new(Vec::new())),
            audit_events: Arc::new(Mutex::new(Vec::new())),
            portal_members: Arc::new(Mutex::new(Vec::new())),
            service_memberships: Arc::new(Mutex::new(Vec::new())),
            managed_identities: Arc::new(Mutex::new(Vec::new())),
            oidc_transactions: Arc::new(Mutex::new(Vec::new())),
            portal_sessions: Arc::new(Mutex::new(Vec::new())),
            postgres_ready: true,
            studio_connection: Arc::new(Mutex::new(None)),
            gateway_auth_settings: Arc::new(Mutex::new(None)),
            litellm_passthrough_settings: Arc::new(Mutex::new(
                LiteLlmPassthroughSettings::default_with_updated_at(Utc::now()),
            )),
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
            anthropic_routes: Arc::new(Mutex::new(default_anthropic_routes())),
            operator_tokens: Arc::new(Mutex::new(vec![TEST_OPERATOR_TOKEN.to_owned()])),
            events: Arc::new(Mutex::new(Vec::new())),
            audit_events: Arc::new(Mutex::new(Vec::new())),
            portal_members: Arc::new(Mutex::new(Vec::new())),
            service_memberships: Arc::new(Mutex::new(Vec::new())),
            managed_identities: Arc::new(Mutex::new(Vec::new())),
            oidc_transactions: Arc::new(Mutex::new(Vec::new())),
            portal_sessions: Arc::new(Mutex::new(Vec::new())),
            postgres_ready: true,
            studio_connection: Arc::new(Mutex::new(None)),
            gateway_auth_settings: Arc::new(Mutex::new(None)),
            litellm_passthrough_settings: Arc::new(Mutex::new(
                LiteLlmPassthroughSettings::default_with_updated_at(Utc::now()),
            )),
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
    async fn admin_policy_simulator_accepts_explicit_service_name() {
        let app = router_with_state(test_state(default_store()));
        let response = admin_post(
            app,
            "/admin-ui/admin/policy/simulate",
            Some(TEST_OPERATOR_TOKEN),
            r#"{"path":"/services/ocr-service-api/test","provider":"internal-service","service_name":"ocr-service"}"#,
        )
        .await;

        assert_eq!(response.status(), StatusCode::OK);
        let value = response_json(response).await;
        assert_eq!(value["route_match"]["route"], "/services/*");
        assert_eq!(value["route_match"]["provider"], "internal-service");
        assert_eq!(value["route_match"]["service_name"], "ocr-service");
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
    async fn admin_policy_simulator_accepts_anthropic_policy_routes() {
        let app = router_with_state(test_state(default_store()));
        let response = admin_post(
            app,
            "/admin-ui/admin/policy/simulate",
            Some(TEST_OPERATOR_TOKEN),
            r#"{"path":"/v1/messages","body":{"model":"claude-review"},"policy":{"allowed_routes":["/v1/messages","/v1/messages/batches","/v1/messages/batches/*/results"]}}"#,
        )
        .await;

        assert_eq!(response.status(), StatusCode::OK);
        let value = response_json(response).await;
        assert_eq!(value["route_match"]["route"], "/v1/messages");
        assert_eq!(value["final_decision"]["allowed"], true);
        assert_eq!(value["policy_merge"]["allowed_routes"][0], "/v1/messages");
        assert_eq!(
            value["policy_merge"]["allowed_routes"][2],
            "/v1/messages/batches/*/results"
        );
    }

    #[tokio::test]
    async fn admin_policy_simulator_warns_when_effective_allowlists_exclude_request() {
        let app = router_with_state(test_state(default_store()));
        let response = admin_post(
            app,
            "/admin-ui/admin/policy/simulate",
            Some(TEST_OPERATOR_TOKEN),
            r#"{"path":"/v1/chat/completions","body":{"model":"gpt-4.1-mini"},"policy":{"allowed_providers":["internal-service"]}}"#,
        )
        .await;

        assert_eq!(response.status(), StatusCode::OK);
        let value = response_json(response).await;
        assert_eq!(value["final_decision"]["allowed"], false);
        assert_eq!(value["final_decision"]["error_code"], "policy_denied");
        assert_eq!(
            value["policy_merge"]["allowed_providers"][0],
            "internal-service"
        );
        assert_eq!(
            value["policy_merge"]["applied_layers"],
            serde_json::json!([])
        );
        assert!(value["warnings"][0]
            .as_str()
            .expect("warning")
            .contains("Effective provider allowlist excludes litellm"));
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
            anthropic_routes: Arc::new(Mutex::new(default_anthropic_routes())),
            operator_tokens: Arc::new(Mutex::new(vec![TEST_OPERATOR_TOKEN.to_owned()])),
            events: Arc::new(Mutex::new(Vec::new())),
            audit_events: Arc::new(Mutex::new(Vec::new())),
            portal_members: Arc::new(Mutex::new(Vec::new())),
            service_memberships: Arc::new(Mutex::new(Vec::new())),
            managed_identities: Arc::new(Mutex::new(Vec::new())),
            oidc_transactions: Arc::new(Mutex::new(Vec::new())),
            portal_sessions: Arc::new(Mutex::new(Vec::new())),
            postgres_ready: true,
            studio_connection: Arc::new(Mutex::new(None)),
            gateway_auth_settings: Arc::new(Mutex::new(None)),
            litellm_passthrough_settings: Arc::new(Mutex::new(
                LiteLlmPassthroughSettings::default_with_updated_at(Utc::now()),
            )),
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
            anthropic_routes: Arc::new(Mutex::new(default_anthropic_routes())),
            operator_tokens: Arc::new(Mutex::new(vec![TEST_OPERATOR_TOKEN.to_owned()])),
            events: Arc::new(Mutex::new(Vec::new())),
            audit_events: Arc::new(Mutex::new(Vec::new())),
            portal_members: Arc::new(Mutex::new(Vec::new())),
            service_memberships: Arc::new(Mutex::new(Vec::new())),
            managed_identities: Arc::new(Mutex::new(Vec::new())),
            oidc_transactions: Arc::new(Mutex::new(Vec::new())),
            portal_sessions: Arc::new(Mutex::new(Vec::new())),
            postgres_ready: true,
            studio_connection: Arc::new(Mutex::new(None)),
            gateway_auth_settings: Arc::new(Mutex::new(None)),
            litellm_passthrough_settings: Arc::new(Mutex::new(
                LiteLlmPassthroughSettings::default_with_updated_at(Utc::now()),
            )),
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
            anthropic_routes: Arc::new(Mutex::new(default_anthropic_routes())),
            operator_tokens: Arc::new(Mutex::new(vec![TEST_POLICY_OPERATOR_TOKEN.to_owned()])),
            events: Arc::new(Mutex::new(Vec::new())),
            audit_events: Arc::new(Mutex::new(Vec::new())),
            portal_members: Arc::new(Mutex::new(Vec::new())),
            service_memberships: Arc::new(Mutex::new(Vec::new())),
            managed_identities: Arc::new(Mutex::new(Vec::new())),
            oidc_transactions: Arc::new(Mutex::new(Vec::new())),
            portal_sessions: Arc::new(Mutex::new(Vec::new())),
            postgres_ready: true,
            studio_connection: Arc::new(Mutex::new(None)),
            gateway_auth_settings: Arc::new(Mutex::new(None)),
            litellm_passthrough_settings: Arc::new(Mutex::new(
                LiteLlmPassthroughSettings::default_with_updated_at(Utc::now()),
            )),
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
            anthropic_routes: Arc::new(Mutex::new(default_anthropic_routes())),
            operator_tokens: Arc::new(Mutex::new(vec![TEST_OPERATOR_TOKEN.to_owned()])),
            events: Arc::new(Mutex::new(Vec::new())),
            audit_events: Arc::new(Mutex::new(Vec::new())),
            portal_members: Arc::new(Mutex::new(Vec::new())),
            service_memberships: Arc::new(Mutex::new(Vec::new())),
            managed_identities: Arc::new(Mutex::new(Vec::new())),
            oidc_transactions: Arc::new(Mutex::new(Vec::new())),
            portal_sessions: Arc::new(Mutex::new(Vec::new())),
            postgres_ready: true,
            studio_connection: Arc::new(Mutex::new(None)),
            gateway_auth_settings: Arc::new(Mutex::new(None)),
            litellm_passthrough_settings: Arc::new(Mutex::new(
                LiteLlmPassthroughSettings::default_with_updated_at(Utc::now()),
            )),
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
            anthropic_routes: Arc::new(Mutex::new(default_anthropic_routes())),
            operator_tokens: Arc::new(Mutex::new(vec![TEST_OPERATOR_TOKEN.to_owned()])),
            events: Arc::new(Mutex::new(Vec::new())),
            audit_events: Arc::new(Mutex::new(Vec::new())),
            portal_members: Arc::new(Mutex::new(Vec::new())),
            service_memberships: Arc::new(Mutex::new(Vec::new())),
            managed_identities: Arc::new(Mutex::new(Vec::new())),
            oidc_transactions: Arc::new(Mutex::new(Vec::new())),
            portal_sessions: Arc::new(Mutex::new(Vec::new())),
            postgres_ready: true,
            studio_connection: Arc::new(Mutex::new(None)),
            gateway_auth_settings: Arc::new(Mutex::new(None)),
            litellm_passthrough_settings: Arc::new(Mutex::new(
                LiteLlmPassthroughSettings::default_with_updated_at(Utc::now()),
            )),
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
            anthropic_routes: Arc::new(Mutex::new(default_anthropic_routes())),
            operator_tokens: Arc::new(Mutex::new(vec![TEST_OPERATOR_TOKEN.to_owned()])),
            events: Arc::new(Mutex::new(Vec::new())),
            audit_events: Arc::new(Mutex::new(Vec::new())),
            portal_members: Arc::new(Mutex::new(Vec::new())),
            service_memberships: Arc::new(Mutex::new(Vec::new())),
            managed_identities: Arc::new(Mutex::new(Vec::new())),
            oidc_transactions: Arc::new(Mutex::new(Vec::new())),
            portal_sessions: Arc::new(Mutex::new(Vec::new())),
            postgres_ready: true,
            studio_connection: Arc::new(Mutex::new(None)),
            gateway_auth_settings: Arc::new(Mutex::new(None)),
            litellm_passthrough_settings: Arc::new(Mutex::new(
                LiteLlmPassthroughSettings::default_with_updated_at(Utc::now()),
            )),
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
            anthropic_routes: Arc::new(Mutex::new(default_anthropic_routes())),
            operator_tokens: Arc::new(Mutex::new(vec![TEST_OPERATOR_TOKEN.to_owned()])),
            events: Arc::new(Mutex::new(Vec::new())),
            audit_events: Arc::new(Mutex::new(Vec::new())),
            portal_members: Arc::new(Mutex::new(Vec::new())),
            service_memberships: Arc::new(Mutex::new(Vec::new())),
            managed_identities: Arc::new(Mutex::new(Vec::new())),
            oidc_transactions: Arc::new(Mutex::new(Vec::new())),
            portal_sessions: Arc::new(Mutex::new(Vec::new())),
            postgres_ready: true,
            studio_connection: Arc::new(Mutex::new(None)),
            gateway_auth_settings: Arc::new(Mutex::new(None)),
            litellm_passthrough_settings: Arc::new(Mutex::new(
                LiteLlmPassthroughSettings::default_with_updated_at(Utc::now()),
            )),
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
            anthropic_routes: Arc::new(Mutex::new(default_anthropic_routes())),
            operator_tokens: Arc::new(Mutex::new(vec![TEST_OPERATOR_TOKEN.to_owned()])),
            events: Arc::new(Mutex::new(Vec::new())),
            audit_events: Arc::new(Mutex::new(Vec::new())),
            portal_members: Arc::new(Mutex::new(Vec::new())),
            service_memberships: Arc::new(Mutex::new(Vec::new())),
            managed_identities: Arc::new(Mutex::new(Vec::new())),
            oidc_transactions: Arc::new(Mutex::new(Vec::new())),
            portal_sessions: Arc::new(Mutex::new(Vec::new())),
            postgres_ready: true,
            studio_connection: Arc::new(Mutex::new(None)),
            gateway_auth_settings: Arc::new(Mutex::new(None)),
            litellm_passthrough_settings: Arc::new(Mutex::new(
                LiteLlmPassthroughSettings::default_with_updated_at(Utc::now()),
            )),
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
    async fn missing_debug_bundle_returns_specific_error() {
        let app = router_with_state(test_state(default_store()));
        let response = admin_get(
            app,
            "/admin-ui/admin/debug-bundles/mock-service-ts-0004",
            Some(TEST_OPERATOR_TOKEN),
        )
        .await;

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let value: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(value["error"]["code"], "debug_bundle_not_found");
    }

    #[tokio::test]
    async fn task_usage_requires_admin_token_and_returns_summary() {
        let store = MemoryStore {
            key: Arc::new(Mutex::new(None)),
            admin_key: Arc::new(Mutex::new(None)),
            services: Arc::new(Mutex::new(Vec::new())),
            openai_routes: Arc::new(Mutex::new(default_openai_routes())),
            anthropic_routes: Arc::new(Mutex::new(default_anthropic_routes())),
            operator_tokens: Arc::new(Mutex::new(vec![TEST_OPERATOR_TOKEN.to_owned()])),
            events: Arc::new(Mutex::new(Vec::new())),
            audit_events: Arc::new(Mutex::new(Vec::new())),
            portal_members: Arc::new(Mutex::new(Vec::new())),
            service_memberships: Arc::new(Mutex::new(Vec::new())),
            managed_identities: Arc::new(Mutex::new(Vec::new())),
            oidc_transactions: Arc::new(Mutex::new(Vec::new())),
            portal_sessions: Arc::new(Mutex::new(Vec::new())),
            postgres_ready: true,
            studio_connection: Arc::new(Mutex::new(None)),
            gateway_auth_settings: Arc::new(Mutex::new(None)),
            litellm_passthrough_settings: Arc::new(Mutex::new(
                LiteLlmPassthroughSettings::default_with_updated_at(Utc::now()),
            )),
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
                cost_source: Some("upstream_passthrough".to_owned()),
                cost_mode: Some(gateway_core::ServiceCostMode::Passthrough),
                pricing_rule_name: None,
                service_name: None,
                http_method: None,
                endpoint_path: None,
                endpoint_template: None,
                task_id: Some("task-1".to_owned()),
                run_id: Some("run-1".to_owned()),
                trace_id: Some("4bf92f3577b34da6a3ce929d0e0e4736".to_owned()),
                fallback_count: 1,
                created_at: Utc::now(),
            },
            UsageEvent {
                request_id: "=req-failure".to_owned(),
                key_id,
                project_id: Some(project_id),
                route: Route::ServiceWildcard,
                model: Some("gpt-test".to_owned()),
                provider: gateway_core::Provider::InternalService,
                status: UsageStatus::Failure,
                status_code: 502,
                latency_ms: 50,
                input_tokens: None,
                output_tokens: None,
                total_tokens: None,
                estimated_cost_usd: None,
                cost_source: None,
                cost_mode: None,
                pricing_rule_name: None,
                service_name: Some("jobs-service".to_owned()),
                http_method: Some("POST".to_owned()),
                endpoint_path: Some("/jobs/failed-1".to_owned()),
                endpoint_template: Some("/jobs/{job_id}".to_owned()),
                task_id: Some("task-1".to_owned()),
                run_id: Some("run-2".to_owned()),
                trace_id: None,
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
            app.clone(),
            "/admin-ui/admin/usage/events?method=post&endpoint=%2Fjobs%2F%7Bjob_id%7D&status_code=502",
            Some(TEST_OPERATOR_TOKEN),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let value: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(value["rows"][0]["http_method"], "POST");
        assert_eq!(value["rows"][0]["endpoint_path"], "/jobs/failed-1");
        assert_eq!(value["rows"][0]["endpoint_template"], "/jobs/{job_id}");
        assert_eq!(value["rows"][0]["status_code"], 502);

        let response = admin_get(
            app.clone(),
            "/admin-ui/admin/usage/dashboard?status=failure&sort_by=failures",
            Some(TEST_OPERATOR_TOKEN),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let value: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(
            value["breakdowns"]["endpoints"][0]["name"],
            "POST /jobs/{job_id}"
        );
        assert_eq!(
            value["breakdowns"]["endpoints"][0]["summary"]["failure_count"],
            1
        );

        let response = admin_get(
            app.clone(),
            "/admin-ui/admin/usage/filter-values?field=endpoint&status=failure&status_code=502",
            Some(TEST_OPERATOR_TOKEN),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let value: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(value["values"], serde_json::json!(["/jobs/{job_id}"]));

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
        assert!(csv
            .lines()
            .next()
            .expect("header")
            .ends_with("pricing_rule_name,http_method,endpoint_path,endpoint_template"));
        assert!(csv.contains("'=req-failure"));
        assert!(csv.contains("POST,/jobs/failed-1,/jobs/{job_id}"));
        assert!(!csv.contains("req-success"));
    }

    #[tokio::test]
    async fn usage_dashboard_returns_service_timeseries() {
        let store = default_store();
        let key_id = Uuid::new_v4();
        let project_id = Uuid::new_v4();
        let bucket = chrono::DateTime::parse_from_rfc3339("2026-07-04T15:22:00Z")
            .expect("time")
            .with_timezone(&Utc);
        store.events.lock().expect("events lock").extend([
            UsageEvent {
                request_id: "svc-fixed".to_owned(),
                key_id,
                project_id: Some(project_id),
                route: Route::Summary,
                model: None,
                provider: gateway_core::Provider::InternalService,
                status: UsageStatus::Success,
                status_code: 200,
                latency_ms: 120,
                input_tokens: None,
                output_tokens: None,
                total_tokens: None,
                estimated_cost_usd: Some(0.026),
                cost_source: Some("service_default".to_owned()),
                cost_mode: Some(ServiceCostMode::Fixed),
                pricing_rule_name: None,
                service_name: Some("summarizer".to_owned()),
                http_method: Some("POST".to_owned()),
                endpoint_path: Some("/summaries/summary-1".to_owned()),
                endpoint_template: Some("/summaries/{summary_id}".to_owned()),
                task_id: None,
                run_id: None,
                trace_id: Some("trace-fixed".to_owned()),
                fallback_count: 0,
                created_at: bucket,
            },
            UsageEvent {
                request_id: "svc-rule".to_owned(),
                key_id,
                project_id: Some(project_id),
                route: Route::Translation,
                model: None,
                provider: gateway_core::Provider::InternalService,
                status: UsageStatus::Success,
                status_code: 200,
                latency_ms: 180,
                input_tokens: None,
                output_tokens: None,
                total_tokens: None,
                estimated_cost_usd: Some(0.042),
                cost_source: Some("pricing_rule".to_owned()),
                cost_mode: Some(ServiceCostMode::Fixed),
                pricing_rule_name: Some("legal-es".to_owned()),
                service_name: Some("translation".to_owned()),
                http_method: Some("POST".to_owned()),
                endpoint_path: Some("/translations/translation-1".to_owned()),
                endpoint_template: Some("/translations/{translation_id}".to_owned()),
                task_id: None,
                run_id: None,
                trace_id: Some("trace-rule".to_owned()),
                fallback_count: 0,
                created_at: bucket,
            },
        ]);
        let app = router_with_state(test_state(store));

        let response = admin_get(
            app,
            "/admin-ui/admin/usage/dashboard?interval=hour",
            Some(TEST_OPERATOR_TOKEN),
        )
        .await;

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let value: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(
            value["service_timeseries"].as_array().expect("rows").len(),
            2
        );
        assert_eq!(
            value["service_timeseries"][0]["bucket"],
            "2026-07-04T15:00:00Z"
        );
        assert_eq!(value["service_timeseries"][0]["service_name"], "summarizer");
        assert_eq!(
            value["service_timeseries"][0]["summary"]["estimated_cost_usd"],
            0.026
        );
        assert_eq!(
            value["service_timeseries"][1]["service_name"],
            "translation"
        );
        assert_eq!(
            value["service_timeseries"][1]["summary"]["estimated_cost_usd"],
            0.042
        );
    }

    #[tokio::test]
    async fn usage_dashboard_paginates_timeseries_sections() {
        let store = default_store();
        let key_id = Uuid::new_v4();
        let project_id = Uuid::new_v4();
        let first_bucket = chrono::DateTime::parse_from_rfc3339("2026-07-04T15:22:00Z")
            .expect("time")
            .with_timezone(&Utc);
        let second_bucket = chrono::DateTime::parse_from_rfc3339("2026-07-04T16:05:00Z")
            .expect("time")
            .with_timezone(&Utc);
        store.events.lock().expect("events lock").extend([
            UsageEvent {
                request_id: "svc-fixed".to_owned(),
                key_id,
                project_id: Some(project_id),
                route: Route::Summary,
                model: None,
                provider: gateway_core::Provider::InternalService,
                status: UsageStatus::Success,
                status_code: 200,
                latency_ms: 120,
                input_tokens: None,
                output_tokens: None,
                total_tokens: None,
                estimated_cost_usd: Some(0.026),
                cost_source: Some("service_default".to_owned()),
                cost_mode: Some(ServiceCostMode::Fixed),
                pricing_rule_name: None,
                service_name: Some("summarizer".to_owned()),
                http_method: Some("POST".to_owned()),
                endpoint_path: Some("/summaries/summary-1".to_owned()),
                endpoint_template: Some("/summaries/{summary_id}".to_owned()),
                task_id: None,
                run_id: None,
                trace_id: Some("trace-fixed".to_owned()),
                fallback_count: 0,
                created_at: first_bucket,
            },
            UsageEvent {
                request_id: "svc-rule".to_owned(),
                key_id,
                project_id: Some(project_id),
                route: Route::Translation,
                model: None,
                provider: gateway_core::Provider::InternalService,
                status: UsageStatus::Success,
                status_code: 200,
                latency_ms: 180,
                input_tokens: None,
                output_tokens: None,
                total_tokens: None,
                estimated_cost_usd: Some(0.042),
                cost_source: Some("pricing_rule".to_owned()),
                cost_mode: Some(ServiceCostMode::Fixed),
                pricing_rule_name: Some("legal-es".to_owned()),
                service_name: Some("translation".to_owned()),
                http_method: Some("POST".to_owned()),
                endpoint_path: Some("/translations/translation-1".to_owned()),
                endpoint_template: Some("/translations/{translation_id}".to_owned()),
                task_id: None,
                run_id: None,
                trace_id: Some("trace-rule".to_owned()),
                fallback_count: 0,
                created_at: first_bucket,
            },
            UsageEvent {
                request_id: "svc-next".to_owned(),
                key_id,
                project_id: Some(project_id),
                route: Route::Summary,
                model: None,
                provider: gateway_core::Provider::InternalService,
                status: UsageStatus::Failure,
                status_code: 500,
                latency_ms: 240,
                input_tokens: None,
                output_tokens: None,
                total_tokens: None,
                estimated_cost_usd: Some(0.018),
                cost_source: Some("service_default".to_owned()),
                cost_mode: Some(ServiceCostMode::Fixed),
                pricing_rule_name: None,
                service_name: Some("summarizer".to_owned()),
                http_method: Some("POST".to_owned()),
                endpoint_path: Some("/summaries/summary-2".to_owned()),
                endpoint_template: Some("/summaries/{summary_id}".to_owned()),
                task_id: None,
                run_id: None,
                trace_id: Some("trace-next".to_owned()),
                fallback_count: 0,
                created_at: second_bucket,
            },
        ]);
        let app = router_with_state(test_state(store));

        let response = admin_get(
            app,
            "/admin-ui/admin/usage/dashboard?interval=hour&timeseries_limit=1&service_timeseries_limit=1",
            Some(TEST_OPERATOR_TOKEN),
        )
        .await;

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let value: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(value["timeseries"].as_array().expect("rows").len(), 1);
        assert_eq!(value["timeseries_page"]["limit"], 1);
        assert_eq!(value["timeseries_page"]["offset"], 0);
        assert_eq!(value["timeseries_page"]["has_more"], true);
        assert_eq!(
            value["service_timeseries"].as_array().expect("rows").len(),
            1
        );
        assert_eq!(value["service_timeseries_page"]["limit"], 1);
        assert_eq!(value["service_timeseries_page"]["offset"], 0);
        assert_eq!(value["service_timeseries_page"]["has_more"], true);
    }

    #[tokio::test]
    async fn admin_ui_assets_are_served_without_exposing_operator_token() {
        let store = MemoryStore {
            key: Arc::new(Mutex::new(None)),
            admin_key: Arc::new(Mutex::new(None)),
            services: Arc::new(Mutex::new(Vec::new())),
            openai_routes: Arc::new(Mutex::new(default_openai_routes())),
            anthropic_routes: Arc::new(Mutex::new(default_anthropic_routes())),
            operator_tokens: Arc::new(Mutex::new(vec![TEST_OPERATOR_TOKEN.to_owned()])),
            events: Arc::new(Mutex::new(Vec::new())),
            audit_events: Arc::new(Mutex::new(Vec::new())),
            portal_members: Arc::new(Mutex::new(Vec::new())),
            service_memberships: Arc::new(Mutex::new(Vec::new())),
            managed_identities: Arc::new(Mutex::new(Vec::new())),
            oidc_transactions: Arc::new(Mutex::new(Vec::new())),
            portal_sessions: Arc::new(Mutex::new(Vec::new())),
            postgres_ready: true,
            studio_connection: Arc::new(Mutex::new(None)),
            gateway_auth_settings: Arc::new(Mutex::new(None)),
            litellm_passthrough_settings: Arc::new(Mutex::new(
                LiteLlmPassthroughSettings::default_with_updated_at(Utc::now()),
            )),
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

        let response = request(app.clone(), "/admin-ui/app.css").await;
        assert_eq!(response.status(), StatusCode::OK);

        let response = request(app.clone(), "/admin-ui/microsoft-sign-in.svg").await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE),
            Some(&HeaderValue::from_static("image/svg+xml"))
        );

        for (path, content_type) in [
            ("admin-ui-tabler-icons.woff2", "font/woff2"),
            ("admin-ui-tabler-icons.woff", "font/woff"),
            ("admin-ui-tabler-icons.ttf", "font/ttf"),
        ] {
            let response = request(app.clone(), &format!("/admin-ui/{path}")).await;
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(
                response.headers().get(header::CONTENT_TYPE),
                Some(&HeaderValue::from_static(content_type))
            );
            assert_eq!(
                response.headers().get(header::CACHE_CONTROL),
                Some(&HeaderValue::from_static(
                    "public, max-age=31536000, immutable"
                ))
            );
        }
    }

    #[tokio::test]
    async fn operator_token_rotation_returns_new_raw_token_once_and_invalidates_old_token() {
        let store = MemoryStore {
            key: Arc::new(Mutex::new(None)),
            admin_key: Arc::new(Mutex::new(None)),
            services: Arc::new(Mutex::new(Vec::new())),
            openai_routes: Arc::new(Mutex::new(default_openai_routes())),
            anthropic_routes: Arc::new(Mutex::new(default_anthropic_routes())),
            operator_tokens: Arc::new(Mutex::new(vec![TEST_OPERATOR_TOKEN.to_owned()])),
            events: Arc::new(Mutex::new(Vec::new())),
            audit_events: Arc::new(Mutex::new(Vec::new())),
            portal_members: Arc::new(Mutex::new(Vec::new())),
            service_memberships: Arc::new(Mutex::new(Vec::new())),
            managed_identities: Arc::new(Mutex::new(Vec::new())),
            oidc_transactions: Arc::new(Mutex::new(Vec::new())),
            portal_sessions: Arc::new(Mutex::new(Vec::new())),
            postgres_ready: true,
            studio_connection: Arc::new(Mutex::new(None)),
            gateway_auth_settings: Arc::new(Mutex::new(None)),
            litellm_passthrough_settings: Arc::new(Mutex::new(
                LiteLlmPassthroughSettings::default_with_updated_at(Utc::now()),
            )),
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
    async fn gateway_auth_settings_can_be_patched_without_exposing_secret() {
        let state = test_state(default_store());
        let auth_runtime = state.auth_runtime.clone();
        let app = router_with_state(state);
        let response = admin_patch(
            app,
            "/admin-ui/admin/auth/front-door",
            Some(TEST_OPERATOR_TOKEN),
            r#"{
                "entra_enabled": true,
                "apigee_trusted_header_enabled": true,
                "relayna_key_header": "X-Relayna-Key",
                "tenant_id": "tenant-1",
                "audience": "api://relayna-gateway",
                "issuer": "https://login.example/tenant-1/v2.0",
                "oidc_discovery_url": "https://login.example/tenant-1/.well-known/openid-configuration",
                "required_scope": "gateway.invoke",
                "required_role": "Gateway.Invoke",
                "allowed_groups": ["gateway-users"],
                "accepted_algorithms": ["RS256"],
                "jwks_cache_ttl_seconds": 120,
                "clock_skew_seconds": 30,
                "apigee_trusted_header_secret": "apigee-secret"
            }"#,
        )
        .await;

        assert_eq!(response.status(), StatusCode::OK);
        let value = response_json(response).await;
        assert_eq!(value["source"], "persisted");
        assert_eq!(value["entra"]["enabled"], true);
        assert_eq!(value["entra"]["required_scope"], "gateway.invoke");
        assert_eq!(value["apigee"]["trusted_header_enabled"], true);
        assert_eq!(value["apigee"]["secret_configured"], true);
        assert!(value["apigee"].get("secret").is_none());

        let snapshot = auth_runtime.snapshot().expect("auth snapshot");
        assert!(snapshot.entra_enabled());
        assert_eq!(snapshot.config.relayna_key_header, "X-Relayna-Key");
        assert!(snapshot.entra_verifier.is_some());
    }

    #[tokio::test]
    async fn gateway_auth_settings_reject_enabled_entra_without_required_fields() {
        let app = router_with_state(test_state(default_store()));
        let response = admin_patch(
            app,
            "/admin-ui/admin/auth/front-door",
            Some(TEST_OPERATOR_TOKEN),
            r#"{"entra_enabled":true}"#,
        )
        .await;
        let status = response.status();
        let value = response_json(response).await;

        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(value["error"]["code"], "invalid_configuration");
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
            anthropic_routes: Arc::new(Mutex::new(default_anthropic_routes())),
            operator_tokens: Arc::new(Mutex::new(vec![TEST_OPERATOR_TOKEN.to_owned()])),
            events: Arc::new(Mutex::new(Vec::new())),
            audit_events: Arc::new(Mutex::new(Vec::new())),
            portal_members: Arc::new(Mutex::new(Vec::new())),
            service_memberships: Arc::new(Mutex::new(Vec::new())),
            managed_identities: Arc::new(Mutex::new(Vec::new())),
            oidc_transactions: Arc::new(Mutex::new(Vec::new())),
            portal_sessions: Arc::new(Mutex::new(Vec::new())),
            postgres_ready: true,
            studio_connection: Arc::new(Mutex::new(None)),
            gateway_auth_settings: Arc::new(Mutex::new(None)),
            litellm_passthrough_settings: Arc::new(Mutex::new(
                LiteLlmPassthroughSettings::default_with_updated_at(Utc::now()),
            )),
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
        assert_eq!(value.as_array().expect("routes").len(), 3);
        assert_eq!(value[0]["mode"], "managed_by_gateway");

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

        let response = admin_patch(
            app.clone(),
            "/admin-ui/admin/openai-routes/chat-completions/mode",
            Some(TEST_OPERATOR_TOKEN),
            r#"{"mode":"direct_litellm_passthrough"}"#,
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let value: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(value["mode"], "direct_litellm_passthrough");

        let response = admin_patch(
            app.clone(),
            "/admin-ui/admin/openai-routes/responses/config",
            Some(TEST_OPERATOR_TOKEN),
            r#"{
                "mode":"direct_litellm_passthrough",
                "timeout_ms":240000,
                "max_request_body_bytes":8388608,
                "max_response_body_bytes":4194304
            }"#,
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let value = response_json(response).await;
        assert_eq!(value["route_id"], "responses");
        assert_eq!(value["mode"], "direct_litellm_passthrough");
        assert_eq!(value["timeout_ms"], 240000);
        assert_eq!(value["max_request_body_bytes"], 8388608);
        assert_eq!(value["max_response_body_bytes"], 4194304);

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
    async fn anthropic_route_settings_can_be_listed_and_toggled() {
        let store = default_store();
        let app = router_with_state(test_state(store));

        let response = admin_get(
            app.clone(),
            "/admin-ui/admin/anthropic-routes",
            Some(TEST_OPERATOR_TOKEN),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let value = response_json(response).await;
        assert_eq!(value.as_array().expect("routes").len(), 7);
        assert_eq!(value[0]["mode"], "managed_by_gateway");

        let response = admin_post(
            app.clone(),
            "/admin-ui/admin/anthropic-routes/messages/disable",
            Some(TEST_OPERATOR_TOKEN),
            "{}",
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let value = response_json(response).await;
        assert_eq!(value["route_id"], "messages");
        assert_eq!(value["enabled"], false);

        let response = admin_patch(
            app.clone(),
            "/admin-ui/admin/anthropic-routes/messages/mode",
            Some(TEST_OPERATOR_TOKEN),
            r#"{"mode":"direct_litellm_passthrough"}"#,
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let value = response_json(response).await;
        assert_eq!(value["mode"], "direct_litellm_passthrough");

        let response = admin_patch(
            app.clone(),
            "/admin-ui/admin/anthropic-routes/messages/config",
            Some(TEST_OPERATOR_TOKEN),
            r#"{
                "timeout_ms":180000,
                "max_request_body_bytes":6291456,
                "max_response_body_bytes":3145728
            }"#,
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let value = response_json(response).await;
        assert_eq!(value["route_id"], "messages");
        assert_eq!(value["timeout_ms"], 180000);
        assert_eq!(value["max_request_body_bytes"], 6291456);
        assert_eq!(value["max_response_body_bytes"], 3145728);

        let response = admin_post(
            app,
            "/admin-ui/admin/anthropic-routes/messages/enable",
            Some(TEST_OPERATOR_TOKEN),
            "{}",
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let value = response_json(response).await;
        assert_eq!(value["enabled"], true);
    }

    #[tokio::test]
    async fn litellm_passthrough_settings_can_be_read_and_patched() {
        let store = default_store();
        let audit_events = store.audit_events.clone();
        let app = router_with_state(test_state(store));

        let response = admin_get(
            app.clone(),
            "/admin-ui/admin/providers/litellm-passthrough",
            Some(TEST_OPERATOR_TOKEN),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let value: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(value["enabled"], false);
        assert_eq!(value["allowed_paths"][0], "/v1/*");

        let response = admin_patch(
            app,
            "/admin-ui/admin/providers/litellm-passthrough",
            Some(TEST_OPERATOR_TOKEN),
            r#"{
                "enabled": true,
                "allowed_paths": ["/v1/*", "/ui"],
                "allowed_methods": ["GET", "POST"],
                "ui_exposure": "operator_only",
                "admin_api_exposure": "disabled",
                "timeout_ms": 240000,
                "max_request_body_bytes": 8388608,
                "max_response_body_bytes": 4194304
            }"#,
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let value: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(value["enabled"], true);
        assert_eq!(value["ui_exposure"], "operator_only");
        assert_eq!(value["timeout_ms"], 240000);
        assert_eq!(value["max_request_body_bytes"], 8388608);
        assert_eq!(value["max_response_body_bytes"], 4194304);
        assert!(value.get("credential").is_none());

        let events = audit_events.lock().expect("audit events lock");
        assert!(events
            .iter()
            .any(|event| event.action == "providers:litellm_passthrough_update"));
    }

    #[tokio::test]
    async fn admin_service_create_redacts_raw_credential() {
        let store = MemoryStore {
            key: Arc::new(Mutex::new(None)),
            admin_key: Arc::new(Mutex::new(None)),
            services: Arc::new(Mutex::new(Vec::new())),
            openai_routes: Arc::new(Mutex::new(default_openai_routes())),
            anthropic_routes: Arc::new(Mutex::new(default_anthropic_routes())),
            operator_tokens: Arc::new(Mutex::new(vec![TEST_OPERATOR_TOKEN.to_owned()])),
            events: Arc::new(Mutex::new(Vec::new())),
            audit_events: Arc::new(Mutex::new(Vec::new())),
            portal_members: Arc::new(Mutex::new(Vec::new())),
            service_memberships: Arc::new(Mutex::new(Vec::new())),
            managed_identities: Arc::new(Mutex::new(Vec::new())),
            oidc_transactions: Arc::new(Mutex::new(Vec::new())),
            portal_sessions: Arc::new(Mutex::new(Vec::new())),
            postgres_ready: true,
            studio_connection: Arc::new(Mutex::new(None)),
            gateway_auth_settings: Arc::new(Mutex::new(None)),
            litellm_passthrough_settings: Arc::new(Mutex::new(
                LiteLlmPassthroughSettings::default_with_updated_at(Utc::now()),
            )),
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
                "health_check_path":"/relayna/capabilities",
                "health_check_method":"HEAD",
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
        assert_eq!(value["health_check_path"], "/relayna/capabilities");
        assert_eq!(value["health_check_method"], "HEAD");
        assert_eq!(value["credential_configured"], true);
        assert!(value.get("credential").is_none());
    }

    #[test]
    fn health_check_url_appends_configured_service_path() {
        let url = health_check_url(
            "http://example.internal:8080/base?old=true",
            Some("/relayna/capabilities"),
        )
        .expect("url");

        assert_eq!(
            url.as_str(),
            "http://example.internal:8080/relayna/capabilities"
        );
    }

    #[tokio::test]
    async fn admin_service_import_reports_incomplete_runtime_fields() {
        let store = MemoryStore {
            key: Arc::new(Mutex::new(None)),
            admin_key: Arc::new(Mutex::new(None)),
            services: Arc::new(Mutex::new(Vec::new())),
            openai_routes: Arc::new(Mutex::new(default_openai_routes())),
            anthropic_routes: Arc::new(Mutex::new(default_anthropic_routes())),
            operator_tokens: Arc::new(Mutex::new(vec![TEST_OPERATOR_TOKEN.to_owned()])),
            events: Arc::new(Mutex::new(Vec::new())),
            audit_events: Arc::new(Mutex::new(Vec::new())),
            portal_members: Arc::new(Mutex::new(Vec::new())),
            service_memberships: Arc::new(Mutex::new(Vec::new())),
            managed_identities: Arc::new(Mutex::new(Vec::new())),
            oidc_transactions: Arc::new(Mutex::new(Vec::new())),
            portal_sessions: Arc::new(Mutex::new(Vec::new())),
            postgres_ready: true,
            studio_connection: Arc::new(Mutex::new(None)),
            gateway_auth_settings: Arc::new(Mutex::new(None)),
            litellm_passthrough_settings: Arc::new(Mutex::new(
                LiteLlmPassthroughSettings::default_with_updated_at(Utc::now()),
            )),
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
            anthropic_routes: Arc::new(Mutex::new(default_anthropic_routes())),
            operator_tokens: Arc::new(Mutex::new(vec![TEST_OPERATOR_TOKEN.to_owned()])),
            events: Arc::new(Mutex::new(Vec::new())),
            audit_events: Arc::new(Mutex::new(Vec::new())),
            portal_members: Arc::new(Mutex::new(Vec::new())),
            service_memberships: Arc::new(Mutex::new(Vec::new())),
            managed_identities: Arc::new(Mutex::new(Vec::new())),
            oidc_transactions: Arc::new(Mutex::new(Vec::new())),
            portal_sessions: Arc::new(Mutex::new(Vec::new())),
            postgres_ready: true,
            studio_connection: Arc::new(Mutex::new(None)),
            gateway_auth_settings: Arc::new(Mutex::new(None)),
            litellm_passthrough_settings: Arc::new(Mutex::new(
                LiteLlmPassthroughSettings::default_with_updated_at(Utc::now()),
            )),
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
                "health_check_path":"/healthz",
                "health_check_method":"HEAD",
                "credential":"token",
                "enabled":true,
                "allowed_methods":["POST"],
                "timeout_ms":123456
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
                "health_check_path":"/relayna/capabilities",
                "health_check_method":"GET",
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
        assert_eq!(value["health_check_path"], "/healthz");
        assert_eq!(value["health_check_method"], "HEAD");
        assert_eq!(value["allowed_methods"][0], "POST");
        assert_eq!(value["timeout_ms"], 123456);
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
            anthropic_routes: Arc::new(Mutex::new(default_anthropic_routes())),
            operator_tokens: Arc::new(Mutex::new(vec![TEST_OPERATOR_TOKEN.to_owned()])),
            events: Arc::new(Mutex::new(Vec::new())),
            audit_events: Arc::new(Mutex::new(Vec::new())),
            portal_members: Arc::new(Mutex::new(Vec::new())),
            service_memberships: Arc::new(Mutex::new(Vec::new())),
            managed_identities: Arc::new(Mutex::new(Vec::new())),
            oidc_transactions: Arc::new(Mutex::new(Vec::new())),
            portal_sessions: Arc::new(Mutex::new(Vec::new())),
            postgres_ready: true,
            studio_connection: Arc::new(Mutex::new(None)),
            gateway_auth_settings: Arc::new(Mutex::new(None)),
            litellm_passthrough_settings: Arc::new(Mutex::new(
                LiteLlmPassthroughSettings::default_with_updated_at(Utc::now()),
            )),
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

    #[tokio::test]
    async fn admin_can_change_relayna_endpoint_price_from_none_to_fixed() {
        let app = router_with_state(test_state(default_store()));
        let response = admin_post(
            app.clone(),
            "/admin-ui/admin/services",
            Some(TEST_OPERATOR_TOKEN),
            r#"{
                "name":"ocr",
                "route_pattern":"/services/ocr/*",
                "upstream_base_url":"http://ocr.internal:8000",
                "credential":"token",
                "allowed_methods":["GET","POST"],
                "cost_mode":"fixed",
                "estimated_cost_usd":0.01,
                "openapi_source_path":"/openapi.json",
                "openapi_endpoints":[{
                    "method":"GET",
                    "path_template":"/events/feed",
                    "operation_id":"feed_events_feed_get",
                    "relayna_default":true
                }],
                "endpoint_pricing_rules":[{
                    "method":"GET",
                    "path_template":"/events/feed",
                    "operation_id":"feed_events_feed_get",
                    "cost_mode":"none"
                }]
            }"#,
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);

        let response = admin_patch(
            app,
            "/admin-ui/admin/services/ocr",
            Some(TEST_OPERATOR_TOKEN),
            r#"{
                "endpoint_pricing_rules":[{
                    "method":"GET",
                    "path_template":"/events/feed",
                    "operation_id":"feed_events_feed_get",
                    "cost_mode":"fixed",
                    "estimated_cost_usd":0.02
                }]
            }"#,
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let value: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(value["endpoint_pricing_rules"][0]["cost_mode"], "fixed");
        assert_eq!(
            value["endpoint_pricing_rules"][0]["estimated_cost_usd"],
            0.02
        );
    }

    #[tokio::test]
    async fn health_import_and_litellm_helpers_cover_failure_and_rewrite_edges() {
        let client = reqwest::Client::new();
        let missing = active_health_check(&client, "missing", None, None, "GET", None).await;
        assert!(!missing.ok);
        assert_eq!(missing.error_code.as_deref(), Some("missing_upstream_url"));
        let invalid = active_health_check(
            &client,
            "invalid",
            Some("not-a-url".to_owned()),
            None,
            "GET",
            None,
        )
        .await;
        assert_eq!(invalid.error_code.as_deref(), Some("invalid_upstream_url"));

        let (base_url, captured) = spawn_litellm_server(
            "503 Service Unavailable",
            vec![("Content-Type", "application/json")],
            r#"{"error":"unavailable"}"#,
        );
        let failed = active_health_check(
            &client,
            "provider",
            Some(base_url),
            Some("/health"),
            "HEAD",
            Some("health-secret"),
        )
        .await;
        assert!(!failed.ok);
        assert_eq!(failed.error_code.as_deref(), Some("http_503"));
        let request = captured.recv().expect("captured health request");
        assert!(request.request_line.starts_with("HEAD /health "));

        let failed_state =
            provider_health_state_from_check("provider".to_owned(), Provider::LiteLlm, failed);
        assert_eq!(failed_state.status, ProviderHealthStatus::Unhealthy);
        let healthy_state = provider_health_state_from_check(
            "provider".to_owned(),
            Provider::LiteLlm,
            ActiveHealthCheck {
                ok: true,
                latency_ms: Some(1),
                error_code: None,
                checked_at: Utc::now(),
            },
        );
        assert_eq!(healthy_state.status, ProviderHealthStatus::Healthy);

        let invalid_import: StudioServiceImportRequest =
            serde_json::from_value(serde_json::json!({
                "studio_service_id": "invalid",
                "name": "Invalid Name",
                "upstream_base_url": "ftp://service.example",
                "allowed_methods": []
            }))
            .expect("invalid import shape");
        let issues = service_import_validation_issues(&invalid_import);
        assert!(issues.iter().any(|issue| issue.field == "request"));
        assert!(issues
            .iter()
            .any(|issue| issue.field == "upstream_base_url"));

        assert!(litellm_ui_upstream_url("not-a-url", "ui", None).is_err());
        assert_eq!(
            litellm_ui_upstream_url("https://litellm.example", "/", None)
                .unwrap()
                .path(),
            "/ui/"
        );
        assert_eq!(
            litellm_ui_upstream_url("https://litellm.example", "", None)
                .unwrap()
                .path(),
            "/ui"
        );
        assert_eq!(
            litellm_ui_upstream_url("https://litellm.example", "v2/model/info", Some("x=1"))
                .unwrap()
                .path(),
            "/v2/model/info"
        );

        let raw = LiteLlmUiUpstream {
            base_url: "https://litellm.example".to_owned(),
            credential: "secret".to_owned(),
            credential_header_mode: CredentialHeaderMode::CustomHeader,
            credential_header_name: Some("x-litellm-key".to_owned()),
            credential_header_value_format: CredentialHeaderValueFormat::Raw,
        };
        assert_eq!(litellm_ui_custom_header_credential(&raw), "secret");
        let bearer = LiteLlmUiUpstream {
            credential_header_value_format: CredentialHeaderValueFormat::Bearer,
            ..raw
        };
        assert_eq!(
            litellm_ui_custom_header_credential(&bearer),
            "Bearer secret"
        );

        assert_eq!(
            rewrite_litellm_ui_location(
                &HeaderValue::from_static("/ui/models"),
                "https://litellm.example"
            )
            .as_deref(),
            Some("/admin-ui/litellm-ui/models")
        );
        assert_eq!(
            rewrite_litellm_ui_location(
                &HeaderValue::from_static("https://other.example/ui/models?view=all"),
                "https://litellm.example"
            )
            .as_deref(),
            Some("/admin-ui/litellm-ui/models?view=all")
        );
        let rewritten = rewrite_litellm_ui_json_body(
            br#"["/ui/models",{"nested":"https://litellm.example/ui/keys"},1]"#,
            "https://litellm.example",
        )
        .expect("rewritten JSON");
        let value: serde_json::Value = serde_json::from_slice(&rewritten).expect("JSON body");
        assert_eq!(value[0], "/admin-ui/litellm-ui/models");
        assert_eq!(value[1]["nested"], "/admin-ui/litellm-ui/keys");
        assert_eq!(value[2], 1);
    }

    fn coverage_usage_row(
        request_id: &str,
        status: &str,
        service_name: Option<&str>,
        created_at: chrono::DateTime<Utc>,
    ) -> UsageExportRow {
        UsageExportRow {
            request_id: request_id.to_owned(),
            key_id: Uuid::new_v4(),
            project_id: Some(Uuid::new_v4()),
            route: "/v1/responses".to_owned(),
            model: Some("coverage-model".to_owned()),
            provider: "litellm".to_owned(),
            status: status.to_owned(),
            status_code: if status == "success" { 200 } else { 500 },
            latency_ms: 20,
            input_tokens: 2,
            output_tokens: 3,
            total_tokens: 5,
            estimated_cost_usd: Some(0.01),
            cost_source: Some("upstream_passthrough".to_owned()),
            cost_mode: None,
            pricing_rule_name: None,
            service_name: service_name.map(ToOwned::to_owned),
            http_method: None,
            endpoint_path: None,
            endpoint_template: None,
            task_id: Some("task".to_owned()),
            run_id: Some("run".to_owned()),
            trace_id: Some("trace".to_owned()),
            fallback_count: i32::from(status != "success"),
            guardrail_action_count: 0,
            created_at,
        }
    }

    #[test]
    fn in_memory_usage_helpers_cover_filters_pagination_and_buckets() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-07-22T12:00:00Z")
            .expect("coverage timestamp")
            .with_timezone(&Utc);
        let key = AuthenticatedKey {
            key_id: Uuid::new_v4(),
            project_id: Some(Uuid::new_v4()),
            key_prefix: "rk_live_coverage".to_owned(),
        };
        let mut event = UsageEvent::new(
            "usage-coverage",
            &key,
            Route::Responses,
            Some("coverage-model".to_owned()),
            200,
            20,
            now,
        );
        event.service_name = Some("ocr".to_owned());
        event.task_id = Some("task".to_owned());
        assert!(usage_event_matches_query(&event, &UsageQuery::default()));
        for query in [
            UsageQuery {
                from: Some(now + chrono::Duration::seconds(1)),
                ..UsageQuery::default()
            },
            UsageQuery {
                to: Some(now),
                ..UsageQuery::default()
            },
            UsageQuery {
                project_id: Some(Uuid::new_v4()),
                ..UsageQuery::default()
            },
            UsageQuery {
                key_id: Some(Uuid::new_v4()),
                ..UsageQuery::default()
            },
            UsageQuery {
                route: Some("/v1/embeddings".to_owned()),
                ..UsageQuery::default()
            },
            UsageQuery {
                provider: Some("internal-service".to_owned()),
                ..UsageQuery::default()
            },
            UsageQuery {
                service: Some("translation".to_owned()),
                ..UsageQuery::default()
            },
            UsageQuery {
                task_id: Some("other".to_owned()),
                ..UsageQuery::default()
            },
            UsageQuery {
                model: Some("other".to_owned()),
                ..UsageQuery::default()
            },
            UsageQuery {
                status: Some("failure".to_owned()),
                ..UsageQuery::default()
            },
        ] {
            assert!(!usage_event_matches_query(&event, &query));
        }

        let rows = vec![
            coverage_usage_row("one", "success", Some("ocr"), now),
            coverage_usage_row("two", "failure", None, now + chrono::Duration::hours(2)),
        ];
        let empty = usage_summary_from_rows(&[]);
        assert_eq!(empty.average_latency_ms, None);
        assert_eq!(empty.fallback_rate, 0.0);
        let summary = usage_summary_from_rows(&rows);
        assert_eq!(summary.request_count, 2);
        assert_eq!(summary.success_count, 1);
        assert_eq!(summary.failure_count, 1);
        assert_eq!(summary.average_latency_ms, Some(20.0));

        let unbounded = paginate_usage_rows(rows.clone(), None, None);
        assert_eq!(unbounded.rows.len(), 2);
        assert!(!unbounded.page.has_more);
        let bounded = paginate_usage_rows(rows.clone(), Some(1), Some(-10));
        assert_eq!(bounded.rows.len(), 1);
        assert!(bounded.page.has_more);
        assert_eq!(usage_timeseries_from_rows(&rows, Some("day")).len(), 1);
        assert_eq!(usage_timeseries_from_rows(&rows, Some("hour")).len(), 2);
        let services = usage_service_timeseries_from_rows(&rows, Some("day"));
        assert!(services.iter().any(|point| point.service_name == "ocr"));
        assert!(services.iter().any(|point| point.service_name == "none"));
    }

    #[test]
    fn policy_simulation_helpers_cover_complete_patch_and_warning_contracts() {
        let routes = [
            "/v1/chat/completions",
            "/v1/responses",
            "/v1/embeddings",
            "/v1/messages",
            "/v1/messages/count_tokens",
            "/v1/messages/batches",
            "/v1/messages/batches/*",
            "/v1/messages/batches/*/results",
            "/v1/messages/batches/*/cancel",
            "/v1/models",
            "/providers/openai/*",
            "/summary",
            "/translation",
            "/ocr",
            "/embeddings",
            "/services/*",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
        let patch = gateway_core::admin::KeyPolicyPatch {
            deny: Some(true),
            allowed_routes: Some(routes),
            allowed_models: Some(vec!["model-a".to_owned()]),
            allowed_providers: Some(vec![
                "litellm".to_owned(),
                "openai-compatible".to_owned(),
                "internal-service".to_owned(),
            ]),
            allowed_services: Some(vec!["ocr".to_owned()]),
            rpm_limit: Some(Some(10)),
            tpm_limit: Some(Some(20)),
            daily_budget_usd: Some(Some(1.0)),
            monthly_budget_usd: Some(Some(2.0)),
            allow_streaming: Some(false),
            allow_tools: Some(false),
            max_requests_per_day: Some(Some(30)),
            max_tokens_per_day: Some(Some(40)),
            max_cost_per_request: Some(Some(0.5)),
            max_input_tokens_per_request: Some(Some(50)),
            max_output_tokens_per_request: Some(Some(60)),
            allowed_hours_utc: Some(vec![0, 23]),
            unused_key_auto_disable_after_days: Some(Some(7)),
            max_request_body_bytes: Some(Some(1_024)),
            max_response_body_bytes: Some(Some(2_048)),
            max_stream_duration_seconds: Some(Some(90)),
            max_sse_event_bytes: Some(Some(4_096)),
            max_tool_call_count: Some(Some(3)),
            max_tool_schema_bytes: Some(Some(8_192)),
        };
        let policy = apply_simulation_policy_patch(KeyPolicy::default(), patch)
            .expect("complete policy patch");
        assert!(policy.deny);
        assert_eq!(policy.allowed_routes.len(), 16);
        assert_eq!(policy.allowed_providers.len(), 3);
        assert_eq!(policy.max_tool_schema_bytes, Some(8_192));

        let features = gateway_core::GenerationFeatures {
            model: Some("model-b".to_owned()),
            stream: false,
            tools: false,
            service_name: Some("translation".to_owned()),
        };
        let warnings = policy_simulation_warnings(
            &policy,
            Route::Responses,
            Provider::InternalService,
            &features,
            2,
        );
        assert!(warnings.iter().any(|warning| warning.contains("inherited")));
        assert!(warnings.iter().any(|warning| warning.contains("model-b")));
        assert!(warnings
            .iter()
            .any(|warning| warning.contains("translation")));

        let restrictive = KeyPolicy {
            allowed_routes: vec![Route::ChatCompletions],
            allowed_providers: vec![Provider::LiteLlm],
            ..KeyPolicy::default()
        };
        let restrictive_warnings = policy_simulation_warnings(
            &restrictive,
            Route::Responses,
            Provider::InternalService,
            &gateway_core::GenerationFeatures::default(),
            1,
        );
        assert!(restrictive_warnings
            .iter()
            .any(|warning| warning.contains("route allowlist")));
        assert!(restrictive_warnings
            .iter()
            .any(|warning| warning.contains("provider allowlist")));

        let explicit_deny = policy_simulation_warnings(
            &KeyPolicy {
                deny: true,
                ..KeyPolicy::default()
            },
            Route::Responses,
            Provider::LiteLlm,
            &gateway_core::GenerationFeatures::default(),
            1,
        );
        assert_eq!(explicit_deny.len(), 1);

        for invalid in [
            gateway_core::admin::KeyPolicyPatch {
                allowed_routes: Some(vec!["/invalid".to_owned()]),
                ..Default::default()
            },
            gateway_core::admin::KeyPolicyPatch {
                allowed_providers: Some(vec!["invalid".to_owned()]),
                ..Default::default()
            },
            gateway_core::admin::KeyPolicyPatch {
                allowed_hours_utc: Some(vec![24]),
                ..Default::default()
            },
        ] {
            assert_eq!(
                apply_simulation_policy_patch(KeyPolicy::default(), invalid),
                Err(GatewayError::PolicyDenied)
            );
        }
        assert_eq!(
            parse_simulation_provider("invalid"),
            Err(GatewayError::PolicyDenied)
        );
    }

    #[tokio::test]
    async fn studio_catalog_and_public_router_constructors_use_configured_dependencies() {
        let catalog_body = r#"[{"studio_service_id":"svc-ocr","name":"ocr"}]"#;
        let (catalog_url, captured) = spawn_litellm_server(
            "200 OK",
            vec![("Content-Type", "application/json")],
            catalog_body,
        );
        let catalog = StudioCatalogClient::new(&catalog_url, Some("studio-token".to_owned()));
        let services = catalog.services().await.expect("array-shaped catalog");
        assert_eq!(services.len(), 1);
        assert_eq!(services[0].name, "ocr");
        let request = captured.recv().expect("catalog request");
        assert!(request
            .request_line
            .starts_with("GET /studio/gateway/services "));
        assert!(request
            .headers
            .iter()
            .any(|(name, value)| name.eq_ignore_ascii_case("authorization")
                && value == "Bearer studio-token"));

        let (wrapped_url, wrapped_captured) = spawn_litellm_server(
            "200 OK",
            vec![("Content-Type", "application/json")],
            r#"{"services":[{"studio_service_id":"svc-summary","name":"summary"}]}"#,
        );
        assert_eq!(
            StudioCatalogClient::new(wrapped_url, None)
                .services()
                .await
                .expect("object-shaped catalog")
                .len(),
            1
        );
        wrapped_captured.recv().expect("wrapped catalog request");
        let (failed_url, failed_captured) = spawn_litellm_server(
            "503 Service Unavailable",
            vec![("Content-Type", "application/json")],
            r#"{"error":"unavailable"}"#,
        );
        assert_eq!(
            StudioCatalogClient::new(failed_url, None).services().await,
            Err(GatewayError::StudioUnavailable)
        );
        failed_captured.recv().expect("failed catalog request");

        let (Ok(database_url), Ok(redis_url)) =
            (std::env::var("DATABASE_URL"), std::env::var("REDIS_URL"))
        else {
            return;
        };
        let store = PostgresStore::connect(&database_url)
            .await
            .expect("coverage postgres");
        let redis = RedisReadiness::new(&redis_url).expect("coverage redis");
        let auth_env = GatewayAuthEnv::default();
        let auth_runtime = SharedGatewayAuthRuntime::new(
            EffectiveGatewayAuthSettings::from_sources(None, &auth_env)
                .expect("default auth settings")
                .runtime_config(),
        )
        .expect("default auth runtime");

        drop(router(store.clone(), redis.clone()));
        drop(router_with_studio(
            store.clone(),
            redis.clone(),
            Some(StudioCatalogClient::new(&catalog_url, None)),
        ));
        drop(router_with_studio_and_auth(
            store.clone(),
            redis.clone(),
            None,
            auth_env.clone(),
            auth_runtime.clone(),
        ));
        drop(router_with_studio_auth_and_litellm(
            store,
            redis,
            Some(StudioCatalogClient::new(catalog_url, None)),
            auth_env,
            auth_runtime,
            "http://litellm.local".to_owned(),
            "service-key".to_owned(),
        ));
    }
}
