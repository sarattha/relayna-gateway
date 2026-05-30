use crate::{
    validate_relayna_key_header_name, ApigeeTrustedHeaderConfig, EntraAuthConfig, EntraJwtVerifier,
    GatewayError, GatewayResult, ENTRA_DEFAULT_RELAYNA_KEY_HEADER,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{de::DeserializeOwned, Deserialize, Deserializer, Serialize};
use std::sync::{Arc, RwLock};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GatewayAuthSettingsSource {
    Persisted,
    Environment,
    Unset,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatewayAuthEnv {
    pub relayna_key_header: String,
    pub entra_auth: Option<EntraAuthConfig>,
    pub apigee_trusted_header: Option<ApigeeTrustedHeaderConfig>,
}

impl Default for GatewayAuthEnv {
    fn default() -> Self {
        Self {
            relayna_key_header: ENTRA_DEFAULT_RELAYNA_KEY_HEADER.to_owned(),
            entra_auth: None,
            apigee_trusted_header: None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StoredGatewayAuthSettings {
    pub entra_enabled: bool,
    pub tenant_id: Option<String>,
    pub audience: Option<String>,
    pub issuer: Option<String>,
    pub oidc_discovery_url: Option<String>,
    pub required_scope: Option<String>,
    pub required_role: Option<String>,
    pub allowed_groups: Vec<String>,
    pub accepted_algorithms: Vec<String>,
    pub relayna_key_header: Option<String>,
    pub jwks_cache_ttl_seconds: Option<i64>,
    pub clock_skew_seconds: Option<i64>,
    pub apigee_trusted_header_enabled: bool,
    pub apigee_trusted_header_secret: Option<String>,
    pub updated_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectiveGatewayAuthSettings {
    pub source: GatewayAuthSettingsSource,
    pub relayna_key_header: String,
    pub entra_auth: Option<EntraAuthConfig>,
    pub apigee_trusted_header: Option<ApigeeTrustedHeaderConfig>,
    pub updated_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct GatewayAuthSettingsResponse {
    pub source: GatewayAuthSettingsSource,
    pub updated_at: Option<DateTime<Utc>>,
    pub relayna_key_header: String,
    pub entra: EntraAuthSettingsResponse,
    pub apigee: ApigeeAuthSettingsResponse,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct EntraAuthSettingsResponse {
    pub enabled: bool,
    pub tenant_id: Option<String>,
    pub audience: Option<String>,
    pub issuer: Option<String>,
    pub oidc_discovery_url: Option<String>,
    pub required_scope: Option<String>,
    pub required_role: Option<String>,
    pub allowed_groups: Vec<String>,
    pub accepted_algorithms: Vec<String>,
    pub jwks_cache_ttl_seconds: u64,
    pub clock_skew_seconds: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ApigeeAuthSettingsResponse {
    pub trusted_header_enabled: bool,
    pub secret_configured: bool,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct GatewayAuthSettingsPatchRequest {
    pub relayna_key_header: Option<String>,
    #[serde(default)]
    pub entra_enabled: Option<bool>,
    #[serde(default, deserialize_with = "deserialize_patch_value")]
    pub tenant_id: AuthPatchValue<String>,
    #[serde(default, deserialize_with = "deserialize_patch_value")]
    pub audience: AuthPatchValue<String>,
    #[serde(default, deserialize_with = "deserialize_patch_value")]
    pub issuer: AuthPatchValue<String>,
    #[serde(default, deserialize_with = "deserialize_patch_value")]
    pub oidc_discovery_url: AuthPatchValue<String>,
    #[serde(default, deserialize_with = "deserialize_patch_value")]
    pub required_scope: AuthPatchValue<String>,
    #[serde(default, deserialize_with = "deserialize_patch_value")]
    pub required_role: AuthPatchValue<String>,
    #[serde(default)]
    pub allowed_groups: Option<Vec<String>>,
    #[serde(default)]
    pub accepted_algorithms: Option<Vec<String>>,
    pub jwks_cache_ttl_seconds: Option<u64>,
    pub clock_skew_seconds: Option<i64>,
    #[serde(default)]
    pub apigee_trusted_header_enabled: Option<bool>,
    #[serde(default, deserialize_with = "deserialize_patch_value")]
    pub apigee_trusted_header_secret: AuthPatchValue<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum AuthPatchValue<T> {
    #[default]
    Unchanged,
    Clear,
    Set(T),
}

#[async_trait]
pub trait AdminGatewayAuthSettingsStore: Send + Sync {
    async fn gateway_auth_settings(&self) -> GatewayResult<Option<StoredGatewayAuthSettings>>;
    async fn patch_gateway_auth_settings(
        &self,
        patch: GatewayAuthSettingsPatchRequest,
    ) -> GatewayResult<StoredGatewayAuthSettings>;
}

#[async_trait]
impl<T> AdminGatewayAuthSettingsStore for Arc<T>
where
    T: AdminGatewayAuthSettingsStore + ?Sized,
{
    async fn gateway_auth_settings(&self) -> GatewayResult<Option<StoredGatewayAuthSettings>> {
        (**self).gateway_auth_settings().await
    }

    async fn patch_gateway_auth_settings(
        &self,
        patch: GatewayAuthSettingsPatchRequest,
    ) -> GatewayResult<StoredGatewayAuthSettings> {
        (**self).patch_gateway_auth_settings(patch).await
    }
}

impl EffectiveGatewayAuthSettings {
    pub fn from_sources(
        stored: Option<StoredGatewayAuthSettings>,
        env: &GatewayAuthEnv,
    ) -> GatewayResult<Self> {
        if let Some(stored) = stored {
            let relayna_key_header = normalized_non_empty(stored.relayna_key_header.as_deref())
                .unwrap_or_else(|| env.relayna_key_header.clone());
            let entra_auth = if stored.entra_enabled {
                Some(stored.entra_config(relayna_key_header.clone())?)
            } else {
                None
            };
            let apigee_trusted_header = if stored.apigee_trusted_header_enabled {
                Some(stored.apigee_config()?)
            } else {
                None
            };
            return Ok(Self {
                source: GatewayAuthSettingsSource::Persisted,
                relayna_key_header,
                entra_auth,
                apigee_trusted_header,
                updated_at: stored.updated_at,
            });
        }

        if env.entra_auth.is_some() || env.apigee_trusted_header.is_some() {
            return Ok(Self {
                source: GatewayAuthSettingsSource::Environment,
                relayna_key_header: env.relayna_key_header.clone(),
                entra_auth: env.entra_auth.clone(),
                apigee_trusted_header: env.apigee_trusted_header.clone(),
                updated_at: None,
            });
        }

        Ok(Self {
            source: GatewayAuthSettingsSource::Unset,
            relayna_key_header: env.relayna_key_header.clone(),
            entra_auth: None,
            apigee_trusted_header: None,
            updated_at: None,
        })
    }

    pub fn runtime_config(&self) -> GatewayAuthRuntimeConfig {
        GatewayAuthRuntimeConfig {
            relayna_key_header: self.relayna_key_header.clone(),
            entra_auth: self.entra_auth.clone(),
            apigee_trusted_header: self.apigee_trusted_header.clone(),
        }
    }

    pub fn response(&self) -> GatewayAuthSettingsResponse {
        GatewayAuthSettingsResponse {
            source: self.source,
            updated_at: self.updated_at,
            relayna_key_header: self.relayna_key_header.clone(),
            entra: self
                .entra_auth
                .as_ref()
                .map(entra_response)
                .unwrap_or_else(|| EntraAuthSettingsResponse {
                    enabled: false,
                    tenant_id: None,
                    audience: None,
                    issuer: None,
                    oidc_discovery_url: None,
                    required_scope: None,
                    required_role: None,
                    allowed_groups: Vec::new(),
                    accepted_algorithms: vec!["RS256".to_owned()],
                    jwks_cache_ttl_seconds: 300,
                    clock_skew_seconds: 60,
                }),
            apigee: ApigeeAuthSettingsResponse {
                trusted_header_enabled: self.apigee_trusted_header.is_some(),
                secret_configured: self.apigee_trusted_header.is_some(),
            },
        }
    }
}

impl StoredGatewayAuthSettings {
    pub fn apply_patch(mut self, patch: GatewayAuthSettingsPatchRequest) -> GatewayResult<Self> {
        if let Some(value) = patch.entra_enabled {
            self.entra_enabled = value;
        }
        if let Some(value) = patch.apigee_trusted_header_enabled {
            self.apigee_trusted_header_enabled = value;
        }
        if let Some(value) = patch.relayna_key_header {
            let value = normalize_optional_string(Some(value));
            if let Some(header) = value.as_deref() {
                validate_relayna_key_header_name(header)?;
            }
            self.relayna_key_header = value;
        }
        apply_string_patch(&mut self.tenant_id, patch.tenant_id);
        apply_string_patch(&mut self.audience, patch.audience);
        apply_string_patch(&mut self.issuer, patch.issuer);
        apply_string_patch(&mut self.oidc_discovery_url, patch.oidc_discovery_url);
        apply_string_patch(&mut self.required_scope, patch.required_scope);
        apply_string_patch(&mut self.required_role, patch.required_role);
        apply_string_patch(
            &mut self.apigee_trusted_header_secret,
            patch.apigee_trusted_header_secret,
        );
        if let Some(values) = patch.allowed_groups {
            self.allowed_groups = normalize_string_list(values);
        }
        if let Some(values) = patch.accepted_algorithms {
            self.accepted_algorithms = normalize_string_list(values);
        }
        if let Some(value) = patch.jwks_cache_ttl_seconds {
            self.jwks_cache_ttl_seconds =
                Some(i64::try_from(value).map_err(|_| GatewayError::InvalidConfiguration)?);
        }
        if let Some(value) = patch.clock_skew_seconds {
            self.clock_skew_seconds = Some(value);
        }

        self.validate()?;
        Ok(self)
    }

    pub fn validate(&self) -> GatewayResult<()> {
        if self.entra_enabled {
            self.entra_config(
                self.relayna_key_header
                    .clone()
                    .unwrap_or_else(|| ENTRA_DEFAULT_RELAYNA_KEY_HEADER.to_owned()),
            )?;
        }
        if self.apigee_trusted_header_enabled {
            self.apigee_config()?;
        }
        Ok(())
    }

    fn entra_config(&self, relayna_key_header: String) -> GatewayResult<EntraAuthConfig> {
        let config = EntraAuthConfig {
            tenant_id: required_field(self.tenant_id.as_deref())?,
            audience: required_field(self.audience.as_deref())?,
            issuer: required_field(self.issuer.as_deref())?,
            oidc_discovery_url: required_field(self.oidc_discovery_url.as_deref())?,
            required_scope: normalized_non_empty(self.required_scope.as_deref()),
            required_role: normalized_non_empty(self.required_role.as_deref()),
            allowed_groups: self.allowed_groups.clone(),
            accepted_algorithms: if self.accepted_algorithms.is_empty() {
                vec!["RS256".to_owned()]
            } else {
                self.accepted_algorithms.clone()
            },
            relayna_key_header,
            jwks_cache_ttl_seconds: self
                .jwks_cache_ttl_seconds
                .and_then(|value| u64::try_from(value).ok())
                .unwrap_or(300),
            clock_skew_seconds: self.clock_skew_seconds.unwrap_or(60),
        };
        config.validate()?;
        Ok(config)
    }

    fn apigee_config(&self) -> GatewayResult<ApigeeTrustedHeaderConfig> {
        let config = ApigeeTrustedHeaderConfig {
            secret: required_field(self.apigee_trusted_header_secret.as_deref())?,
            required_scope: normalized_non_empty(self.required_scope.as_deref()),
            required_role: normalized_non_empty(self.required_role.as_deref()),
            allowed_groups: self.allowed_groups.clone(),
        };
        config.validate()?;
        Ok(config)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatewayAuthRuntimeConfig {
    pub relayna_key_header: String,
    pub entra_auth: Option<EntraAuthConfig>,
    pub apigee_trusted_header: Option<ApigeeTrustedHeaderConfig>,
}

impl Default for GatewayAuthRuntimeConfig {
    fn default() -> Self {
        Self {
            relayna_key_header: ENTRA_DEFAULT_RELAYNA_KEY_HEADER.to_owned(),
            entra_auth: None,
            apigee_trusted_header: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct GatewayAuthRuntimeSnapshot {
    pub config: GatewayAuthRuntimeConfig,
    pub entra_verifier: Option<Arc<EntraJwtVerifier>>,
}

#[derive(Debug)]
struct GatewayAuthRuntimeState {
    config: GatewayAuthRuntimeConfig,
    entra_verifier: Option<Arc<EntraJwtVerifier>>,
}

#[derive(Debug, Clone)]
pub struct SharedGatewayAuthRuntime {
    inner: Arc<RwLock<GatewayAuthRuntimeState>>,
}

impl SharedGatewayAuthRuntime {
    pub fn new(config: GatewayAuthRuntimeConfig) -> GatewayResult<Self> {
        Ok(Self {
            inner: Arc::new(RwLock::new(GatewayAuthRuntimeState::new(config)?)),
        })
    }

    pub fn update(&self, config: GatewayAuthRuntimeConfig) -> GatewayResult<()> {
        let next = GatewayAuthRuntimeState::new(config)?;
        let mut guard = self
            .inner
            .write()
            .map_err(|_| GatewayError::InvalidConfiguration)?;
        *guard = next;
        Ok(())
    }

    pub fn snapshot(&self) -> GatewayResult<GatewayAuthRuntimeSnapshot> {
        let guard = self
            .inner
            .read()
            .map_err(|_| GatewayError::InvalidConfiguration)?;
        Ok(GatewayAuthRuntimeSnapshot {
            config: guard.config.clone(),
            entra_verifier: guard.entra_verifier.clone(),
        })
    }
}

impl GatewayAuthRuntimeSnapshot {
    pub fn entra_enabled(&self) -> bool {
        self.entra_verifier.is_some() || self.config.apigee_trusted_header.is_some()
    }
}

impl GatewayAuthRuntimeState {
    fn new(config: GatewayAuthRuntimeConfig) -> GatewayResult<Self> {
        validate_relayna_key_header_name(&config.relayna_key_header)?;
        let entra_verifier = config
            .entra_auth
            .clone()
            .map(EntraJwtVerifier::new)
            .transpose()?
            .map(Arc::new);
        Ok(Self {
            config,
            entra_verifier,
        })
    }
}

fn entra_response(config: &EntraAuthConfig) -> EntraAuthSettingsResponse {
    EntraAuthSettingsResponse {
        enabled: true,
        tenant_id: Some(config.tenant_id.clone()),
        audience: Some(config.audience.clone()),
        issuer: Some(config.issuer.clone()),
        oidc_discovery_url: Some(config.oidc_discovery_url.clone()),
        required_scope: config.required_scope.clone(),
        required_role: config.required_role.clone(),
        allowed_groups: config.allowed_groups.clone(),
        accepted_algorithms: config.accepted_algorithms.clone(),
        jwks_cache_ttl_seconds: config.jwks_cache_ttl_seconds,
        clock_skew_seconds: config.clock_skew_seconds,
    }
}

fn deserialize_patch_value<'de, D, T>(deserializer: D) -> Result<AuthPatchValue<T>, D::Error>
where
    D: Deserializer<'de>,
    T: DeserializeOwned,
{
    Option::<T>::deserialize(deserializer).map(|value| match value {
        Some(value) => AuthPatchValue::Set(value),
        None => AuthPatchValue::Clear,
    })
}

fn apply_string_patch(target: &mut Option<String>, patch: AuthPatchValue<String>) {
    match patch {
        AuthPatchValue::Unchanged => {}
        AuthPatchValue::Clear => *target = None,
        AuthPatchValue::Set(value) => *target = normalize_optional_string(Some(value)),
    }
}

fn normalize_optional_string(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn normalize_string_list(values: Vec<String>) -> Vec<String> {
    values
        .into_iter()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .collect()
}

fn normalized_non_empty(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn required_field(value: Option<&str>) -> GatewayResult<String> {
    normalized_non_empty(value).ok_or(GatewayError::InvalidConfiguration)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stored_enabled() -> StoredGatewayAuthSettings {
        StoredGatewayAuthSettings {
            entra_enabled: true,
            tenant_id: Some("tenant".to_owned()),
            audience: Some("api://gateway".to_owned()),
            issuer: Some("https://login.example/tenant/v2.0".to_owned()),
            oidc_discovery_url: Some(
                "https://login.example/tenant/.well-known/openid-configuration".to_owned(),
            ),
            required_scope: Some("gateway.invoke".to_owned()),
            required_role: None,
            allowed_groups: vec!["gateway-users".to_owned()],
            accepted_algorithms: vec!["RS256".to_owned()],
            relayna_key_header: Some("X-Relayna-Key".to_owned()),
            jwks_cache_ttl_seconds: Some(120),
            clock_skew_seconds: Some(30),
            apigee_trusted_header_enabled: true,
            apigee_trusted_header_secret: Some("secret".to_owned()),
            updated_at: None,
        }
    }

    #[test]
    fn persisted_settings_override_environment() {
        let effective = EffectiveGatewayAuthSettings::from_sources(
            Some(stored_enabled()),
            &GatewayAuthEnv {
                relayna_key_header: "X-Env-Key".to_owned(),
                entra_auth: None,
                apigee_trusted_header: None,
            },
        )
        .expect("effective settings");

        assert_eq!(effective.source, GatewayAuthSettingsSource::Persisted);
        assert_eq!(effective.relayna_key_header, "X-Relayna-Key");
        assert!(effective.entra_auth.is_some());
        assert!(effective.apigee_trusted_header.is_some());
        assert!(effective.response().apigee.secret_configured);
    }

    #[test]
    fn disabled_persisted_settings_override_environment() {
        let env = GatewayAuthEnv {
            relayna_key_header: "X-Env-Key".to_owned(),
            entra_auth: Some(EntraAuthConfig {
                tenant_id: "env-tenant".to_owned(),
                audience: "api://env".to_owned(),
                issuer: "https://login.example/env/v2.0".to_owned(),
                oidc_discovery_url: "https://login.example/env/.well-known/openid-configuration"
                    .to_owned(),
                required_scope: None,
                required_role: None,
                allowed_groups: Vec::new(),
                accepted_algorithms: vec!["RS256".to_owned()],
                relayna_key_header: "X-Env-Key".to_owned(),
                jwks_cache_ttl_seconds: 300,
                clock_skew_seconds: 60,
            }),
            apigee_trusted_header: None,
        };
        let effective = EffectiveGatewayAuthSettings::from_sources(
            Some(StoredGatewayAuthSettings::default()),
            &env,
        )
        .expect("effective settings");

        assert_eq!(effective.source, GatewayAuthSettingsSource::Persisted);
        assert_eq!(effective.relayna_key_header, "X-Env-Key");
        assert!(effective.entra_auth.is_none());
    }

    #[test]
    fn enabled_entra_requires_core_fields() {
        let patch = GatewayAuthSettingsPatchRequest {
            relayna_key_header: None,
            entra_enabled: Some(true),
            tenant_id: AuthPatchValue::Unchanged,
            audience: AuthPatchValue::Unchanged,
            issuer: AuthPatchValue::Unchanged,
            oidc_discovery_url: AuthPatchValue::Unchanged,
            required_scope: AuthPatchValue::Unchanged,
            required_role: AuthPatchValue::Unchanged,
            allowed_groups: None,
            accepted_algorithms: None,
            jwks_cache_ttl_seconds: None,
            clock_skew_seconds: None,
            apigee_trusted_header_enabled: None,
            apigee_trusted_header_secret: AuthPatchValue::Unchanged,
        };

        assert_eq!(
            StoredGatewayAuthSettings::default()
                .apply_patch(patch)
                .unwrap_err(),
            GatewayError::InvalidConfiguration
        );
    }
}
