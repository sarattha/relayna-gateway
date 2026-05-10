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
use serde::Serialize;
use uuid::Uuid;

const OPERATOR_TOKEN_PREFIX: &str = "op_live_";
const LOOKUP_PREFIX_LEN: usize = 16;

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
    pub disabled: bool,
    pub revoked_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct OperatorTokenResponse {
    pub id: Uuid,
    pub token_prefix: String,
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

#[async_trait]
pub trait OperatorTokenStore: Send + Sync {
    async fn bootstrap_operator_token(
        &self,
        material: &OperatorTokenMaterial,
    ) -> GatewayResult<Option<OperatorTokenResponse>>;

    async fn verify_operator_token(&self, raw_token: &str, now: DateTime<Utc>)
        -> GatewayResult<()>;

    async fn rotate_operator_token(
        &self,
        current_raw_token: &str,
        material: &OperatorTokenMaterial,
        now: DateTime<Utc>,
    ) -> GatewayResult<OperatorTokenResponse>;
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
    ) -> GatewayResult<()> {
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
) -> GatewayResult<()> {
    if stored.disabled || stored.revoked_at.is_some() {
        return Err(GatewayError::DisabledOperatorToken);
    }
    verify_secret(raw_token, &stored.token_hash).map_err(|_| GatewayError::InvalidOperatorToken)
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
