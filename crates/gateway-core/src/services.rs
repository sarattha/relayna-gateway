use crate::{GatewayError, GatewayResult};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use http::Method;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use uuid::Uuid;

const DEFAULT_TIMEOUT_MS: i64 = 60_000;
const DEFAULT_MAX_BODY_BYTES: i64 = 2_097_152;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceSource {
    Gateway,
    Studio,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceSyncStatus {
    Local,
    Synced,
    Incomplete,
    Stale,
    Failed,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceCostMode {
    Fixed,
    Passthrough,
    #[default]
    None,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ServiceRegistration {
    pub name: String,
    pub project_id: Option<Uuid>,
    pub studio_service_id: Option<String>,
    pub route_pattern: String,
    pub upstream_base_url: Option<String>,
    pub health_check_path: Option<String>,
    pub health_check_method: String,
    pub enabled: bool,
    pub allowed_methods: Vec<String>,
    pub timeout_ms: i64,
    pub max_body_bytes: i64,
    pub cost_mode: ServiceCostMode,
    pub estimated_cost_usd: Option<f64>,
    pub pricing_rules: Vec<ServicePricingRule>,
    pub openapi_source_path: Option<String>,
    pub openapi_schema_hash: Option<String>,
    pub openapi_synced_at: Option<DateTime<Utc>>,
    pub openapi_endpoints: Vec<ServiceOpenApiEndpoint>,
    pub endpoint_pricing_rules: Vec<ServiceEndpointPricingRule>,
    pub credential_secret: Option<String>,
    pub fallback_services: Vec<String>,
    pub source: ServiceSource,
    pub sync_status: ServiceSyncStatus,
    pub last_synced_at: Option<DateTime<Utc>>,
    pub disabled_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct ServiceCreateRequest {
    pub name: String,
    #[serde(default)]
    pub project_id: Option<Uuid>,
    #[serde(default)]
    pub studio_service_id: Option<String>,
    #[serde(default)]
    pub route_pattern: Option<String>,
    #[serde(default)]
    pub upstream_base_url: Option<String>,
    #[serde(default)]
    pub health_check_path: Option<String>,
    #[serde(default = "default_health_check_method")]
    pub health_check_method: String,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default = "default_allowed_methods")]
    pub allowed_methods: Vec<String>,
    #[serde(default)]
    pub credential: Option<String>,
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: i64,
    #[serde(default = "default_max_body_bytes")]
    pub max_body_bytes: i64,
    #[serde(default)]
    pub cost_mode: ServiceCostMode,
    #[serde(default)]
    pub estimated_cost_usd: Option<f64>,
    #[serde(default)]
    pub pricing_rules: Vec<ServicePricingRule>,
    #[serde(default)]
    pub openapi_source_path: Option<String>,
    #[serde(default)]
    pub openapi_endpoints: Vec<ServiceOpenApiEndpoint>,
    #[serde(default)]
    pub endpoint_pricing_rules: Vec<ServiceEndpointPricingRule>,
    #[serde(default)]
    pub fallback_services: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Default, PartialEq)]
pub struct ServicePatchRequest {
    pub project_id: Option<Option<Uuid>>,
    pub studio_service_id: Option<Option<String>>,
    pub route_pattern: Option<String>,
    pub upstream_base_url: Option<Option<String>>,
    pub health_check_path: Option<Option<String>>,
    pub health_check_method: Option<String>,
    pub enabled: Option<bool>,
    pub allowed_methods: Option<Vec<String>>,
    pub credential: Option<Option<String>>,
    pub timeout_ms: Option<i64>,
    pub max_body_bytes: Option<i64>,
    pub cost_mode: Option<ServiceCostMode>,
    pub estimated_cost_usd: Option<Option<f64>>,
    pub pricing_rules: Option<Vec<ServicePricingRule>>,
    pub openapi_source_path: Option<Option<String>>,
    pub openapi_schema_hash: Option<Option<String>>,
    pub openapi_synced_at: Option<Option<DateTime<Utc>>>,
    pub openapi_endpoints: Option<Vec<ServiceOpenApiEndpoint>>,
    pub endpoint_pricing_rules: Option<Vec<ServiceEndpointPricingRule>>,
    pub fallback_services: Option<Vec<String>>,
    pub sync_status: Option<ServiceSyncStatus>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct StudioServiceImportRequest {
    pub studio_service_id: String,
    pub name: String,
    #[serde(default)]
    pub project_id: Option<Uuid>,
    #[serde(default)]
    pub route_pattern: Option<String>,
    #[serde(default)]
    pub upstream_base_url: Option<String>,
    #[serde(default)]
    pub health_check_path: Option<String>,
    #[serde(default = "default_health_check_method")]
    pub health_check_method: String,
    #[serde(default = "default_allowed_methods")]
    pub allowed_methods: Vec<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub default_pricing: Option<StudioServicePricing>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct StudioServicePricing {
    pub cost_mode: ServiceCostMode,
    #[serde(default)]
    pub estimated_cost_usd: Option<f64>,
    #[serde(default)]
    pub pricing_rules: Vec<ServicePricingRule>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct ServicePricingRule {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(alias = "path")]
    pub json_pointer: String,
    #[serde(deserialize_with = "deserialize_pricing_rule_equals")]
    pub equals: String,
    pub cost_mode: ServiceCostMode,
    #[serde(default)]
    pub estimated_cost_usd: Option<f64>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct ServiceOpenApiEndpoint {
    pub method: String,
    pub path_template: String,
    #[serde(default)]
    pub operation_id: Option<String>,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub relayna_default: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct ServiceEndpointPricingRule {
    pub method: String,
    pub path_template: String,
    #[serde(default)]
    pub operation_id: Option<String>,
    pub cost_mode: ServiceCostMode,
    #[serde(default)]
    pub estimated_cost_usd: Option<f64>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct ServiceOpenApiPreviewRequest {
    #[serde(default = "default_openapi_source_path")]
    pub source_path: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct ServiceOpenApiSyncRequest {
    #[serde(default = "default_openapi_source_path")]
    pub source_path: String,
    pub expected_schema_hash: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ServiceOpenApiPreview {
    pub source_path: String,
    pub schema_hash: String,
    pub title: Option<String>,
    pub version: Option<String>,
    pub endpoints: Vec<ServiceOpenApiEndpoint>,
    pub added: Vec<ServiceOpenApiEndpoint>,
    pub removed: Vec<ServiceOpenApiEndpoint>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedServiceCost {
    pub cost_mode: ServiceCostMode,
    pub estimated_cost_usd: Option<f64>,
    pub pricing_rule_name: Option<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct StudioServiceCatalogResponse {
    #[serde(default)]
    pub services: Vec<StudioCatalogService>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct StudioCatalogService {
    #[serde(alias = "service_id")]
    pub studio_service_id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub gateway_service_name: Option<String>,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default, alias = "health_path", alias = "capabilities_path")]
    pub health_check_path: Option<String>,
    #[serde(default)]
    pub health_check_method: Option<String>,
    #[serde(default)]
    pub environment: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub auth_mode: Option<String>,
    #[serde(default, alias = "route_pattern")]
    pub default_route_pattern: Option<String>,
    #[serde(default)]
    pub allowed_methods: Option<Vec<String>>,
    #[serde(default)]
    pub default_pricing: Option<StudioServicePricing>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct StudioServiceImportPreview {
    pub studio_service_id: String,
    pub name: String,
    pub display_name: Option<String>,
    pub environment: Option<String>,
    pub status: Option<String>,
    pub base_url: Option<String>,
    pub tags: Vec<String>,
    pub route_pattern: String,
    pub import_request: StudioServiceImportRequest,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ServiceResponse {
    pub name: String,
    pub project_id: Option<Uuid>,
    pub studio_service_id: Option<String>,
    pub route_pattern: String,
    pub upstream_base_url: Option<String>,
    pub health_check_path: Option<String>,
    pub health_check_method: String,
    pub enabled: bool,
    pub allowed_methods: Vec<String>,
    pub credential_configured: bool,
    pub timeout_ms: i64,
    pub max_body_bytes: i64,
    pub cost_mode: ServiceCostMode,
    pub estimated_cost_usd: Option<f64>,
    pub pricing_rules: Vec<ServicePricingRule>,
    pub openapi_source_path: Option<String>,
    pub openapi_schema_hash: Option<String>,
    pub openapi_synced_at: Option<DateTime<Utc>>,
    pub openapi_endpoints: Vec<ServiceOpenApiEndpoint>,
    pub endpoint_pricing_rules: Vec<ServiceEndpointPricingRule>,
    pub fallback_services: Vec<String>,
    pub source: ServiceSource,
    pub sync_status: ServiceSyncStatus,
    pub last_synced_at: Option<DateTime<Utc>>,
    pub disabled_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub missing_runtime_fields: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ServiceSyncStatusResponse {
    pub name: String,
    pub source: ServiceSource,
    pub sync_status: ServiceSyncStatus,
    pub last_synced_at: Option<DateTime<Utc>>,
    pub missing_runtime_fields: Vec<String>,
}

#[async_trait]
pub trait AdminServiceStore: Send + Sync {
    async fn create_service(&self, request: ServiceCreateRequest)
        -> GatewayResult<ServiceResponse>;
    async fn list_services(&self) -> GatewayResult<Vec<ServiceResponse>>;
    async fn get_service(&self, name: &str) -> GatewayResult<Option<ServiceResponse>>;
    async fn patch_service(
        &self,
        name: &str,
        patch: ServicePatchRequest,
    ) -> GatewayResult<Option<ServiceResponse>>;
    async fn delete_service(&self, name: &str) -> GatewayResult<bool>;
    async fn set_service_enabled(
        &self,
        name: &str,
        enabled: bool,
    ) -> GatewayResult<Option<ServiceResponse>>;
    async fn import_studio_service(
        &self,
        request: StudioServiceImportRequest,
    ) -> GatewayResult<ServiceResponse>;
    async fn sync_studio_service(
        &self,
        request: StudioServiceImportRequest,
    ) -> GatewayResult<ServiceResponse>;
    async fn service_sync_status(
        &self,
        name: &str,
    ) -> GatewayResult<Option<ServiceSyncStatusResponse>>;
}

#[async_trait]
impl<T> AdminServiceStore for std::sync::Arc<T>
where
    T: AdminServiceStore + ?Sized,
{
    async fn create_service(
        &self,
        request: ServiceCreateRequest,
    ) -> GatewayResult<ServiceResponse> {
        (**self).create_service(request).await
    }

    async fn list_services(&self) -> GatewayResult<Vec<ServiceResponse>> {
        (**self).list_services().await
    }

    async fn get_service(&self, name: &str) -> GatewayResult<Option<ServiceResponse>> {
        (**self).get_service(name).await
    }

    async fn patch_service(
        &self,
        name: &str,
        patch: ServicePatchRequest,
    ) -> GatewayResult<Option<ServiceResponse>> {
        (**self).patch_service(name, patch).await
    }

    async fn delete_service(&self, name: &str) -> GatewayResult<bool> {
        (**self).delete_service(name).await
    }

    async fn set_service_enabled(
        &self,
        name: &str,
        enabled: bool,
    ) -> GatewayResult<Option<ServiceResponse>> {
        (**self).set_service_enabled(name, enabled).await
    }

    async fn import_studio_service(
        &self,
        request: StudioServiceImportRequest,
    ) -> GatewayResult<ServiceResponse> {
        (**self).import_studio_service(request).await
    }

    async fn sync_studio_service(
        &self,
        request: StudioServiceImportRequest,
    ) -> GatewayResult<ServiceResponse> {
        (**self).sync_studio_service(request).await
    }

    async fn service_sync_status(
        &self,
        name: &str,
    ) -> GatewayResult<Option<ServiceSyncStatusResponse>> {
        (**self).service_sync_status(name).await
    }
}

#[async_trait]
pub trait ServiceRegistryLookup: Send + Sync {
    async fn service_registration(&self, name: &str) -> GatewayResult<Option<ServiceRegistration>>;
}

#[async_trait]
impl<T> ServiceRegistryLookup for std::sync::Arc<T>
where
    T: ServiceRegistryLookup + ?Sized,
{
    async fn service_registration(&self, name: &str) -> GatewayResult<Option<ServiceRegistration>> {
        (**self).service_registration(name).await
    }
}

#[async_trait]
pub trait ServiceRouteLookup: Send + Sync {
    async fn service_registration_for_route(
        &self,
        method: &Method,
        path: &str,
    ) -> GatewayResult<Option<ServiceRegistration>>;
}

#[async_trait]
impl<T> ServiceRouteLookup for std::sync::Arc<T>
where
    T: ServiceRouteLookup + ?Sized,
{
    async fn service_registration_for_route(
        &self,
        method: &Method,
        path: &str,
    ) -> GatewayResult<Option<ServiceRegistration>> {
        (**self).service_registration_for_route(method, path).await
    }
}

impl ServiceCreateRequest {
    pub fn validate(&self) -> GatewayResult<()> {
        validate_service_name(&self.name)?;
        let route_pattern = self
            .route_pattern
            .clone()
            .or_else(|| default_route_pattern(&self.name))
            .unwrap_or_else(|| format!("/services/{}/*", self.name));
        validate_route_pattern(&route_pattern)?;
        validate_optional_upstream(self.upstream_base_url.as_deref())?;
        validate_optional_health_check_path(self.health_check_path.as_deref())?;
        validate_health_check_method(&self.health_check_method)?;
        validate_allowed_methods(&self.allowed_methods)?;
        validate_runtime_limits(self.timeout_ms, self.max_body_bytes)?;
        validate_cost(self.cost_mode, self.estimated_cost_usd)?;
        validate_pricing_rules(&self.pricing_rules)?;
        validate_optional_openapi_source_path(self.openapi_source_path.as_deref())?;
        validate_openapi_endpoints(&self.openapi_endpoints)?;
        validate_endpoint_pricing_rules(&self.endpoint_pricing_rules)?;
        validate_fallback_services(&self.fallback_services)?;
        validate_optional_secret(self.credential.as_deref())?;
        Ok(())
    }
}

impl ServicePatchRequest {
    pub fn validate(&self) -> GatewayResult<()> {
        if let Some(route_pattern) = self.route_pattern.as_deref() {
            validate_route_pattern(route_pattern)?;
        }
        if let Some(upstream) = &self.upstream_base_url {
            validate_optional_upstream(upstream.as_deref())?;
        }
        if let Some(path) = &self.health_check_path {
            validate_optional_health_check_path(path.as_deref())?;
        }
        if let Some(method) = &self.health_check_method {
            validate_health_check_method(method)?;
        }
        if let Some(methods) = &self.allowed_methods {
            validate_allowed_methods(methods)?;
        }
        if let Some(timeout_ms) = self.timeout_ms {
            validate_runtime_limits(timeout_ms, self.max_body_bytes.unwrap_or(1))?;
        }
        if let Some(max_body_bytes) = self.max_body_bytes {
            validate_runtime_limits(self.timeout_ms.unwrap_or(1), max_body_bytes)?;
        }
        if let Some(Some(cost)) = self.estimated_cost_usd {
            validate_cost(self.cost_mode.unwrap_or_default(), Some(cost))?;
        }
        if let Some(pricing_rules) = &self.pricing_rules {
            validate_pricing_rules(pricing_rules)?;
        }
        if let Some(source_path) = &self.openapi_source_path {
            validate_optional_openapi_source_path(source_path.as_deref())?;
        }
        if let Some(endpoints) = &self.openapi_endpoints {
            validate_openapi_endpoints(endpoints)?;
        }
        if let Some(rules) = &self.endpoint_pricing_rules {
            validate_endpoint_pricing_rules(rules)?;
        }
        if let Some(fallback_services) = &self.fallback_services {
            validate_fallback_services(fallback_services)?;
        }
        if let Some(credential) = &self.credential {
            validate_optional_secret(credential.as_deref())?;
        }
        Ok(())
    }
}

impl StudioServiceImportRequest {
    pub fn validate(&self) -> GatewayResult<()> {
        validate_service_name(&self.name)?;
        if self.studio_service_id.trim().is_empty() {
            return Err(GatewayError::InvalidServicePayload);
        }
        if let Some(route_pattern) = self.route_pattern.as_deref() {
            validate_route_pattern(route_pattern)?;
        }
        validate_optional_upstream(self.upstream_base_url.as_deref())?;
        validate_optional_health_check_path(self.health_check_path.as_deref())?;
        validate_health_check_method(&self.health_check_method)?;
        validate_allowed_methods(&self.allowed_methods)?;
        if let Some(pricing) = &self.default_pricing {
            validate_cost(pricing.cost_mode, pricing.estimated_cost_usd)?;
            validate_pricing_rules(&pricing.pricing_rules)?;
        }
        Ok(())
    }
}

impl StudioCatalogService {
    pub fn into_preview(self) -> GatewayResult<StudioServiceImportPreview> {
        let name = self.gateway_name()?;
        let route_pattern = self
            .default_route_pattern
            .clone()
            .or_else(|| default_route_pattern(&name))
            .unwrap_or_else(|| format!("/services/{name}/*"));
        validate_route_pattern(&route_pattern)?;
        validate_optional_upstream(self.base_url.as_deref())?;
        validate_optional_health_check_path(self.health_check_path.as_deref())?;
        if let Some(method) = &self.health_check_method {
            validate_health_check_method(method)?;
        }
        let allowed_methods = self
            .allowed_methods
            .clone()
            .filter(|methods| !methods.is_empty())
            .unwrap_or_else(default_allowed_methods);
        validate_allowed_methods(&allowed_methods)?;
        if let Some(pricing) = &self.default_pricing {
            validate_cost(pricing.cost_mode, pricing.estimated_cost_usd)?;
            validate_pricing_rules(&pricing.pricing_rules)?;
        }

        let import_request = StudioServiceImportRequest {
            studio_service_id: self.studio_service_id.clone(),
            name: name.clone(),
            project_id: None,
            route_pattern: Some(route_pattern.clone()),
            upstream_base_url: self.base_url.clone(),
            health_check_path: self.health_check_path.clone(),
            health_check_method: self
                .health_check_method
                .clone()
                .unwrap_or_else(default_health_check_method),
            allowed_methods,
            category: self.environment.clone(),
            default_pricing: self.default_pricing.clone(),
        };
        import_request.validate()?;

        Ok(StudioServiceImportPreview {
            studio_service_id: self.studio_service_id,
            name,
            display_name: self.display_name,
            environment: self.environment,
            status: self.status,
            base_url: self.base_url,
            tags: self.tags,
            route_pattern,
            import_request,
        })
    }

    fn gateway_name(&self) -> GatewayResult<String> {
        for candidate in [
            self.gateway_service_name.as_deref(),
            self.name.as_deref(),
            Some(self.studio_service_id.as_str()),
        ]
        .into_iter()
        .flatten()
        {
            let normalized = normalize_gateway_service_name(candidate);
            if validate_service_name(&normalized).is_ok() {
                return Ok(normalized);
            }
        }
        Err(GatewayError::InvalidServicePayload)
    }
}

impl ServiceRegistration {
    pub fn missing_runtime_fields(&self) -> Vec<String> {
        let mut fields = Vec::new();
        if self.upstream_base_url.as_deref().is_none_or(str::is_empty) {
            fields.push("upstream_base_url".to_owned());
        }
        if self.credential_secret.as_deref().is_none_or(str::is_empty) {
            fields.push("credential".to_owned());
        }
        fields
    }

    pub fn ensure_routable(&self) -> GatewayResult<()> {
        if !self.enabled {
            return Err(GatewayError::DisabledService);
        }
        if !self.missing_runtime_fields().is_empty() {
            return Err(GatewayError::IncompleteService);
        }
        validate_optional_upstream(self.upstream_base_url.as_deref())?;
        Ok(())
    }

    pub fn validate_cost(&self) -> GatewayResult<()> {
        validate_cost(self.cost_mode, self.estimated_cost_usd)
    }

    pub fn validate_pricing_rules(&self) -> GatewayResult<()> {
        validate_pricing_rules(&self.pricing_rules)
    }

    pub fn validate_openapi_configuration(&self) -> GatewayResult<()> {
        validate_optional_openapi_source_path(self.openapi_source_path.as_deref())?;
        validate_openapi_endpoints(&self.openapi_endpoints)?;
        validate_endpoint_pricing_rules(&self.endpoint_pricing_rules)
    }

    pub fn to_response(&self) -> ServiceResponse {
        ServiceResponse {
            name: self.name.clone(),
            project_id: self.project_id,
            studio_service_id: self.studio_service_id.clone(),
            route_pattern: self.route_pattern.clone(),
            upstream_base_url: self.upstream_base_url.clone(),
            health_check_path: self.health_check_path.clone(),
            health_check_method: self.health_check_method.clone(),
            enabled: self.enabled,
            allowed_methods: self.allowed_methods.clone(),
            credential_configured: self
                .credential_secret
                .as_deref()
                .is_some_and(|value| !value.is_empty()),
            timeout_ms: self.timeout_ms,
            max_body_bytes: self.max_body_bytes,
            cost_mode: self.cost_mode,
            estimated_cost_usd: self.estimated_cost_usd,
            pricing_rules: self.pricing_rules.clone(),
            openapi_source_path: self.openapi_source_path.clone(),
            openapi_schema_hash: self.openapi_schema_hash.clone(),
            openapi_synced_at: self.openapi_synced_at,
            openapi_endpoints: self.openapi_endpoints.clone(),
            endpoint_pricing_rules: self.endpoint_pricing_rules.clone(),
            fallback_services: self.fallback_services.clone(),
            source: self.source,
            sync_status: self.sync_status,
            last_synced_at: self.last_synced_at,
            disabled_at: self.disabled_at,
            created_at: self.created_at,
            updated_at: self.updated_at,
            missing_runtime_fields: self.missing_runtime_fields(),
        }
    }

    pub fn sync_status_response(&self) -> ServiceSyncStatusResponse {
        ServiceSyncStatusResponse {
            name: self.name.clone(),
            source: self.source,
            sync_status: self.sync_status,
            last_synced_at: self.last_synced_at,
            missing_runtime_fields: self.missing_runtime_fields(),
        }
    }
}

pub fn validate_service_name(name: &str) -> GatewayResult<()> {
    let valid = !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|value| value.is_ascii_lowercase() || value.is_ascii_digit() || value == '-')
        && name
            .chars()
            .next()
            .is_some_and(|value| value.is_ascii_lowercase() || value.is_ascii_digit())
        && name
            .chars()
            .last()
            .is_some_and(|value| value.is_ascii_lowercase() || value.is_ascii_digit());
    if valid {
        Ok(())
    } else {
        Err(GatewayError::InvalidServicePayload)
    }
}

pub fn default_route_pattern(name: &str) -> Option<String> {
    match name {
        "summary" => Some("/summary".to_owned()),
        "translation" => Some("/translation".to_owned()),
        "ocr" => Some("/ocr".to_owned()),
        "embeddings" => Some("/embeddings".to_owned()),
        _ => None,
    }
}

pub fn default_openapi_source_path() -> String {
    "/openapi.json".to_owned()
}

pub fn is_relayna_default_endpoint(path_template: &str) -> bool {
    const EXACT: &[&str] = &["/health", "/history"];
    const PREFIXES: &[&str] = &[
        "/events",
        "/status",
        "/dlq",
        "/broker/dlq",
        "/failed-tasks",
        "/relayna",
        "/executions",
    ];

    EXACT.contains(&path_template)
        || PREFIXES.iter().any(|prefix| {
            path_template == *prefix
                || path_template
                    .strip_prefix(prefix)
                    .is_some_and(|suffix| suffix.starts_with('/'))
        })
}

pub fn endpoint_template_matches(path_template: &str, path: &str) -> bool {
    let template_segments = path_template.trim_matches('/').split('/');
    let path_segments = path.trim_matches('/').split('/');
    let mut template_segments = template_segments.peekable();
    let mut path_segments = path_segments.peekable();

    loop {
        match (template_segments.next(), path_segments.next()) {
            (None, None) => return true,
            (Some(template), Some(actual)) => {
                if !(template.starts_with('{') && template.ends_with('}')) && template != actual {
                    return false;
                }
            }
            _ => return false,
        }
    }
}

pub fn matching_openapi_endpoint<'a>(
    method: &Method,
    path: &str,
    endpoints: &'a [ServiceOpenApiEndpoint],
) -> Option<&'a ServiceOpenApiEndpoint> {
    endpoints
        .iter()
        .filter(|endpoint| endpoint.method.eq_ignore_ascii_case(method.as_str()))
        .filter(|endpoint| endpoint_template_matches(&endpoint.path_template, path))
        .max_by_key(|endpoint| endpoint_template_specificity(&endpoint.path_template))
}

pub fn resolve_endpoint_pricing_rule(
    method: &Method,
    path: &str,
    rules: &[ServiceEndpointPricingRule],
) -> Option<ResolvedServiceCost> {
    rules
        .iter()
        .filter(|rule| rule.method.eq_ignore_ascii_case(method.as_str()))
        .filter(|rule| endpoint_template_matches(&rule.path_template, path))
        .max_by_key(|rule| endpoint_template_specificity(&rule.path_template))
        .map(|rule| ResolvedServiceCost {
            cost_mode: rule.cost_mode,
            estimated_cost_usd: rule.estimated_cost_usd,
            pricing_rule_name: Some(endpoint_pricing_rule_name(rule)),
        })
}

pub fn merge_endpoint_pricing_rules(
    endpoints: &[ServiceOpenApiEndpoint],
    existing: &[ServiceEndpointPricingRule],
    default_cost_mode: ServiceCostMode,
    default_estimated_cost_usd: Option<f64>,
) -> Vec<ServiceEndpointPricingRule> {
    let mut merged = endpoints
        .iter()
        .map(|endpoint| {
            existing
                .iter()
                .find(|rule| {
                    rule.method.eq_ignore_ascii_case(&endpoint.method)
                        && normalized_endpoint_template(&rule.path_template)
                            == normalized_endpoint_template(&endpoint.path_template)
                })
                .cloned()
                .map(|mut rule| {
                    rule.path_template.clone_from(&endpoint.path_template);
                    rule.operation_id.clone_from(&endpoint.operation_id);
                    rule
                })
                .unwrap_or_else(|| ServiceEndpointPricingRule {
                    method: endpoint.method.clone(),
                    path_template: endpoint.path_template.clone(),
                    operation_id: endpoint.operation_id.clone(),
                    cost_mode: if endpoint.relayna_default {
                        ServiceCostMode::None
                    } else {
                        default_cost_mode
                    },
                    estimated_cost_usd: if endpoint.relayna_default {
                        None
                    } else {
                        default_estimated_cost_usd
                    },
                })
        })
        .collect::<Vec<_>>();

    merged.extend(
        existing
            .iter()
            .filter(|rule| {
                !endpoints.iter().any(|endpoint| {
                    endpoint.method.eq_ignore_ascii_case(&rule.method)
                        && normalized_endpoint_template(&endpoint.path_template)
                            == normalized_endpoint_template(&rule.path_template)
                })
            })
            .cloned(),
    );
    merged
}

pub fn service_wildcard_suffix(path: &str, service_name: &str) -> Option<String> {
    let prefix = format!("/services/{service_name}");
    let suffix = path.strip_prefix(&prefix)?;
    if suffix.is_empty() {
        Some("/".to_owned())
    } else if suffix.starts_with('/') {
        Some(suffix.to_owned())
    } else {
        None
    }
}

pub fn route_pattern_wildcard_suffix(path: &str, route_pattern: &str) -> Option<String> {
    let prefix = route_pattern.strip_suffix("/*")?;
    let suffix = path.strip_prefix(prefix)?;
    if suffix.is_empty() {
        Some("/".to_owned())
    } else if suffix.starts_with('?') {
        Some(format!("/{suffix}"))
    } else if suffix.starts_with('/') {
        Some(suffix.to_owned())
    } else {
        None
    }
}

fn normalize_gateway_service_name(value: &str) -> String {
    let mut output = String::new();
    let mut previous_dash = false;
    for character in value.chars().flat_map(char::to_lowercase) {
        if character.is_ascii_lowercase() || character.is_ascii_digit() {
            output.push(character);
            previous_dash = false;
        } else if !previous_dash {
            output.push('-');
            previous_dash = true;
        }
    }
    output.trim_matches('-').chars().take(64).collect()
}

fn validate_route_pattern(route_pattern: &str) -> GatewayResult<()> {
    if route_pattern.starts_with('/') && !route_pattern.contains("//") {
        Ok(())
    } else {
        Err(GatewayError::InvalidServicePayload)
    }
}

fn validate_optional_upstream(upstream: Option<&str>) -> GatewayResult<()> {
    let Some(upstream) = upstream.filter(|value| !value.is_empty()) else {
        return Ok(());
    };
    let url = url::Url::parse(upstream).map_err(|_| GatewayError::InvalidServiceUpstream)?;
    match url.scheme() {
        "http" | "https" if url.host_str().is_some() => Ok(()),
        _ => Err(GatewayError::InvalidServiceUpstream),
    }
}

fn validate_optional_health_check_path(path: Option<&str>) -> GatewayResult<()> {
    let Some(path) = path.filter(|value| !value.trim().is_empty()) else {
        return Ok(());
    };
    if path.starts_with('/') && !path.contains("//") {
        Ok(())
    } else {
        Err(GatewayError::InvalidServicePayload)
    }
}

pub fn validate_openapi_source_path(path: &str) -> GatewayResult<()> {
    if path.len() <= 512
        && path.starts_with('/')
        && !path.starts_with("//")
        && !path.contains("//")
        && !path.contains(['?', '#', '\\'])
        && !path.chars().any(char::is_control)
    {
        Ok(())
    } else {
        Err(GatewayError::InvalidServicePayload)
    }
}

fn validate_optional_openapi_source_path(path: Option<&str>) -> GatewayResult<()> {
    match path {
        Some(path) => validate_openapi_source_path(path),
        None => Ok(()),
    }
}

fn validate_openapi_endpoint_path(path: &str) -> GatewayResult<()> {
    if path.len() > 512 || !path.starts_with('/') || path.contains("//") {
        return Err(GatewayError::InvalidServicePayload);
    }
    for segment in path.trim_matches('/').split('/') {
        let contains_brace = segment.contains(['{', '}']);
        if contains_brace
            && !(segment.starts_with('{')
                && segment.ends_with('}')
                && segment.len() > 2
                && !segment[1..segment.len() - 1].contains(['{', '}']))
        {
            return Err(GatewayError::InvalidServicePayload);
        }
    }
    Ok(())
}

fn validate_endpoint_method(method: &str) -> GatewayResult<()> {
    let parsed =
        Method::from_bytes(method.as_bytes()).map_err(|_| GatewayError::InvalidServicePayload)?;
    if matches!(
        parsed,
        Method::GET
            | Method::POST
            | Method::PUT
            | Method::PATCH
            | Method::DELETE
            | Method::HEAD
            | Method::OPTIONS
    ) {
        Ok(())
    } else {
        Err(GatewayError::InvalidServicePayload)
    }
}

pub fn validate_openapi_endpoints(endpoints: &[ServiceOpenApiEndpoint]) -> GatewayResult<()> {
    if endpoints.len() > 500 {
        return Err(GatewayError::InvalidServicePayload);
    }
    let mut identities = std::collections::BTreeSet::new();
    for endpoint in endpoints {
        validate_endpoint_method(&endpoint.method)?;
        validate_openapi_endpoint_path(&endpoint.path_template)?;
        if endpoint
            .operation_id
            .as_ref()
            .is_some_and(|value| value.len() > 256)
            || endpoint
                .summary
                .as_ref()
                .is_some_and(|value| value.len() > 512)
        {
            return Err(GatewayError::InvalidServicePayload);
        }
        let identity = (
            endpoint.method.to_ascii_uppercase(),
            normalized_endpoint_template(&endpoint.path_template),
        );
        if !identities.insert(identity) {
            return Err(GatewayError::InvalidServicePayload);
        }
    }
    Ok(())
}

fn validate_endpoint_pricing_rules(rules: &[ServiceEndpointPricingRule]) -> GatewayResult<()> {
    if rules.len() > 500 {
        return Err(GatewayError::InvalidServicePayload);
    }
    let mut identities = std::collections::BTreeSet::new();
    for rule in rules {
        validate_endpoint_method(&rule.method)?;
        validate_openapi_endpoint_path(&rule.path_template)?;
        validate_cost(rule.cost_mode, rule.estimated_cost_usd)?;
        if rule
            .operation_id
            .as_ref()
            .is_some_and(|value| value.len() > 256)
        {
            return Err(GatewayError::InvalidServicePayload);
        }
        let identity = (
            rule.method.to_ascii_uppercase(),
            normalized_endpoint_template(&rule.path_template),
        );
        if !identities.insert(identity) {
            return Err(GatewayError::InvalidServicePayload);
        }
    }
    Ok(())
}

fn normalized_endpoint_template(path: &str) -> String {
    path.split('/')
        .map(|segment| {
            if segment.starts_with('{') && segment.ends_with('}') {
                "{}"
            } else {
                segment
            }
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn endpoint_template_specificity(path: &str) -> (usize, usize) {
    let static_segments = path
        .trim_matches('/')
        .split('/')
        .filter(|segment| !(segment.starts_with('{') && segment.ends_with('}')))
        .count();
    (static_segments, path.len())
}

fn endpoint_pricing_rule_name(rule: &ServiceEndpointPricingRule) -> String {
    rule.operation_id.clone().unwrap_or_else(|| {
        format!(
            "{} {}",
            rule.method.to_ascii_uppercase(),
            rule.path_template
        )
    })
}

fn validate_health_check_method(method: &str) -> GatewayResult<()> {
    let parsed =
        Method::from_bytes(method.as_bytes()).map_err(|_| GatewayError::InvalidServicePayload)?;
    if parsed == Method::GET || parsed == Method::HEAD {
        Ok(())
    } else {
        Err(GatewayError::InvalidServicePayload)
    }
}

fn validate_allowed_methods(methods: &[String]) -> GatewayResult<()> {
    if methods.is_empty() {
        return Err(GatewayError::InvalidServicePayload);
    }
    for method in methods {
        let parsed = Method::from_bytes(method.as_bytes())
            .map_err(|_| GatewayError::InvalidServicePayload)?;
        if parsed != Method::GET
            && parsed != Method::POST
            && parsed != Method::PUT
            && parsed != Method::PATCH
            && parsed != Method::DELETE
        {
            return Err(GatewayError::InvalidServicePayload);
        }
    }
    Ok(())
}

fn validate_runtime_limits(timeout_ms: i64, max_body_bytes: i64) -> GatewayResult<()> {
    if (1..=600_000).contains(&timeout_ms) && (1..=104_857_600).contains(&max_body_bytes) {
        Ok(())
    } else {
        Err(GatewayError::InvalidServicePayload)
    }
}

fn validate_cost(cost_mode: ServiceCostMode, estimated_cost_usd: Option<f64>) -> GatewayResult<()> {
    if let Some(cost) = estimated_cost_usd {
        if !cost.is_finite() || cost < 0.0 {
            return Err(GatewayError::InvalidServicePayload);
        }
    }
    if cost_mode == ServiceCostMode::Fixed && estimated_cost_usd.is_none() {
        return Err(GatewayError::InvalidServicePayload);
    }
    Ok(())
}

fn validate_pricing_rules(rules: &[ServicePricingRule]) -> GatewayResult<()> {
    if rules.len() > 50 {
        return Err(GatewayError::InvalidServicePayload);
    }
    for rule in rules {
        if let Some(name) = rule.name.as_deref() {
            validate_service_name(name)?;
        }
        if !rule.json_pointer.starts_with('/') {
            return Err(GatewayError::InvalidServicePayload);
        }
        if rule.equals.is_empty() {
            return Err(GatewayError::InvalidServicePayload);
        }
        validate_cost(rule.cost_mode, rule.estimated_cost_usd)?;
    }
    Ok(())
}

pub fn resolve_service_cost(
    body: &[u8],
    default_cost_mode: ServiceCostMode,
    default_estimated_cost_usd: Option<f64>,
    rules: &[ServicePricingRule],
) -> ResolvedServiceCost {
    let value: Value = match serde_json::from_slice(body) {
        Ok(value) => value,
        Err(_) => return default_service_cost(default_cost_mode, default_estimated_cost_usd),
    };

    resolve_service_cost_from_value(&value, default_cost_mode, default_estimated_cost_usd, rules)
}

pub fn resolve_service_cost_from_value(
    value: &Value,
    default_cost_mode: ServiceCostMode,
    default_estimated_cost_usd: Option<f64>,
    rules: &[ServicePricingRule],
) -> ResolvedServiceCost {
    if let Some(rule) = matching_service_pricing_rule(value, rules) {
        return ResolvedServiceCost {
            cost_mode: rule.cost_mode,
            estimated_cost_usd: rule.estimated_cost_usd,
            pricing_rule_name: rule.name.clone(),
        };
    }

    default_service_cost(default_cost_mode, default_estimated_cost_usd)
}

pub fn matching_service_pricing_rule<'a>(
    value: &Value,
    rules: &'a [ServicePricingRule],
) -> Option<&'a ServicePricingRule> {
    rules.iter().find(|rule| {
        value
            .pointer(&rule.json_pointer)
            .and_then(Value::as_str)
            .is_some_and(|actual| actual == rule.equals)
    })
}

pub fn service_preflight_estimated_cost(
    default_cost_mode: ServiceCostMode,
    default_estimated_cost_usd: Option<f64>,
    rules: &[ServicePricingRule],
) -> Option<f64> {
    let default = (default_cost_mode == ServiceCostMode::Fixed)
        .then_some(default_estimated_cost_usd)
        .flatten();
    rules
        .iter()
        .filter(|rule| rule.cost_mode == ServiceCostMode::Fixed)
        .filter_map(|rule| rule.estimated_cost_usd)
        .fold(default, |highest, cost| {
            Some(highest.map_or(cost, |current| current.max(cost)))
        })
}

fn default_service_cost(
    cost_mode: ServiceCostMode,
    estimated_cost_usd: Option<f64>,
) -> ResolvedServiceCost {
    ResolvedServiceCost {
        cost_mode,
        estimated_cost_usd,
        pricing_rule_name: None,
    }
}

fn deserialize_pricing_rule_equals<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Value::deserialize(deserializer)?;
    match value {
        Value::String(value) => Ok(value),
        Value::Number(value) => Ok(value.to_string()),
        Value::Bool(value) => Ok(value.to_string()),
        _ => Err(serde::de::Error::custom(
            "pricing rule equals must be a string, number, or boolean",
        )),
    }
}

fn validate_fallback_services(services: &[String]) -> GatewayResult<()> {
    for service in services {
        validate_service_name(service)?;
    }
    Ok(())
}

fn validate_optional_secret(secret: Option<&str>) -> GatewayResult<()> {
    match secret {
        Some(value) if value.trim().is_empty() => Err(GatewayError::InvalidServicePayload),
        _ => Ok(()),
    }
}

fn default_enabled() -> bool {
    true
}

fn default_allowed_methods() -> Vec<String> {
    vec!["POST".to_owned()]
}

fn default_health_check_method() -> String {
    "GET".to_owned()
}

fn default_timeout_ms() -> i64 {
    DEFAULT_TIMEOUT_MS
}

fn default_max_body_bytes() -> i64 {
    DEFAULT_MAX_BODY_BYTES
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_service_names() {
        validate_service_name("summary").expect("valid");
        validate_service_name("custom-ai-1").expect("valid");
        assert_eq!(
            validate_service_name("Custom").unwrap_err(),
            GatewayError::InvalidServicePayload
        );
        assert_eq!(
            validate_service_name("-custom").unwrap_err(),
            GatewayError::InvalidServicePayload
        );
    }

    #[test]
    fn redacts_service_credentials_in_response() {
        let now = Utc::now();
        let registration = ServiceRegistration {
            name: "summary".to_owned(),
            project_id: None,
            studio_service_id: None,
            route_pattern: "/summary".to_owned(),
            upstream_base_url: Some("http://summary.internal".to_owned()),
            health_check_path: Some("/health".to_owned()),
            health_check_method: "GET".to_owned(),
            enabled: true,
            allowed_methods: vec!["POST".to_owned()],
            timeout_ms: 60_000,
            max_body_bytes: 1024,
            cost_mode: ServiceCostMode::Fixed,
            estimated_cost_usd: Some(0.01),
            pricing_rules: Vec::new(),
            openapi_source_path: None,
            openapi_schema_hash: None,
            openapi_synced_at: None,
            openapi_endpoints: Vec::new(),
            endpoint_pricing_rules: Vec::new(),
            credential_secret: Some("secret-token".to_owned()),
            fallback_services: Vec::new(),
            source: ServiceSource::Gateway,
            sync_status: ServiceSyncStatus::Local,
            last_synced_at: None,
            disabled_at: None,
            created_at: now,
            updated_at: now,
        };

        let response = registration.to_response();

        assert!(response.credential_configured);
        assert!(response.missing_runtime_fields.is_empty());
    }

    #[test]
    fn create_service_requires_estimate_for_fixed_cost_mode() {
        let mut request = valid_create_request();
        request.cost_mode = ServiceCostMode::Fixed;
        request.estimated_cost_usd = None;

        assert_eq!(
            request.validate().unwrap_err(),
            GatewayError::InvalidServicePayload
        );

        request.estimated_cost_usd = Some(0.0);
        request.validate().expect("fixed cost with estimate");
    }

    #[test]
    fn service_timeout_validation_accepts_documented_boundary() {
        let valid = ServicePatchRequest {
            timeout_ms: Some(600_000),
            ..ServicePatchRequest::default()
        };
        valid.validate().expect("maximum timeout");

        for timeout_ms in [0, 600_001] {
            let invalid = ServicePatchRequest {
                timeout_ms: Some(timeout_ms),
                ..ServicePatchRequest::default()
            };
            assert_eq!(
                invalid.validate().unwrap_err(),
                GatewayError::InvalidServicePayload
            );
        }
    }

    #[test]
    fn patch_cost_validation_uses_effective_service_state() {
        let mut registration = service_registration(ServiceCostMode::None, Some(0.02));
        let patch = ServicePatchRequest {
            cost_mode: Some(ServiceCostMode::Fixed),
            ..ServicePatchRequest::default()
        };

        patch.validate().expect("partial patch is valid");
        if let Some(cost_mode) = patch.cost_mode {
            registration.cost_mode = cost_mode;
        }
        registration
            .validate_cost()
            .expect("existing estimate satisfies fixed cost mode");

        registration.estimated_cost_usd = None;
        assert_eq!(
            registration.validate_cost().unwrap_err(),
            GatewayError::InvalidServicePayload
        );
    }

    #[test]
    fn patch_clearing_estimate_is_rejected_for_effective_fixed_cost_mode() {
        let mut registration = service_registration(ServiceCostMode::Fixed, Some(0.02));
        let patch = ServicePatchRequest {
            estimated_cost_usd: Some(None),
            ..ServicePatchRequest::default()
        };

        patch.validate().expect("clearing estimate is patch-shaped");
        if let Some(estimated_cost_usd) = patch.estimated_cost_usd {
            registration.estimated_cost_usd = estimated_cost_usd;
        }

        assert_eq!(
            registration.validate_cost().unwrap_err(),
            GatewayError::InvalidServicePayload
        );
    }

    #[test]
    fn passthrough_and_none_cost_modes_do_not_require_estimate() {
        service_registration(ServiceCostMode::None, None)
            .validate_cost()
            .expect("none mode");
        service_registration(ServiceCostMode::Passthrough, None)
            .validate_cost()
            .expect("passthrough mode");
    }

    #[test]
    fn maps_wildcard_path_to_upstream_suffix() {
        assert_eq!(
            service_wildcard_suffix("/services/custom-ai/run?x=1", "custom-ai").as_deref(),
            Some("/run?x=1")
        );
        assert_eq!(
            service_wildcard_suffix("/services/custom-ai", "custom-ai").as_deref(),
            Some("/")
        );
    }

    #[test]
    fn maps_persisted_wildcard_route_to_upstream_suffix() {
        assert_eq!(
            route_pattern_wildcard_suffix(
                "/services/translation/translations?trace=1",
                "/services/translation/*",
            )
            .as_deref(),
            Some("/translations?trace=1")
        );
        assert_eq!(
            route_pattern_wildcard_suffix("/services/translation", "/services/translation/*")
                .as_deref(),
            Some("/")
        );
        assert_eq!(
            route_pattern_wildcard_suffix(
                "/services/translation?trace=1",
                "/services/translation/*",
            )
            .as_deref(),
            Some("/?trace=1")
        );
        assert_eq!(
            route_pattern_wildcard_suffix("/translations", "/translations").as_deref(),
            None
        );
    }

    #[test]
    fn maps_studio_catalog_service_to_import_preview_without_secrets() {
        let preview = StudioCatalogService {
            studio_service_id: "Payments API".to_owned(),
            name: Some("Payments API".to_owned()),
            gateway_service_name: None,
            display_name: Some("Payments API".to_owned()),
            base_url: Some("https://payments.example.test".to_owned()),
            health_check_path: Some("/relayna/capabilities".to_owned()),
            health_check_method: Some("HEAD".to_owned()),
            environment: Some("prod".to_owned()),
            tags: vec!["core".to_owned()],
            status: Some("healthy".to_owned()),
            auth_mode: Some("internal_network".to_owned()),
            default_route_pattern: None,
            allowed_methods: Some(vec!["GET".to_owned(), "POST".to_owned()]),
            default_pricing: None,
        }
        .into_preview()
        .expect("preview");

        assert_eq!(preview.name, "payments-api");
        assert_eq!(preview.route_pattern, "/services/payments-api/*");
        assert_eq!(
            preview.import_request.upstream_base_url.as_deref(),
            Some("https://payments.example.test")
        );
        assert_eq!(
            preview.import_request.health_check_path.as_deref(),
            Some("/relayna/capabilities")
        );
        assert_eq!(preview.import_request.health_check_method, "HEAD");
        assert_eq!(preview.import_request.allowed_methods, ["GET", "POST"]);
    }

    #[test]
    fn studio_import_allows_incomplete_runtime_fields() {
        let request = StudioServiceImportRequest {
            studio_service_id: "svc_1".to_owned(),
            name: "translation".to_owned(),
            project_id: None,
            route_pattern: Some("/translation".to_owned()),
            upstream_base_url: None,
            health_check_path: None,
            health_check_method: "GET".to_owned(),
            allowed_methods: vec!["POST".to_owned()],
            category: None,
            default_pricing: Some(StudioServicePricing {
                cost_mode: ServiceCostMode::Fixed,
                estimated_cost_usd: Some(0.02),
                pricing_rules: Vec::new(),
            }),
        };

        request.validate().expect("valid studio import");
    }

    #[test]
    fn resolves_first_matching_service_pricing_rule() {
        let rules = vec![
            ServicePricingRule {
                name: Some("ocr-doc-int".to_owned()),
                json_pointer: "/model".to_owned(),
                equals: "doct-int".to_owned(),
                cost_mode: ServiceCostMode::Fixed,
                estimated_cost_usd: Some(0.08),
            },
            ServicePricingRule {
                name: Some("ocr-internal".to_owned()),
                json_pointer: "/model".to_owned(),
                equals: "internal".to_owned(),
                cost_mode: ServiceCostMode::Fixed,
                estimated_cost_usd: Some(0.02),
            },
        ];

        let resolved = resolve_service_cost(
            br#"{"model":"doct-int"}"#,
            ServiceCostMode::Fixed,
            Some(0.01),
            &rules,
        );

        assert_eq!(resolved.cost_mode, ServiceCostMode::Fixed);
        assert_eq!(resolved.estimated_cost_usd, Some(0.08));
        assert_eq!(resolved.pricing_rule_name.as_deref(), Some("ocr-doc-int"));
    }

    #[test]
    fn falls_back_to_service_default_when_pricing_rule_does_not_match() {
        let rules = vec![ServicePricingRule {
            name: Some("ocr-doc-int".to_owned()),
            json_pointer: "/model".to_owned(),
            equals: "doct-int".to_owned(),
            cost_mode: ServiceCostMode::Fixed,
            estimated_cost_usd: Some(0.08),
        }];

        let resolved = resolve_service_cost(
            br#"{"model":"unknown"}"#,
            ServiceCostMode::Fixed,
            Some(0.01),
            &rules,
        );

        assert_eq!(resolved.cost_mode, ServiceCostMode::Fixed);
        assert_eq!(resolved.estimated_cost_usd, Some(0.01));
        assert_eq!(resolved.pricing_rule_name, None);
    }

    #[test]
    fn resolves_service_pricing_rule_from_preparsed_selector_value() {
        let rules = vec![ServicePricingRule {
            name: Some("docint".to_owned()),
            json_pointer: "/engine".to_owned(),
            equals: "docint".to_owned(),
            cost_mode: ServiceCostMode::Fixed,
            estimated_cost_usd: Some(0.5),
        }];

        let resolved = resolve_service_cost_from_value(
            &serde_json::json!({"engine": "docint"}),
            ServiceCostMode::Fixed,
            Some(0.01),
            &rules,
        );

        assert_eq!(resolved.cost_mode, ServiceCostMode::Fixed);
        assert_eq!(resolved.estimated_cost_usd, Some(0.5));
        assert_eq!(resolved.pricing_rule_name.as_deref(), Some("docint"));
    }

    #[test]
    fn service_preflight_uses_highest_configured_fixed_cost() {
        let rules = vec![
            ServicePricingRule {
                name: Some("docint".to_owned()),
                json_pointer: "/engine".to_owned(),
                equals: "docint".to_owned(),
                cost_mode: ServiceCostMode::Fixed,
                estimated_cost_usd: Some(0.5),
            },
            ServicePricingRule {
                name: Some("provider-cost".to_owned()),
                json_pointer: "/engine".to_owned(),
                equals: "provider".to_owned(),
                cost_mode: ServiceCostMode::Passthrough,
                estimated_cost_usd: Some(2.0),
            },
        ];

        assert_eq!(
            service_preflight_estimated_cost(ServiceCostMode::Fixed, Some(0.01), &rules,),
            Some(0.5)
        );
        assert_eq!(
            service_preflight_estimated_cost(ServiceCostMode::Passthrough, None, &rules[1..],),
            None
        );
    }

    #[test]
    fn validates_service_pricing_rules() {
        let mut request = valid_create_request();
        request.pricing_rules = vec![ServicePricingRule {
            name: Some("ocr-doc-int".to_owned()),
            json_pointer: "/model".to_owned(),
            equals: "doct-int".to_owned(),
            cost_mode: ServiceCostMode::Fixed,
            estimated_cost_usd: Some(0.08),
        }];
        request.validate().expect("valid pricing rule");

        request.pricing_rules[0].json_pointer = "model".to_owned();
        assert_eq!(
            request.validate().unwrap_err(),
            GatewayError::InvalidServicePayload
        );
    }

    #[test]
    fn deserializes_legacy_service_pricing_rule_shape() {
        let rules: Vec<ServicePricingRule> = serde_json::from_value(serde_json::json!([
            {
                "name": "long-doc",
                "path": "/payload/page_count",
                "equals": 25,
                "cost_mode": "fixed",
                "estimated_cost_usd": 0.072
            },
            {
                "name": "legal-es",
                "path": "/target_locale",
                "equals": "es-MX",
                "cost_mode": "fixed",
                "estimated_cost_usd": 0.046
            }
        ]))
        .expect("legacy pricing rules deserialize");

        assert_eq!(rules[0].json_pointer, "/payload/page_count");
        assert_eq!(rules[0].equals, "25");
        assert_eq!(rules[1].json_pointer, "/target_locale");
        assert_eq!(rules[1].equals, "es-MX");
    }

    #[test]
    fn classifies_relayna_default_endpoints_without_classifying_ocr() {
        for path in [
            "/events/feed",
            "/status/{task_id}",
            "/history",
            "/dlq/messages",
            "/broker/dlq/queues",
            "/failed-tasks/{failure_id}/retry",
            "/relayna/runtime/backpressure",
            "/executions/{task_id}/graph",
            "/health",
        ] {
            assert!(is_relayna_default_endpoint(path), "{path}");
        }
        assert!(!is_relayna_default_endpoint("/ocr"));
    }

    #[test]
    fn endpoint_template_matching_is_segment_aware() {
        assert!(endpoint_template_matches(
            "/events/{task_id}",
            "/events/task-123"
        ));
        assert!(!endpoint_template_matches(
            "/events/{task_id}",
            "/events/task-123/more"
        ));
        assert!(!endpoint_template_matches(
            "/events/{task_id}",
            "/event/task-123"
        ));
    }

    #[test]
    fn openapi_endpoint_matching_respects_method_and_specificity() {
        let endpoints = vec![
            ServiceOpenApiEndpoint {
                method: "POST".to_owned(),
                path_template: "/jobs/{job_id}".to_owned(),
                operation_id: Some("update_job".to_owned()),
                summary: None,
                relayna_default: false,
            },
            ServiceOpenApiEndpoint {
                method: "POST".to_owned(),
                path_template: "/jobs/special".to_owned(),
                operation_id: Some("update_special_job".to_owned()),
                summary: None,
                relayna_default: false,
            },
            ServiceOpenApiEndpoint {
                method: "GET".to_owned(),
                path_template: "/jobs/{job_id}".to_owned(),
                operation_id: Some("get_job".to_owned()),
                summary: None,
                relayna_default: false,
            },
        ];

        let matched = matching_openapi_endpoint(&Method::POST, "/jobs/special", &endpoints)
            .expect("specific endpoint");
        assert_eq!(matched.operation_id.as_deref(), Some("update_special_job"));

        let matched = matching_openapi_endpoint(&Method::GET, "/jobs/123", &endpoints)
            .expect("method-specific endpoint");
        assert_eq!(matched.operation_id.as_deref(), Some("get_job"));
        assert!(matching_openapi_endpoint(&Method::DELETE, "/jobs/123", &endpoints).is_none());
    }

    #[test]
    fn endpoint_pricing_prefers_the_most_specific_method_path() {
        let rules = vec![
            ServiceEndpointPricingRule {
                method: "GET".to_owned(),
                path_template: "/events/{task_id}".to_owned(),
                operation_id: Some("task-events".to_owned()),
                cost_mode: ServiceCostMode::None,
                estimated_cost_usd: None,
            },
            ServiceEndpointPricingRule {
                method: "GET".to_owned(),
                path_template: "/events/feed".to_owned(),
                operation_id: Some("feed".to_owned()),
                cost_mode: ServiceCostMode::Fixed,
                estimated_cost_usd: Some(0.02),
            },
        ];

        let resolved = resolve_endpoint_pricing_rule(&Method::GET, "/events/feed", &rules)
            .expect("endpoint pricing");
        assert_eq!(resolved.cost_mode, ServiceCostMode::Fixed);
        assert_eq!(resolved.estimated_cost_usd, Some(0.02));
        assert_eq!(resolved.pricing_rule_name.as_deref(), Some("feed"));
        assert!(resolve_endpoint_pricing_rule(&Method::POST, "/events/feed", &rules).is_none());
    }

    #[test]
    fn merges_relayna_defaults_as_none_and_preserves_admin_prices() {
        let endpoints = vec![
            ServiceOpenApiEndpoint {
                method: "POST".to_owned(),
                path_template: "/ocr".to_owned(),
                operation_id: Some("submit_ocr".to_owned()),
                summary: None,
                relayna_default: false,
            },
            ServiceOpenApiEndpoint {
                method: "GET".to_owned(),
                path_template: "/events/feed".to_owned(),
                operation_id: Some("feed".to_owned()),
                summary: None,
                relayna_default: true,
            },
        ];
        let existing = vec![ServiceEndpointPricingRule {
            method: "GET".to_owned(),
            path_template: "/events/feed".to_owned(),
            operation_id: Some("old-feed".to_owned()),
            cost_mode: ServiceCostMode::Fixed,
            estimated_cost_usd: Some(0.03),
        }];

        let merged =
            merge_endpoint_pricing_rules(&endpoints, &existing, ServiceCostMode::Fixed, Some(0.01));
        assert_eq!(merged[0].cost_mode, ServiceCostMode::Fixed);
        assert_eq!(merged[0].estimated_cost_usd, Some(0.01));
        assert_eq!(merged[1].cost_mode, ServiceCostMode::Fixed);
        assert_eq!(merged[1].estimated_cost_usd, Some(0.03));
        assert_eq!(merged[1].operation_id.as_deref(), Some("feed"));

        let defaults =
            merge_endpoint_pricing_rules(&endpoints, &[], ServiceCostMode::Fixed, Some(0.01));
        assert_eq!(defaults[1].cost_mode, ServiceCostMode::None);
        assert_eq!(defaults[1].estimated_cost_usd, None);
    }

    #[test]
    fn validates_openapi_paths_and_rejects_ambiguous_templates() {
        assert!(validate_openapi_source_path("/openapi.json").is_ok());
        assert!(validate_openapi_source_path("https://evil.example/openapi.json").is_err());
        assert!(validate_openapi_source_path("//evil.example/openapi.json").is_err());

        let mut request = valid_create_request();
        request.openapi_source_path = Some("/openapi.json".to_owned());
        request.openapi_endpoints = vec![
            ServiceOpenApiEndpoint {
                method: "GET".to_owned(),
                path_template: "/events/{task_id}".to_owned(),
                operation_id: None,
                summary: None,
                relayna_default: true,
            },
            ServiceOpenApiEndpoint {
                method: "GET".to_owned(),
                path_template: "/events/{other_id}".to_owned(),
                operation_id: None,
                summary: None,
                relayna_default: true,
            },
        ];
        assert_eq!(
            request.validate().unwrap_err(),
            GatewayError::InvalidServicePayload
        );
    }

    fn valid_create_request() -> ServiceCreateRequest {
        ServiceCreateRequest {
            name: "summary".to_owned(),
            project_id: None,
            studio_service_id: None,
            route_pattern: Some("/summary".to_owned()),
            upstream_base_url: Some("http://summary.internal".to_owned()),
            health_check_path: Some("/health".to_owned()),
            health_check_method: "GET".to_owned(),
            enabled: true,
            allowed_methods: vec!["POST".to_owned()],
            credential: Some("secret-token".to_owned()),
            timeout_ms: 60_000,
            max_body_bytes: 1024,
            cost_mode: ServiceCostMode::None,
            estimated_cost_usd: None,
            pricing_rules: Vec::new(),
            openapi_source_path: None,
            openapi_endpoints: Vec::new(),
            endpoint_pricing_rules: Vec::new(),
            fallback_services: Vec::new(),
        }
    }

    fn service_registration(
        cost_mode: ServiceCostMode,
        estimated_cost_usd: Option<f64>,
    ) -> ServiceRegistration {
        let now = Utc::now();
        ServiceRegistration {
            name: "summary".to_owned(),
            project_id: None,
            studio_service_id: None,
            route_pattern: "/summary".to_owned(),
            upstream_base_url: Some("http://summary.internal".to_owned()),
            health_check_path: Some("/health".to_owned()),
            health_check_method: "GET".to_owned(),
            enabled: true,
            allowed_methods: vec!["POST".to_owned()],
            timeout_ms: 60_000,
            max_body_bytes: 1024,
            cost_mode,
            estimated_cost_usd,
            pricing_rules: Vec::new(),
            openapi_source_path: None,
            openapi_schema_hash: None,
            openapi_synced_at: None,
            openapi_endpoints: Vec::new(),
            endpoint_pricing_rules: Vec::new(),
            credential_secret: Some("secret-token".to_owned()),
            fallback_services: Vec::new(),
            source: ServiceSource::Gateway,
            sync_status: ServiceSyncStatus::Local,
            last_synced_at: None,
            disabled_at: None,
            created_at: now,
            updated_at: now,
        }
    }
}
