use crate::{GatewayError, GatewayResult, Provider};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
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

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct ProviderConfigCreateRequest {
    pub provider: ProviderConfigKind,
    pub name: String,
    pub base_url: String,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub credential: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default, PartialEq)]
pub struct ProviderConfigPatchRequest {
    pub name: Option<String>,
    pub base_url: Option<String>,
    pub enabled: Option<bool>,
    pub credential: Option<Option<String>>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ProviderConfigResponse {
    pub id: Uuid,
    pub provider: ProviderConfigKind,
    pub name: String,
    pub base_url: String,
    pub enabled: bool,
    pub credential_configured: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderRuntimeConfig {
    pub provider: Provider,
    pub base_url: String,
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
}

#[async_trait]
pub trait ProviderConfigLookup: Send + Sync {
    async fn active_litellm_config(&self) -> GatewayResult<Option<ProviderRuntimeConfig>>;
}

#[async_trait]
impl<T> ProviderConfigLookup for std::sync::Arc<T>
where
    T: ProviderConfigLookup + ?Sized,
{
    async fn active_litellm_config(&self) -> GatewayResult<Option<ProviderRuntimeConfig>> {
        (**self).active_litellm_config().await
    }
}

impl ProviderConfigCreateRequest {
    pub fn validate(&self) -> GatewayResult<()> {
        validate_name(&self.name)?;
        validate_base_url(&self.base_url)?;
        validate_optional_secret(self.credential.as_deref())?;
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
        Ok(())
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

fn default_enabled() -> bool {
    true
}
