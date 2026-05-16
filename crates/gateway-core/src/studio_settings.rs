use crate::{GatewayError, GatewayResult};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{de::DeserializeOwned, Deserialize, Deserializer, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StudioConnectionSource {
    Persisted,
    Environment,
    Unset,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StudioConnectionEnv {
    pub base_url: Option<String>,
    pub token: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StoredStudioConnection {
    pub base_url: Option<String>,
    pub bearer_token_secret: Option<String>,
    pub updated_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectiveStudioConnection {
    pub base_url: Option<String>,
    pub token: Option<String>,
    pub source: StudioConnectionSource,
    pub updated_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct StudioConnectionResponse {
    pub base_url: Option<String>,
    pub token_configured: bool,
    pub source: StudioConnectionSource,
    pub updated_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct StudioConnectionTestResponse {
    pub ok: bool,
    pub service_count: usize,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct StudioConnectionPatchRequest {
    #[serde(default, deserialize_with = "deserialize_patch_value")]
    pub base_url: PatchValue<String>,
    #[serde(default, deserialize_with = "deserialize_patch_value")]
    pub token: PatchValue<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum PatchValue<T> {
    #[default]
    Unchanged,
    Clear,
    Set(T),
}

#[async_trait]
pub trait AdminStudioConnectionStore: Send + Sync {
    async fn studio_connection_settings(&self) -> GatewayResult<Option<StoredStudioConnection>>;
    async fn patch_studio_connection_settings(
        &self,
        patch: StudioConnectionPatchRequest,
    ) -> GatewayResult<StoredStudioConnection>;
}

#[async_trait]
impl<T> AdminStudioConnectionStore for std::sync::Arc<T>
where
    T: AdminStudioConnectionStore + ?Sized,
{
    async fn studio_connection_settings(&self) -> GatewayResult<Option<StoredStudioConnection>> {
        (**self).studio_connection_settings().await
    }

    async fn patch_studio_connection_settings(
        &self,
        patch: StudioConnectionPatchRequest,
    ) -> GatewayResult<StoredStudioConnection> {
        (**self).patch_studio_connection_settings(patch).await
    }
}

impl StudioConnectionPatchRequest {
    pub fn validate(&self) -> GatewayResult<()> {
        if let PatchValue::Set(base_url) = &self.base_url {
            validate_base_url(base_url)?;
        }
        if let PatchValue::Set(token) = &self.token {
            validate_secret(token)?;
        }
        Ok(())
    }
}

impl EffectiveStudioConnection {
    pub fn from_sources(
        stored: Option<StoredStudioConnection>,
        env: &StudioConnectionEnv,
    ) -> EffectiveStudioConnection {
        if let Some(stored) = stored {
            if let Some(base_url) = normalized_non_empty(stored.base_url.as_deref()) {
                return EffectiveStudioConnection {
                    base_url: Some(base_url),
                    token: stored
                        .bearer_token_secret
                        .as_deref()
                        .and_then(|value| normalized_non_empty(Some(value))),
                    source: StudioConnectionSource::Persisted,
                    updated_at: stored.updated_at,
                };
            }
        }

        if let Some(base_url) = normalized_non_empty(env.base_url.as_deref()) {
            return EffectiveStudioConnection {
                base_url: Some(base_url),
                token: env
                    .token
                    .as_deref()
                    .and_then(|value| normalized_non_empty(Some(value))),
                source: StudioConnectionSource::Environment,
                updated_at: None,
            };
        }

        EffectiveStudioConnection {
            base_url: None,
            token: None,
            source: StudioConnectionSource::Unset,
            updated_at: None,
        }
    }

    pub fn response(&self) -> StudioConnectionResponse {
        StudioConnectionResponse {
            base_url: self.base_url.clone(),
            token_configured: self.token.is_some(),
            source: self.source,
            updated_at: self.updated_at,
        }
    }
}

pub fn normalize_base_url(base_url: &str) -> GatewayResult<String> {
    let trimmed = base_url.trim().trim_end_matches('/').to_owned();
    validate_base_url(&trimmed)?;
    Ok(trimmed)
}

pub fn normalize_secret(secret: &str) -> GatewayResult<String> {
    let trimmed = secret.trim().to_owned();
    validate_secret(&trimmed)?;
    Ok(trimmed)
}

fn deserialize_patch_value<'de, D, T>(deserializer: D) -> Result<PatchValue<T>, D::Error>
where
    D: Deserializer<'de>,
    T: DeserializeOwned,
{
    Option::<T>::deserialize(deserializer).map(|value| match value {
        Some(value) => PatchValue::Set(value),
        None => PatchValue::Clear,
    })
}

fn validate_base_url(base_url: &str) -> GatewayResult<()> {
    let url =
        url::Url::parse(base_url).map_err(|_| GatewayError::InvalidStudioConnectionPayload)?;
    match url.scheme() {
        "http" | "https" if url.host_str().is_some() => Ok(()),
        _ => Err(GatewayError::InvalidStudioConnectionPayload),
    }
}

fn validate_secret(secret: &str) -> GatewayResult<()> {
    if secret.trim().is_empty() {
        Err(GatewayError::InvalidStudioConnectionPayload)
    } else {
        Ok(())
    }
}

fn normalized_non_empty(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persisted_settings_override_environment() {
        let effective = EffectiveStudioConnection::from_sources(
            Some(StoredStudioConnection {
                base_url: Some("http://stored.example".to_owned()),
                bearer_token_secret: Some("stored-token".to_owned()),
                updated_at: None,
            }),
            &StudioConnectionEnv {
                base_url: Some("http://env.example".to_owned()),
                token: Some("env-token".to_owned()),
            },
        );

        assert_eq!(effective.base_url.as_deref(), Some("http://stored.example"));
        assert_eq!(effective.token.as_deref(), Some("stored-token"));
        assert_eq!(effective.source, StudioConnectionSource::Persisted);
        assert!(effective.response().token_configured);
    }

    #[test]
    fn cleared_persisted_base_url_reveals_environment() {
        let effective = EffectiveStudioConnection::from_sources(
            Some(StoredStudioConnection {
                base_url: None,
                bearer_token_secret: None,
                updated_at: None,
            }),
            &StudioConnectionEnv {
                base_url: Some("http://env.example".to_owned()),
                token: Some("env-token".to_owned()),
            },
        );

        assert_eq!(effective.base_url.as_deref(), Some("http://env.example"));
        assert_eq!(effective.token.as_deref(), Some("env-token"));
        assert_eq!(effective.source, StudioConnectionSource::Environment);
    }

    #[test]
    fn unset_when_no_persisted_or_environment_base_url() {
        let effective = EffectiveStudioConnection::from_sources(
            Some(StoredStudioConnection {
                base_url: None,
                bearer_token_secret: Some("orphan".to_owned()),
                updated_at: None,
            }),
            &StudioConnectionEnv::default(),
        );

        assert_eq!(effective.base_url, None);
        assert!(!effective.response().token_configured);
        assert_eq!(effective.source, StudioConnectionSource::Unset);
    }

    #[test]
    fn patch_request_distinguishes_omitted_null_and_set_values() {
        let omitted: StudioConnectionPatchRequest = serde_json::from_str("{}").expect("json");
        assert_eq!(omitted.base_url, PatchValue::Unchanged);
        assert_eq!(omitted.token, PatchValue::Unchanged);

        let cleared: StudioConnectionPatchRequest =
            serde_json::from_str(r#"{"base_url":null,"token":null}"#).expect("json");
        assert_eq!(cleared.base_url, PatchValue::Clear);
        assert_eq!(cleared.token, PatchValue::Clear);

        let set: StudioConnectionPatchRequest =
            serde_json::from_str(r#"{"base_url":"http://studio.example","token":"secret"}"#)
                .expect("json");
        assert_eq!(
            set.base_url,
            PatchValue::Set("http://studio.example".to_owned())
        );
        assert_eq!(set.token, PatchValue::Set("secret".to_owned()));
    }

    #[test]
    fn response_never_exposes_token_value() {
        let effective = EffectiveStudioConnection {
            base_url: Some("http://studio.example".to_owned()),
            token: Some("secret".to_owned()),
            source: StudioConnectionSource::Persisted,
            updated_at: None,
        };

        let value = serde_json::to_value(effective.response()).expect("json");
        assert_eq!(value["token_configured"], true);
        assert!(value.get("token").is_none());
    }

    #[test]
    fn accepts_uppercase_http_scheme() {
        assert_eq!(
            normalize_base_url("HTTP://studio.example/").expect("valid url"),
            "HTTP://studio.example"
        );
    }
}
