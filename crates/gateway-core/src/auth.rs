use crate::errors::{GatewayError, GatewayResult};
use argon2::{
    password_hash::{PasswordHash, PasswordVerifier},
    Argon2,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::sync::Arc;
use uuid::Uuid;

const RELAYNA_KEY_PREFIX: &str = "rk_live_";
const LOOKUP_PREFIX_LEN: usize = 16;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VirtualKey {
    raw: String,
    prefix: String,
}

impl VirtualKey {
    pub fn parse(raw: impl Into<String>) -> GatewayResult<Self> {
        let raw = raw.into();
        if !raw.starts_with(RELAYNA_KEY_PREFIX) || raw.len() <= LOOKUP_PREFIX_LEN {
            return Err(GatewayError::MalformedAuthorization);
        }

        let prefix = raw.chars().take(LOOKUP_PREFIX_LEN).collect();
        Ok(Self { raw, prefix })
    }

    pub fn from_authorization(value: &str) -> GatewayResult<Self> {
        let Some(token) = value.strip_prefix("Bearer ") else {
            return Err(GatewayError::MalformedAuthorization);
        };
        Self::parse(token.trim())
    }

    pub fn prefix(&self) -> &str {
        &self.prefix
    }

    fn raw(&self) -> &str {
        &self.raw
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredVirtualKey {
    pub id: Uuid,
    pub project_id: Option<Uuid>,
    pub key_prefix: String,
    pub key_hash: String,
    pub disabled: bool,
    pub revoked_at: Option<DateTime<Utc>>,
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthenticatedKey {
    pub key_id: Uuid,
    pub project_id: Option<Uuid>,
    pub key_prefix: String,
}

#[async_trait]
pub trait VirtualKeyLookup: Send + Sync {
    async fn find_by_prefix(&self, prefix: &str) -> GatewayResult<Option<StoredVirtualKey>>;

    async fn mark_key_used(&self, _key_id: Uuid, _used_at: DateTime<Utc>) -> GatewayResult<()> {
        Ok(())
    }
}

#[async_trait]
impl<T> VirtualKeyLookup for Arc<T>
where
    T: VirtualKeyLookup + ?Sized,
{
    async fn find_by_prefix(&self, prefix: &str) -> GatewayResult<Option<StoredVirtualKey>> {
        (**self).find_by_prefix(prefix).await
    }

    async fn mark_key_used(&self, key_id: Uuid, used_at: DateTime<Utc>) -> GatewayResult<()> {
        (**self).mark_key_used(key_id, used_at).await
    }
}

#[derive(Debug, Clone)]
pub struct Authenticator<S> {
    store: S,
}

impl<S> Authenticator<S>
where
    S: VirtualKeyLookup,
{
    pub fn new(store: S) -> Self {
        Self { store }
    }

    pub async fn authenticate_authorization(
        &self,
        authorization: Option<&str>,
        now: DateTime<Utc>,
    ) -> GatewayResult<AuthenticatedKey> {
        let authorization = authorization.ok_or(GatewayError::MissingAuthorization)?;
        let key = VirtualKey::from_authorization(authorization)?;
        let stored = self
            .store
            .find_by_prefix(key.prefix())
            .await?
            .ok_or(GatewayError::InvalidVirtualKey)?;

        if stored.revoked_at.is_some() {
            return Err(GatewayError::RevokedVirtualKey);
        }

        if stored.disabled {
            return Err(GatewayError::DisabledVirtualKey);
        }

        if stored
            .expires_at
            .is_some_and(|expires_at| expires_at <= now)
        {
            return Err(GatewayError::ExpiredVirtualKey);
        }

        verify_secret(key.raw(), &stored.key_hash)?;
        let _ = self.store.mark_key_used(stored.id, now).await;

        Ok(AuthenticatedKey {
            key_id: stored.id,
            project_id: stored.project_id,
            key_prefix: stored.key_prefix,
        })
    }
}

pub fn verify_secret(raw_key: &str, encoded_hash: &str) -> GatewayResult<()> {
    let parsed_hash =
        PasswordHash::new(encoded_hash).map_err(|_| GatewayError::InvalidVirtualKey)?;
    Argon2::default()
        .verify_password(raw_key.as_bytes(), &parsed_hash)
        .map_err(|_| GatewayError::InvalidVirtualKey)
}

#[cfg(test)]
mod tests {
    use super::*;
    use argon2::{
        password_hash::{rand_core::OsRng, PasswordHasher, SaltString},
        Argon2,
    };
    use chrono::Duration;
    use std::sync::{Arc, Mutex};

    #[derive(Clone)]
    struct MemoryLookup {
        value: Arc<Mutex<Option<StoredVirtualKey>>>,
    }

    #[async_trait]
    impl VirtualKeyLookup for MemoryLookup {
        async fn find_by_prefix(&self, _prefix: &str) -> GatewayResult<Option<StoredVirtualKey>> {
            Ok(self.value.lock().expect("lock poisoned").clone())
        }
    }

    fn hash(raw: &str) -> String {
        let salt = SaltString::generate(&mut OsRng);
        Argon2::default()
            .hash_password(raw.as_bytes(), &salt)
            .expect("hash")
            .to_string()
    }

    fn stored(raw: &str) -> StoredVirtualKey {
        StoredVirtualKey {
            id: Uuid::new_v4(),
            project_id: Some(Uuid::new_v4()),
            key_prefix: raw.chars().take(LOOKUP_PREFIX_LEN).collect(),
            key_hash: hash(raw),
            disabled: false,
            revoked_at: None,
            expires_at: None,
        }
    }

    #[test]
    fn parses_virtual_key_prefix() {
        let key = VirtualKey::parse("rk_live_1234567890abcdef").expect("parse");
        assert_eq!(key.prefix(), "rk_live_12345678");
    }

    #[test]
    fn rejects_malformed_key() {
        assert_eq!(
            VirtualKey::parse("sk_live_123").unwrap_err(),
            GatewayError::MalformedAuthorization
        );
    }

    #[tokio::test]
    async fn authenticates_valid_key() {
        let raw = "rk_live_1234567890abcdef";
        let lookup = MemoryLookup {
            value: Arc::new(Mutex::new(Some(stored(raw)))),
        };

        let authenticated = Authenticator::new(lookup)
            .authenticate_authorization(Some(&format!("Bearer {raw}")), Utc::now())
            .await
            .expect("authenticated");

        assert_eq!(authenticated.key_prefix, "rk_live_12345678");
    }

    #[tokio::test]
    async fn rejects_disabled_key() {
        let raw = "rk_live_1234567890abcdef";
        let mut key = stored(raw);
        key.disabled = true;
        let lookup = MemoryLookup {
            value: Arc::new(Mutex::new(Some(key))),
        };

        let err = Authenticator::new(lookup)
            .authenticate_authorization(Some(&format!("Bearer {raw}")), Utc::now())
            .await
            .unwrap_err();

        assert_eq!(err, GatewayError::DisabledVirtualKey);
    }

    #[tokio::test]
    async fn rejects_expired_key() {
        let raw = "rk_live_1234567890abcdef";
        let mut key = stored(raw);
        key.expires_at = Some(Utc::now() - Duration::minutes(1));
        let lookup = MemoryLookup {
            value: Arc::new(Mutex::new(Some(key))),
        };

        let err = Authenticator::new(lookup)
            .authenticate_authorization(Some(&format!("Bearer {raw}")), Utc::now())
            .await
            .unwrap_err();

        assert_eq!(err, GatewayError::ExpiredVirtualKey);
    }

    #[tokio::test]
    async fn rejects_revoked_key_even_when_not_disabled() {
        let raw = "rk_live_1234567890abcdef";
        let mut key = stored(raw);
        key.disabled = false;
        key.revoked_at = Some(Utc::now());
        let lookup = MemoryLookup {
            value: Arc::new(Mutex::new(Some(key))),
        };

        let err = Authenticator::new(lookup)
            .authenticate_authorization(Some(&format!("Bearer {raw}")), Utc::now())
            .await
            .unwrap_err();

        assert_eq!(err, GatewayError::RevokedVirtualKey);
    }
}
