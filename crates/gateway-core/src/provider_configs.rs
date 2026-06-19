use crate::{GatewayError, GatewayResult, Provider};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use http::header::HeaderName;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProviderConfigKind {
    #[serde(rename = "litellm")]
    LiteLlm,
    #[serde(rename = "internal-service")]
    InternalService,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialHeaderMode {
    #[default]
    AuthorizationBearer,
    CustomHeader,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialHeaderValueFormat {
    #[default]
    Raw,
    Bearer,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct ProviderConfigCreateRequest {
    pub provider: ProviderConfigKind,
    pub name: String,
    pub base_url: String,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub credential: Option<String>,
    #[serde(default)]
    pub credential_header_mode: CredentialHeaderMode,
    #[serde(default)]
    pub credential_header_name: Option<String>,
    #[serde(default)]
    pub credential_header_value_format: CredentialHeaderValueFormat,
}

#[derive(Debug, Clone, Deserialize, Default, PartialEq)]
pub struct ProviderConfigPatchRequest {
    pub name: Option<String>,
    pub base_url: Option<String>,
    pub enabled: Option<bool>,
    pub credential: Option<Option<String>>,
    pub credential_header_mode: Option<CredentialHeaderMode>,
    pub credential_header_name: Option<Option<String>>,
    pub credential_header_value_format: Option<CredentialHeaderValueFormat>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ProviderConfigResponse {
    pub id: Uuid,
    pub provider: ProviderConfigKind,
    pub name: String,
    pub base_url: String,
    pub enabled: bool,
    pub credential_configured: bool,
    pub credential_header_mode: CredentialHeaderMode,
    pub credential_header_name: Option<String>,
    pub credential_header_value_format: CredentialHeaderValueFormat,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderRuntimeConfig {
    pub provider: Provider,
    pub base_url: String,
    pub credential: Option<String>,
    pub credential_header_mode: CredentialHeaderMode,
    pub credential_header_name: Option<String>,
    pub credential_header_value_format: CredentialHeaderValueFormat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LiteLlmCredentialMappingScope {
    Key,
    Project,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct LiteLlmCredentialMappingUpsertRequest {
    pub scope: LiteLlmCredentialMappingScope,
    pub target_id: Uuid,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub credential: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct LiteLlmCredentialMappingResponse {
    pub id: Uuid,
    pub scope: LiteLlmCredentialMappingScope,
    pub target_id: Uuid,
    pub target_label: Option<String>,
    pub enabled: bool,
    pub credential_configured: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiteLlmCredentialMappingRuntime {
    pub credential: String,
}

#[async_trait]
pub trait AdminProviderConfigStore: Send + Sync {
    async fn create_provider_config(
        &self,
        request: ProviderConfigCreateRequest,
    ) -> GatewayResult<ProviderConfigResponse>;
    async fn list_provider_configs(&self) -> GatewayResult<Vec<ProviderConfigResponse>>;
    async fn get_provider_config(
        &self,
        provider_id: Uuid,
    ) -> GatewayResult<Option<ProviderConfigResponse>>;
    async fn patch_provider_config(
        &self,
        provider_id: Uuid,
        patch: ProviderConfigPatchRequest,
    ) -> GatewayResult<Option<ProviderConfigResponse>>;
    async fn delete_provider_config(&self, provider_id: Uuid) -> GatewayResult<bool>;
    async fn set_provider_config_enabled(
        &self,
        provider_id: Uuid,
        enabled: bool,
    ) -> GatewayResult<Option<ProviderConfigResponse>>;
    async fn upsert_litellm_credential_mapping(
        &self,
        request: LiteLlmCredentialMappingUpsertRequest,
    ) -> GatewayResult<LiteLlmCredentialMappingResponse>;
    async fn list_litellm_credential_mappings(
        &self,
    ) -> GatewayResult<Vec<LiteLlmCredentialMappingResponse>>;
    async fn delete_litellm_credential_mapping(&self, mapping_id: Uuid) -> GatewayResult<bool>;
    async fn set_litellm_credential_mapping_enabled(
        &self,
        mapping_id: Uuid,
        enabled: bool,
    ) -> GatewayResult<Option<LiteLlmCredentialMappingResponse>>;
}

#[async_trait]
impl<T> AdminProviderConfigStore for std::sync::Arc<T>
where
    T: AdminProviderConfigStore + ?Sized,
{
    async fn create_provider_config(
        &self,
        request: ProviderConfigCreateRequest,
    ) -> GatewayResult<ProviderConfigResponse> {
        (**self).create_provider_config(request).await
    }

    async fn list_provider_configs(&self) -> GatewayResult<Vec<ProviderConfigResponse>> {
        (**self).list_provider_configs().await
    }

    async fn get_provider_config(
        &self,
        provider_id: Uuid,
    ) -> GatewayResult<Option<ProviderConfigResponse>> {
        (**self).get_provider_config(provider_id).await
    }

    async fn patch_provider_config(
        &self,
        provider_id: Uuid,
        patch: ProviderConfigPatchRequest,
    ) -> GatewayResult<Option<ProviderConfigResponse>> {
        (**self).patch_provider_config(provider_id, patch).await
    }

    async fn delete_provider_config(&self, provider_id: Uuid) -> GatewayResult<bool> {
        (**self).delete_provider_config(provider_id).await
    }

    async fn set_provider_config_enabled(
        &self,
        provider_id: Uuid,
        enabled: bool,
    ) -> GatewayResult<Option<ProviderConfigResponse>> {
        (**self)
            .set_provider_config_enabled(provider_id, enabled)
            .await
    }

    async fn upsert_litellm_credential_mapping(
        &self,
        request: LiteLlmCredentialMappingUpsertRequest,
    ) -> GatewayResult<LiteLlmCredentialMappingResponse> {
        (**self).upsert_litellm_credential_mapping(request).await
    }

    async fn list_litellm_credential_mappings(
        &self,
    ) -> GatewayResult<Vec<LiteLlmCredentialMappingResponse>> {
        (**self).list_litellm_credential_mappings().await
    }

    async fn delete_litellm_credential_mapping(&self, mapping_id: Uuid) -> GatewayResult<bool> {
        (**self).delete_litellm_credential_mapping(mapping_id).await
    }

    async fn set_litellm_credential_mapping_enabled(
        &self,
        mapping_id: Uuid,
        enabled: bool,
    ) -> GatewayResult<Option<LiteLlmCredentialMappingResponse>> {
        (**self)
            .set_litellm_credential_mapping_enabled(mapping_id, enabled)
            .await
    }
}

#[async_trait]
pub trait ProviderConfigLookup: Send + Sync {
    async fn active_litellm_config(&self) -> GatewayResult<Option<ProviderRuntimeConfig>>;
    async fn litellm_credential_mapping_for_context(
        &self,
        key_id: Uuid,
        project_id: Option<Uuid>,
    ) -> GatewayResult<Option<LiteLlmCredentialMappingRuntime>>;
}

#[async_trait]
impl<T> ProviderConfigLookup for std::sync::Arc<T>
where
    T: ProviderConfigLookup + ?Sized,
{
    async fn active_litellm_config(&self) -> GatewayResult<Option<ProviderRuntimeConfig>> {
        (**self).active_litellm_config().await
    }

    async fn litellm_credential_mapping_for_context(
        &self,
        key_id: Uuid,
        project_id: Option<Uuid>,
    ) -> GatewayResult<Option<LiteLlmCredentialMappingRuntime>> {
        (**self)
            .litellm_credential_mapping_for_context(key_id, project_id)
            .await
    }
}

impl ProviderConfigCreateRequest {
    pub fn validate(&self) -> GatewayResult<()> {
        validate_name(&self.name)?;
        validate_base_url(&self.base_url)?;
        validate_optional_secret(self.credential.as_deref())?;
        validate_header_settings(
            self.credential_header_mode,
            self.credential_header_name.as_deref(),
        )?;
        Ok(())
    }
}

impl ProviderConfigPatchRequest {
    pub fn validate(&self) -> GatewayResult<()> {
        if let Some(name) = self.name.as_deref() {
            validate_name(name)?;
        }
        if let Some(base_url) = self.base_url.as_deref() {
            validate_base_url(base_url)?;
        }
        if let Some(credential) = &self.credential {
            validate_optional_secret(credential.as_deref())?;
        }
        if let Some(Some(name)) = &self.credential_header_name {
            validate_litellm_credential_header_name(name)?;
        }
        if self.credential_header_mode == Some(CredentialHeaderMode::CustomHeader)
            && self.credential_header_name == Some(None)
        {
            return Err(GatewayError::InvalidProviderConfigPayload);
        }
        Ok(())
    }
}

impl ProviderConfigResponse {
    pub fn validate_header_settings(&self) -> GatewayResult<()> {
        validate_header_settings(
            self.credential_header_mode,
            self.credential_header_name.as_deref(),
        )
    }
}

pub fn provider_config_kind_str(provider: ProviderConfigKind) -> &'static str {
    match provider {
        ProviderConfigKind::LiteLlm => "litellm",
        ProviderConfigKind::InternalService => "internal-service",
    }
}

pub fn parse_provider_config_kind(value: &str) -> GatewayResult<ProviderConfigKind> {
    match value {
        "litellm" => Ok(ProviderConfigKind::LiteLlm),
        "internal-service" => Ok(ProviderConfigKind::InternalService),
        _ => Err(GatewayError::InvalidProviderConfigPayload),
    }
}

fn validate_name(name: &str) -> GatewayResult<()> {
    let trimmed = name.trim();
    if trimmed.is_empty() || trimmed.len() > 120 {
        return Err(GatewayError::InvalidProviderConfigPayload);
    }
    Ok(())
}

fn validate_base_url(base_url: &str) -> GatewayResult<()> {
    let url = url::Url::parse(base_url).map_err(|_| GatewayError::InvalidProviderConfigPayload)?;
    match url.scheme() {
        "http" | "https" if url.host_str().is_some() => Ok(()),
        _ => Err(GatewayError::InvalidProviderConfigPayload),
    }
}

fn validate_optional_secret(secret: Option<&str>) -> GatewayResult<()> {
    match secret {
        Some(value) if value.trim().is_empty() => Err(GatewayError::InvalidProviderConfigPayload),
        _ => Ok(()),
    }
}

pub fn credential_header_mode_str(mode: CredentialHeaderMode) -> &'static str {
    match mode {
        CredentialHeaderMode::AuthorizationBearer => "authorization_bearer",
        CredentialHeaderMode::CustomHeader => "custom_header",
    }
}

pub fn parse_credential_header_mode(value: &str) -> GatewayResult<CredentialHeaderMode> {
    match value {
        "authorization_bearer" => Ok(CredentialHeaderMode::AuthorizationBearer),
        "custom_header" => Ok(CredentialHeaderMode::CustomHeader),
        _ => Err(GatewayError::InvalidProviderConfigPayload),
    }
}

pub fn credential_header_value_format_str(format: CredentialHeaderValueFormat) -> &'static str {
    match format {
        CredentialHeaderValueFormat::Raw => "raw",
        CredentialHeaderValueFormat::Bearer => "bearer",
    }
}

pub fn parse_credential_header_value_format(
    value: &str,
) -> GatewayResult<CredentialHeaderValueFormat> {
    match value {
        "raw" => Ok(CredentialHeaderValueFormat::Raw),
        "bearer" => Ok(CredentialHeaderValueFormat::Bearer),
        _ => Err(GatewayError::InvalidProviderConfigPayload),
    }
}

pub fn credential_mapping_scope_str(scope: LiteLlmCredentialMappingScope) -> &'static str {
    match scope {
        LiteLlmCredentialMappingScope::Key => "key",
        LiteLlmCredentialMappingScope::Project => "project",
    }
}

pub fn parse_credential_mapping_scope(value: &str) -> GatewayResult<LiteLlmCredentialMappingScope> {
    match value {
        "key" => Ok(LiteLlmCredentialMappingScope::Key),
        "project" => Ok(LiteLlmCredentialMappingScope::Project),
        _ => Err(GatewayError::InvalidProviderConfigPayload),
    }
}

pub fn validate_litellm_credential_header_name(header: &str) -> GatewayResult<()> {
    let header = header.trim();
    if header.is_empty() || HeaderName::from_bytes(header.as_bytes()).is_err() {
        return Err(GatewayError::InvalidProviderConfigPayload);
    }
    let normalized = header.to_ascii_lowercase();
    let blocked = [
        "host",
        "content-length",
        "authorization",
        "proxy-authorization",
        "x-relayna-key",
        "x-aih-api-key",
        "x-api-key",
        "x-relayna-worker-token",
        "x-apigee-entra-identity",
        "x-apigee-entra-signature",
    ];
    if blocked.contains(&normalized.as_str()) || normalized.starts_with("x-relayna-") {
        return Err(GatewayError::InvalidProviderConfigPayload);
    }
    Ok(())
}

fn validate_header_settings(mode: CredentialHeaderMode, header: Option<&str>) -> GatewayResult<()> {
    match mode {
        CredentialHeaderMode::AuthorizationBearer => Ok(()),
        CredentialHeaderMode::CustomHeader => {
            let header = header.ok_or(GatewayError::InvalidProviderConfigPayload)?;
            validate_litellm_credential_header_name(header)
        }
    }
}

fn default_enabled() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_litellm_custom_header_names() {
        validate_litellm_credential_header_name("x-litellm-api-key").expect("valid");
        assert_eq!(
            validate_litellm_credential_header_name("authorization").unwrap_err(),
            GatewayError::InvalidProviderConfigPayload
        );
        assert_eq!(
            validate_litellm_credential_header_name("x-relayna-key").unwrap_err(),
            GatewayError::InvalidProviderConfigPayload
        );
        assert_eq!(
            validate_litellm_credential_header_name("not a header").unwrap_err(),
            GatewayError::InvalidProviderConfigPayload
        );
    }

    #[test]
    fn parses_credential_header_value_format() {
        assert_eq!(
            parse_credential_header_value_format("raw").expect("raw"),
            CredentialHeaderValueFormat::Raw
        );
        assert_eq!(
            parse_credential_header_value_format("bearer").expect("bearer"),
            CredentialHeaderValueFormat::Bearer
        );
        assert_eq!(
            parse_credential_header_value_format("basic").unwrap_err(),
            GatewayError::InvalidProviderConfigPayload
        );
    }
}
