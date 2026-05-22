use crate::{
    auth::verify_secret,
    errors::{GatewayError, GatewayResult},
};
use argon2::{
    password_hash::{rand_core::OsRng, PasswordHasher, SaltString},
    Argon2,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

const OPERATOR_TOKEN_PREFIX: &str = "op_live_";
const LOOKUP_PREFIX_LEN: usize = 16;
pub const OPERATOR_SCOPE_ALL: &str = "*";
pub const OPERATOR_ROLE_OWNER: &str = "owner";
pub const SCOPE_KEYS_CREATE: &str = "keys:create";
pub const SCOPE_KEYS_DISABLE: &str = "keys:disable";
pub const SCOPE_KEYS_ROTATE: &str = "keys:rotate";
pub const SCOPE_POLICIES_UPDATE: &str = "policies:update";
pub const SCOPE_GUARDRAILS_UPDATE: &str = "guardrails:update";
pub const SCOPE_USAGE_READ: &str = "usage:read";
pub const SCOPE_USAGE_EXPORT: &str = "usage:export";
pub const SCOPE_PROVIDERS_UPDATE: &str = "providers:update";
pub const SCOPE_SERVICES_UPDATE: &str = "services:update";
pub const SCOPE_SETTINGS_UPDATE: &str = "settings:update";
pub const SCOPE_OPERATORS_MANAGE: &str = "operators:manage";
pub const SCOPE_AUDIT_READ: &str = "audit:read";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperatorTokenMaterial {
    pub raw_token: String,
    pub token_prefix: String,
    pub token_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredOperatorToken {
    pub id: Uuid,
    pub token_prefix: String,
    pub token_hash: String,
    pub roles: Vec<String>,
    pub scopes: Vec<String>,
    pub disabled: bool,
    pub revoked_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct OperatorTokenResponse {
    pub id: Uuid,
    pub token_prefix: String,
    pub roles: Vec<String>,
    pub scopes: Vec<String>,
    pub disabled: bool,
    pub revoked_at: Option<DateTime<Utc>>,
    pub last_used_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CreatedOperatorTokenResponse {
    pub token: OperatorTokenResponse,
    pub raw_token: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct OperatorAuthorization {
    pub token_id: Uuid,
    pub token_prefix: String,
    pub roles: Vec<String>,
    pub scopes: Vec<String>,
}

impl OperatorAuthorization {
    pub fn has_scope(&self, required_scope: &str) -> bool {
        self.scopes
            .iter()
            .any(|scope| scope == OPERATOR_SCOPE_ALL || scope == required_scope)
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct AuditEvent {
    pub id: Uuid,
    pub actor_token_id: Uuid,
    pub action: String,
    pub target_type: String,
    pub target_id: Option<String>,
    pub before: Option<serde_json::Value>,
    pub after: Option<serde_json::Value>,
    pub request_id: String,
    pub ip: Option<String>,
    pub user_agent: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AuditEventCreate {
    pub actor_token_id: Uuid,
    pub action: String,
    pub target_type: String,
    pub target_id: Option<String>,
    pub before: Option<serde_json::Value>,
    pub after: Option<serde_json::Value>,
    pub request_id: String,
    pub ip: Option<String>,
    pub user_agent: Option<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct AuditEventQuery {
    #[serde(default)]
    pub actor_token_id: Option<Uuid>,
    #[serde(default)]
    pub action: Option<String>,
    #[serde(default)]
    pub target_type: Option<String>,
    #[serde(default)]
    pub target_id: Option<String>,
    #[serde(default = "default_audit_limit")]
    pub limit: i64,
}

fn default_audit_limit() -> i64 {
    100
}

#[async_trait]
pub trait OperatorTokenStore: Send + Sync {
    async fn bootstrap_operator_token(
        &self,
        material: &OperatorTokenMaterial,
    ) -> GatewayResult<Option<OperatorTokenResponse>>;

    async fn verify_operator_token(
        &self,
        raw_token: &str,
        now: DateTime<Utc>,
    ) -> GatewayResult<OperatorAuthorization>;

    async fn rotate_operator_token(
        &self,
        current_raw_token: &str,
        material: &OperatorTokenMaterial,
        now: DateTime<Utc>,
    ) -> GatewayResult<OperatorTokenResponse>;
}

#[async_trait]
pub trait AdminAuditStore: Send + Sync {
    async fn record_audit_event(&self, event: AuditEventCreate) -> GatewayResult<AuditEvent>;

    async fn list_audit_events(&self, query: AuditEventQuery) -> GatewayResult<Vec<AuditEvent>>;
}

#[async_trait]
impl<T> OperatorTokenStore for std::sync::Arc<T>
where
    T: OperatorTokenStore + ?Sized,
{
    async fn bootstrap_operator_token(
        &self,
        material: &OperatorTokenMaterial,
    ) -> GatewayResult<Option<OperatorTokenResponse>> {
        (**self).bootstrap_operator_token(material).await
    }

    async fn verify_operator_token(
        &self,
        raw_token: &str,
        now: DateTime<Utc>,
    ) -> GatewayResult<OperatorAuthorization> {
        (**self).verify_operator_token(raw_token, now).await
    }

    async fn rotate_operator_token(
        &self,
        current_raw_token: &str,
        material: &OperatorTokenMaterial,
        now: DateTime<Utc>,
    ) -> GatewayResult<OperatorTokenResponse> {
        (**self)
            .rotate_operator_token(current_raw_token, material, now)
            .await
    }
}

#[async_trait]
impl<T> AdminAuditStore for std::sync::Arc<T>
where
    T: AdminAuditStore + ?Sized,
{
    async fn record_audit_event(&self, event: AuditEventCreate) -> GatewayResult<AuditEvent> {
        (**self).record_audit_event(event).await
    }

    async fn list_audit_events(&self, query: AuditEventQuery) -> GatewayResult<Vec<AuditEvent>> {
        (**self).list_audit_events(query).await
    }
}

impl OperatorTokenMaterial {
    pub fn generate() -> GatewayResult<Self> {
        Self::from_raw(format!(
            "{OPERATOR_TOKEN_PREFIX}{}{}",
            Uuid::new_v4().simple(),
            Uuid::new_v4().simple()
        ))
    }

    pub fn from_raw(raw_token: String) -> GatewayResult<Self> {
        let token_prefix = operator_token_prefix(&raw_token)?;
        let salt = SaltString::generate(&mut OsRng);
        let token_hash = Argon2::default()
            .hash_password(raw_token.as_bytes(), &salt)
            .map_err(|_| GatewayError::InvalidConfiguration)?
            .to_string();

        Ok(Self {
            raw_token,
            token_prefix,
            token_hash,
        })
    }
}

pub fn operator_token_prefix(raw_token: &str) -> GatewayResult<String> {
    if !raw_token.starts_with(OPERATOR_TOKEN_PREFIX) || raw_token.len() <= LOOKUP_PREFIX_LEN {
        return Err(GatewayError::MalformedAuthorization);
    }
    Ok(raw_token.chars().take(LOOKUP_PREFIX_LEN).collect())
}

pub fn verify_stored_operator_token(
    raw_token: &str,
    stored: &StoredOperatorToken,
) -> GatewayResult<OperatorAuthorization> {
    if stored.disabled || stored.revoked_at.is_some() {
        return Err(GatewayError::DisabledOperatorToken);
    }
    verify_secret(raw_token, &stored.token_hash).map_err(|_| GatewayError::InvalidOperatorToken)?;
    Ok(OperatorAuthorization {
        token_id: stored.id,
        token_prefix: stored.token_prefix.clone(),
        roles: stored.roles.clone(),
        scopes: stored.scopes.clone(),
    })
}

pub fn default_operator_roles() -> Vec<String> {
    vec![OPERATOR_ROLE_OWNER.to_owned()]
}

pub fn default_operator_scopes() -> Vec<String> {
    vec![OPERATOR_SCOPE_ALL.to_owned()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_operator_token_is_hashed_and_prefixed() {
        let material = OperatorTokenMaterial::generate().expect("token");

        assert!(material.raw_token.starts_with(OPERATOR_TOKEN_PREFIX));
        assert_eq!(
            material.token_prefix,
            &material.raw_token[..LOOKUP_PREFIX_LEN]
        );
        assert_ne!(material.raw_token, material.token_hash);
    }

    #[test]
    fn rejects_malformed_operator_token() {
        assert_eq!(
            operator_token_prefix("not-valid").unwrap_err(),
            GatewayError::MalformedAuthorization
        );
    }
}
