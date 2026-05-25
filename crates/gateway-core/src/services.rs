use crate::{GatewayError, GatewayResult};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use http::Method;
use serde::{Deserialize, Serialize};
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
        if let Some(cost_mode) = self.cost_mode {
            validate_cost(cost_mode, self.estimated_cost_usd.flatten())?;
        }
        if let Some(cost) = self.estimated_cost_usd {
            validate_cost(self.cost_mode.unwrap_or_default(), cost)?;
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
            }),
        };

        request.validate().expect("valid studio import");
    }
}
