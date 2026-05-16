use crate::{
    errors::{GatewayError, GatewayResult},
    GuardrailPolicy, GuardrailPolicyPatch,
};
use argon2::{
    password_hash::{rand_core::OsRng, PasswordHasher, SaltString},
    Argon2,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

const RELAYNA_KEY_PREFIX: &str = "rk_live_";
const LOOKUP_PREFIX_LEN: usize = 16;

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct AdminKeyCreate {
    #[serde(default)]
    pub owner_type: AdminKeyOwnerType,
    #[serde(default)]
    pub project_id: Option<Uuid>,
    #[serde(default)]
    pub service_names: Vec<String>,
    pub expires_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub policy: KeyPolicyPatch,
    #[serde(default)]
    pub guardrail_policy: GuardrailPolicy,
}

#[derive(Debug, Clone, Deserialize, Default, PartialEq)]
pub struct AdminKeyPatch {
    pub owner_type: Option<AdminKeyOwnerType>,
    pub project_id: Option<Option<Uuid>>,
    pub service_names: Option<Vec<String>>,
    pub expires_at: Option<Option<DateTime<Utc>>>,
    pub disabled: Option<bool>,
    #[serde(default)]
    pub policy: Option<KeyPolicyPatch>,
    #[serde(default)]
    pub guardrail_policy: Option<GuardrailPolicyPatch>,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AdminKeyOwnerType {
    #[default]
    Project,
    Individual,
}

#[derive(Debug, Clone, Deserialize, Default, PartialEq)]
pub struct KeyPolicyPatch {
    pub allowed_routes: Option<Vec<String>>,
    pub allowed_models: Option<Vec<String>>,
    pub allowed_providers: Option<Vec<String>>,
    pub allowed_services: Option<Vec<String>>,
    pub rpm_limit: Option<Option<i32>>,
    pub tpm_limit: Option<Option<i32>>,
    pub daily_budget_usd: Option<Option<f64>>,
    pub monthly_budget_usd: Option<Option<f64>>,
    pub allow_streaming: Option<bool>,
    pub allow_tools: Option<bool>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct CreatedAdminKeyResponse {
    pub key: AdminKeyResponse,
    pub raw_key: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct AdminKeyResponse {
    pub id: Uuid,
    pub owner_type: AdminKeyOwnerType,
    pub project_id: Option<Uuid>,
    pub service_names: Vec<String>,
    pub key_prefix: String,
    pub disabled: bool,
    pub revoked_at: Option<DateTime<Utc>>,
    pub expires_at: Option<DateTime<Utc>>,
    pub policy: AdminPolicyResponse,
    pub guardrail_policy: GuardrailPolicy,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct AdminPolicyResponse {
    pub allowed_routes: Vec<String>,
    pub allowed_models: Vec<String>,
    pub allowed_providers: Vec<String>,
    pub allowed_services: Vec<String>,
    pub rpm_limit: Option<i32>,
    pub tpm_limit: Option<i32>,
    pub daily_budget_usd: Option<f64>,
    pub monthly_budget_usd: Option<f64>,
    pub allow_streaming: bool,
    pub allow_tools: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct AdminKeyUsageSummary {
    pub key_id: Uuid,
    pub request_count: i64,
    pub success_count: i64,
    pub failure_count: i64,
    pub total_latency_ms: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub total_tokens: i64,
    pub estimated_cost_usd: Option<f64>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ProjectUsageSummary {
    pub project_id: Uuid,
    pub request_count: i64,
    pub success_count: i64,
    pub failure_count: i64,
    pub total_latency_ms: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub total_tokens: i64,
    pub estimated_cost_usd: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VirtualKeyMaterial {
    pub raw_key: String,
    pub key_prefix: String,
    pub key_hash: String,
}

#[async_trait]
pub trait AdminKeyStore: Send + Sync {
    async fn create_admin_key(
        &self,
        request: AdminKeyCreate,
        material: &VirtualKeyMaterial,
    ) -> GatewayResult<AdminKeyResponse>;

    async fn list_admin_keys(&self) -> GatewayResult<Vec<AdminKeyResponse>>;

    async fn get_admin_key(&self, key_id: Uuid) -> GatewayResult<Option<AdminKeyResponse>>;

    async fn patch_admin_key(
        &self,
        key_id: Uuid,
        patch: AdminKeyPatch,
    ) -> GatewayResult<Option<AdminKeyResponse>>;

    async fn revoke_admin_key(&self, key_id: Uuid) -> GatewayResult<Option<AdminKeyResponse>>;

    async fn disable_admin_key(&self, key_id: Uuid) -> GatewayResult<Option<AdminKeyResponse>>;

    async fn enable_admin_key(&self, key_id: Uuid) -> GatewayResult<Option<AdminKeyResponse>>;

    async fn key_usage_summary(&self, key_id: Uuid) -> GatewayResult<Option<AdminKeyUsageSummary>>;

    async fn project_usage_summary(&self, project_id: Uuid) -> GatewayResult<ProjectUsageSummary>;
}

#[async_trait]
impl<T> AdminKeyStore for std::sync::Arc<T>
where
    T: AdminKeyStore + ?Sized,
{
    async fn create_admin_key(
        &self,
        request: AdminKeyCreate,
        material: &VirtualKeyMaterial,
    ) -> GatewayResult<AdminKeyResponse> {
        (**self).create_admin_key(request, material).await
    }

    async fn list_admin_keys(&self) -> GatewayResult<Vec<AdminKeyResponse>> {
        (**self).list_admin_keys().await
    }

    async fn get_admin_key(&self, key_id: Uuid) -> GatewayResult<Option<AdminKeyResponse>> {
        (**self).get_admin_key(key_id).await
    }

    async fn patch_admin_key(
        &self,
        key_id: Uuid,
        patch: AdminKeyPatch,
    ) -> GatewayResult<Option<AdminKeyResponse>> {
        (**self).patch_admin_key(key_id, patch).await
    }

    async fn revoke_admin_key(&self, key_id: Uuid) -> GatewayResult<Option<AdminKeyResponse>> {
        (**self).revoke_admin_key(key_id).await
    }

    async fn disable_admin_key(&self, key_id: Uuid) -> GatewayResult<Option<AdminKeyResponse>> {
        (**self).disable_admin_key(key_id).await
    }

    async fn enable_admin_key(&self, key_id: Uuid) -> GatewayResult<Option<AdminKeyResponse>> {
        (**self).enable_admin_key(key_id).await
    }

    async fn key_usage_summary(&self, key_id: Uuid) -> GatewayResult<Option<AdminKeyUsageSummary>> {
        (**self).key_usage_summary(key_id).await
    }

    async fn project_usage_summary(&self, project_id: Uuid) -> GatewayResult<ProjectUsageSummary> {
        (**self).project_usage_summary(project_id).await
    }
}

impl VirtualKeyMaterial {
    pub fn generate() -> GatewayResult<Self> {
        let raw_key = format!(
            "{RELAYNA_KEY_PREFIX}{}{}",
            Uuid::new_v4().simple(),
            Uuid::new_v4().simple()
        );
        Self::from_raw(raw_key)
    }

    pub fn from_raw(raw_key: String) -> GatewayResult<Self> {
        if !raw_key.starts_with(RELAYNA_KEY_PREFIX) || raw_key.len() <= LOOKUP_PREFIX_LEN {
            return Err(GatewayError::MalformedAuthorization);
        }
        let key_prefix = raw_key.chars().take(LOOKUP_PREFIX_LEN).collect();
        let salt = SaltString::generate(&mut OsRng);
        let key_hash = Argon2::default()
            .hash_password(raw_key.as_bytes(), &salt)
            .map_err(|_| GatewayError::InvalidConfiguration)?
            .to_string();

        Ok(Self {
            raw_key,
            key_prefix,
            key_hash,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::verify_secret;

    #[test]
    fn generated_key_returns_hash_and_lookup_prefix() {
        let material = VirtualKeyMaterial::generate().expect("key");

        assert!(material.raw_key.starts_with(RELAYNA_KEY_PREFIX));
        assert_eq!(material.key_prefix, &material.raw_key[..LOOKUP_PREFIX_LEN]);
        assert_ne!(material.raw_key, material.key_hash);
        verify_secret(&material.raw_key, &material.key_hash).expect("hash verifies");
    }

    #[test]
    fn rejects_malformed_raw_key_material() {
        assert_eq!(
            VirtualKeyMaterial::from_raw("sk_live_bad".to_owned()).unwrap_err(),
            GatewayError::MalformedAuthorization
        );
    }
}
