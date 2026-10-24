use crate::errors::{GatewayError, GatewayResult};
use argon2::{
    password_hash::{PasswordHash, PasswordVerifier},
    Argon2,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::sync::Arc;
use tokio::sync::Semaphore;
use uuid::Uuid;

const RELAYNA_KEY_PREFIX: &str = "rk_live_";
const LOOKUP_PREFIX_LEN: usize = 16;
// Argon2 is CPU/memory intensive. Share this bound across both Gateway
// runtimes, independently of the number of outstanding authentication futures.
static KEY_VERIFICATION_SLOTS: Semaphore = Semaphore::const_new(2);

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
        self.authenticate_virtual_key(key, now).await
    }

    pub async fn authenticate_raw_key(
        &self,
        raw_key: Option<&str>,
        now: DateTime<Utc>,
    ) -> GatewayResult<AuthenticatedKey> {
        let raw_key = raw_key.ok_or(GatewayError::MissingAuthorization)?;
        let key = VirtualKey::parse(raw_key.trim())?;
        self.authenticate_virtual_key(key, now).await
    }

    async fn authenticate_virtual_key(
        &self,
        key: VirtualKey,
        now: DateTime<Utc>,
    ) -> GatewayResult<AuthenticatedKey> {
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

        let raw_key = key.raw().to_owned();
        let key_hash = stored.key_hash.clone();
        verify_in_background(&KEY_VERIFICATION_SLOTS, move || {
            verify_secret(&raw_key, &key_hash)
        })
        .await?;
        let _ = self.store.mark_key_used(stored.id, now).await;

        Ok(AuthenticatedKey {
            key_id: stored.id,
            project_id: stored.project_id,
            key_prefix: stored.key_prefix,
        })
    }
}

async fn verify_in_background(
    slots: &'static Semaphore,
    verify: impl FnOnce() -> GatewayResult<()> + Send + 'static,
) -> GatewayResult<()> {
    let permit = slots.acquire().await.expect("private semaphore stays open");
    tokio::task::spawn_blocking(move || {
        // Cancellation cannot stop an executing CPU job. Keep its slot until
        // verification actually finishes, even if the request is dropped.
        let _permit = permit;
        verify()
    })
    .await
    .unwrap_or(Err(GatewayError::InvalidVirtualKey))
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
    async fn authenticates_raw_header_key() {
        let raw = "rk_live_1234567890abcdef";
        let lookup = MemoryLookup {
            value: Arc::new(Mutex::new(Some(stored(raw)))),
        };

        let authenticated = Authenticator::new(lookup)
            .authenticate_raw_key(Some(raw), Utc::now())
            .await
            .expect("authenticated");

        assert_eq!(authenticated.key_prefix, "rk_live_12345678");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn key_verification_leaves_the_request_executor_responsive() {
        let raw = "rk_live_1234567890abcdef";
        let lookup = MemoryLookup {
            value: Arc::new(Mutex::new(Some(stored(raw)))),
        };
        let authenticator = Authenticator::new(lookup);
        let requests = async {
            for _ in 0..4 {
                authenticator
                    .authenticate_raw_key(Some(raw), Utc::now())
                    .await
                    .expect("authenticated");
            }
        };
        tokio::pin!(requests);

        // This must progress on the same single-thread executor while the
        // first authentication performs real, unchanged Argon2 verification.
        let other_io = async {
            for _ in 0..3 {
                tokio::task::yield_now().await;
            }
        };
        tokio::select! {
            biased;
            () = &mut requests => panic!("key verification blocked unrelated I/O"),
            () = other_io => {}
        }
        requests.await;
    }

    #[tokio::test]
    async fn rejects_wrong_key_secret() {
        let lookup = MemoryLookup {
            value: Arc::new(Mutex::new(Some(stored("rk_live_1234567890abcdef")))),
        };
        assert_eq!(
            Authenticator::new(lookup)
                .authenticate_raw_key(Some("rk_live_1234567890abcdeg"), Utc::now())
                .await
                .unwrap_err(),
            GatewayError::InvalidVirtualKey
        );
    }

    #[tokio::test]
    async fn verification_workers_are_bounded() {
        static SLOTS: Semaphore = Semaphore::const_new(2);
        let (started_tx, mut started_rx) = tokio::sync::mpsc::unbounded_channel();
        let mut jobs = Vec::new();
        let mut releases = Vec::new();
        for index in 0..4 {
            let (release_tx, release_rx) = std::sync::mpsc::channel();
            releases.push(release_tx);
            let started = started_tx.clone();
            jobs.push(tokio::spawn(verify_in_background(&SLOTS, move || {
                started.send(index).unwrap();
                release_rx.recv().unwrap();
                Ok(())
            })));
        }
        let first = started_rx.recv().await.unwrap();
        let second = started_rx.recv().await.unwrap();
        assert_ne!(first, second);
        tokio::task::yield_now().await;
        assert!(started_rx.try_recv().is_err());
        assert_eq!(SLOTS.available_permits(), 0);

        for release in releases {
            release.send(()).unwrap();
        }
        for job in jobs {
            job.await.unwrap().unwrap();
        }
        assert_eq!(SLOTS.available_permits(), 2);
    }

    #[tokio::test]
    async fn cancelling_started_verification_keeps_its_worker_slot() {
        static SLOTS: Semaphore = Semaphore::const_new(1);
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let job = tokio::spawn(verify_in_background(&SLOTS, move || {
            started_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            Ok(())
        }));
        started_rx.await.unwrap();
        job.abort();
        assert!(job.await.unwrap_err().is_cancelled());
        assert_eq!(SLOTS.available_permits(), 0);

        let (next_tx, mut next_rx) = tokio::sync::oneshot::channel();
        let next = tokio::spawn(verify_in_background(&SLOTS, move || {
            next_tx.send(()).unwrap();
            Ok(())
        }));
        tokio::task::yield_now().await;
        assert!(matches!(
            next_rx.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty)
        ));
        release_tx.send(()).unwrap();
        next_rx.await.unwrap();
        next.await.unwrap().unwrap();
        assert_eq!(SLOTS.available_permits(), 1);
    }

    #[tokio::test]
    async fn cancelling_queued_verification_does_not_start_cpu_work() {
        use std::sync::atomic::{AtomicBool, Ordering};

        static SLOTS: Semaphore = Semaphore::const_new(1);
        let held = SLOTS.acquire().await.unwrap();
        let ran = Arc::new(AtomicBool::new(false));
        let observed = ran.clone();
        let job = tokio::spawn(verify_in_background(&SLOTS, move || {
            observed.store(true, Ordering::SeqCst);
            Ok(())
        }));
        tokio::task::yield_now().await;
        job.abort();
        assert!(job.await.unwrap_err().is_cancelled());
        drop(held);
        tokio::task::yield_now().await;
        assert!(!ran.load(Ordering::SeqCst));
        assert_eq!(SLOTS.available_permits(), 1);
    }

    #[tokio::test]
    async fn verification_worker_failure_is_closed_and_releases_its_slot() {
        static SLOTS: Semaphore = Semaphore::const_new(1);
        assert_eq!(
            verify_in_background(&SLOTS, || panic!("simulated verifier failure")).await,
            Err(GatewayError::InvalidVirtualKey)
        );
        assert_eq!(SLOTS.available_permits(), 1);
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
