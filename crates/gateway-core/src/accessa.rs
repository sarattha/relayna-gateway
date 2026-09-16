//! Internal admission context: trusted gateway identity, never a client-supplied user ID.
use crate::{EndpointAccess, EntraIdentityContext, GatewayError, GatewayResult};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

pub const ADMISSION_CONTEXT_HEADER: &str = "x-relayna-admission-context";
pub const ADMISSION_PREFIX: &str = "/internal/accessa/admissions/";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessaSession {
    pub key_id: Uuid,
    pub key_prefix: String,
    pub service_name: String,
    pub identity: EntraIdentityContext,
    pub access: EndpointAccess,
    pub expires_at: i64,
}

pub fn session_store_key(token: &str) -> GatewayResult<String> {
    Uuid::parse_str(token).map_err(|_| GatewayError::InvalidVirtualKey)?;
    Ok(format!(
        "accessa:session:{:x}",
        Sha256::digest(token.as_bytes())
    ))
}

/// Redis-backed ephemeral state, shared between replicas. Implementations fail closed.
#[async_trait]
pub trait AccessaStore: Send + Sync {
    async fn accessa_get(&self, key: &str) -> GatewayResult<Option<String>>;
    async fn accessa_put(
        &self,
        key: &str,
        value: &str,
        ttl: u64,
        only_if_absent: bool,
    ) -> GatewayResult<bool>;
    async fn accessa_delete(&self, key: &str) -> GatewayResult<()>;
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn context_keys_are_hashed_and_reject_invalid_tokens() {
        let token = Uuid::new_v4().to_string();
        let key = session_store_key(&token).unwrap();
        assert!(key.starts_with("accessa:session:"));
        assert!(!key.contains(&token));
        assert_eq!(key, session_store_key(&token).unwrap());
        assert!(session_store_key("bad").is_err());
    }
}
