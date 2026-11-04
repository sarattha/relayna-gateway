use crate::errors::{GatewayError, GatewayResult};
use argon2::{
    password_hash::{PasswordHash, PasswordVerifier},
    Argon2,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::{
    sync::Arc,
    time::{Duration as StdDuration, Instant},
};
use tokio::sync::Semaphore;
use uuid::Uuid;
use zeroize::Zeroizing;

#[path = "verification_cache.rs"]
mod verification_cache;

const RELAYNA_KEY_PREFIX: &str = "rk_live_";
const LOOKUP_PREFIX_LEN: usize = 16;
// Argon2 is CPU/memory intensive. Share this bound across both Gateway
// runtimes, independently of the number of outstanding authentication futures.
static KEY_VERIFICATION_SLOTS: Semaphore = Semaphore::const_new(2);
static KEY_VERIFICATION_WAITERS: Semaphore = Semaphore::const_new(8);
static AUTH_LOOKUP_SLOTS: Semaphore = Semaphore::const_new(64);
const VERIFICATION_WAIT: StdDuration = StdDuration::from_millis(250);

/// Covers store work even for callers outside the HTTP adapters.
pub fn try_auth_lookup() -> GatewayResult<tokio::sync::SemaphorePermit<'static>> {
    AUTH_LOOKUP_SLOTS
        .try_acquire()
        .map_err(|_| GatewayError::AuthenticationOverloaded)
}

/// All runtime key hashing/verification shares this executor across runtimes.
pub async fn run_key_crypto<T: Send + 'static>(
    work: impl FnOnce() -> GatewayResult<T> + Send + 'static,
) -> GatewayResult<T> {
    verify_in_background(&KEY_VERIFICATION_SLOTS, &KEY_VERIFICATION_WAITERS, work).await
}

pub async fn run_verification_cache_maintenance() {
    let mut interval = tokio::time::interval(StdDuration::from_secs(5));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        interval.tick().await;
        verification_cache::purge_expired();
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct VirtualKey {
    raw: Zeroizing<String>,
    prefix: String,
}

impl std::fmt::Debug for VirtualKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VirtualKey")
            .field("raw", &"[REDACTED]")
            .field("prefix", &self.prefix)
            .finish()
    }
}

impl VirtualKey {
    pub fn parse(raw: impl Into<String>) -> GatewayResult<Self> {
        let raw = Zeroizing::new(raw.into());
        crate::auth_admission::validate_key_text(&raw)?;
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
    lookup_slots: &'static Semaphore,
}

impl<S> Authenticator<S>
where
    S: VirtualKeyLookup,
{
    pub fn new(store: S) -> Self {
        Self {
            store,
            lookup_slots: &AUTH_LOOKUP_SLOTS,
        }
    }

    pub async fn authenticate_authorization(
        &self,
        authorization: Option<&str>,
        now: DateTime<Utc>,
    ) -> GatewayResult<AuthenticatedKey> {
        let authorization = authorization.ok_or(GatewayError::MissingAuthorization)?;
        if authorization.len() > crate::auth_admission::MAX_KEY_BYTES + 16 {
            return Err(GatewayError::MalformedAuthorization);
        }
        let key = VirtualKey::from_authorization(authorization)?;
        self.authenticate_virtual_key(key, now).await
    }

    pub async fn authenticate_raw_key(
        &self,
        raw_key: Option<&str>,
        now: DateTime<Utc>,
    ) -> GatewayResult<AuthenticatedKey> {
        let raw_key = raw_key.ok_or(GatewayError::MissingAuthorization)?;
        if raw_key.len() > crate::auth_admission::MAX_KEY_BYTES + 16 {
            return Err(GatewayError::MalformedAuthorization);
        }
        let key = VirtualKey::parse(raw_key.trim())?;
        self.authenticate_virtual_key(key, now).await
    }

    async fn authenticate_virtual_key(
        &self,
        key: VirtualKey,
        now: DateTime<Utc>,
    ) -> GatewayResult<AuthenticatedKey> {
        let _lookup = self
            .lookup_slots
            .try_acquire()
            .map_err(|_| GatewayError::AuthenticationOverloaded)?;
        let started = Instant::now();
        let current_time =
            || now + chrono::Duration::from_std(started.elapsed()).unwrap_or_default();
        let mut stored = self
            .store
            .find_by_prefix(key.prefix())
            .await?
            .ok_or(GatewayError::InvalidVirtualKey)?;
        validate_key_state(&stored, current_time())?;

        let proof = verification_cache::proof(stored.id, key.raw(), &stored.key_hash);
        if !verification_cache::contains(proof) {
            let raw_key = key.raw.clone();
            let key_hash = Zeroizing::new(stored.key_hash.clone());
            run_key_crypto(move || {
                if !verification_cache::contains(proof) {
                    verify_secret(&raw_key, &key_hash)?;
                    verification_cache::insert(proof);
                }
                Ok(())
            })
            .await?;
            let refreshed = self
                .store
                .find_by_prefix(key.prefix())
                .await?
                .ok_or(GatewayError::InvalidVirtualKey)?;
            validate_key_state(&refreshed, current_time())?;
            if refreshed.id != stored.id || refreshed.key_hash != stored.key_hash {
                return Err(GatewayError::InvalidVirtualKey);
            }
            stored = refreshed;
        }
        let _ = self.store.mark_key_used(stored.id, current_time()).await;

        Ok(AuthenticatedKey {
            key_id: stored.id,
            project_id: stored.project_id,
            key_prefix: stored.key_prefix,
        })
    }
}

fn validate_key_state(stored: &StoredVirtualKey, now: DateTime<Utc>) -> GatewayResult<()> {
    if stored.revoked_at.is_some() {
        return Err(GatewayError::RevokedVirtualKey);
    }
    if stored.disabled {
        return Err(GatewayError::DisabledVirtualKey);
    }
    if stored.expires_at.is_some_and(|expiry| expiry <= now) {
        return Err(GatewayError::ExpiredVirtualKey);
    }
    Ok(())
}

async fn verify_in_background<T: Send + 'static>(
    slots: &'static Semaphore,
    waiters: &'static Semaphore,
    verify: impl FnOnce() -> GatewayResult<T> + Send + 'static,
) -> GatewayResult<T> {
    let permit = match slots.try_acquire() {
        Ok(permit) => permit,
        Err(_) => {
            let _waiting = waiters
                .try_acquire()
                .map_err(|_| GatewayError::AuthenticationOverloaded)?;
            tokio::time::timeout(VERIFICATION_WAIT, slots.acquire())
                .await
                .map_err(|_| GatewayError::AuthenticationOverloaded)?
                .map_err(|_| GatewayError::AuthenticationOverloaded)?
        }
    };
    // JoinSet aborts jobs that have not started when the request is cancelled.
    // A started blocking job cannot be aborted and keeps its captured permit.
    let mut jobs = tokio::task::JoinSet::new();
    jobs.spawn_blocking(move || {
        // Dropping the request cannot stop started CPU work: keep its permit.
        let _permit = permit;
        verify()
    });
    jobs.join_next()
        .await
        .expect("one verification job")
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

    static AUTH_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    #[test]
    fn cancellation_aborts_crypto_queued_in_the_blocking_pool() {
        use std::sync::atomic::{AtomicBool, Ordering};
        static SLOTS: Semaphore = Semaphore::const_new(1);
        static WAITERS: Semaphore = Semaphore::const_new(8);
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .max_blocking_threads(1)
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let (started_tx, started_rx) = tokio::sync::oneshot::channel();
            let (release_tx, release_rx) = std::sync::mpsc::channel();
            let blocker = tokio::task::spawn_blocking(move || {
                started_tx.send(()).unwrap();
                release_rx.recv().unwrap();
            });
            started_rx.await.unwrap();
            let ran = Arc::new(AtomicBool::new(false));
            let observed = ran.clone();
            let request = tokio::spawn(verify_in_background(&SLOTS, &WAITERS, move || {
                observed.store(true, Ordering::SeqCst);
                Ok(())
            }));
            while SLOTS.available_permits() != 0 {
                tokio::task::yield_now().await;
            }
            request.abort();
            assert!(request.await.unwrap_err().is_cancelled());
            release_tx.send(()).unwrap();
            blocker.await.unwrap();
            let _released = SLOTS.acquire().await.unwrap();
            assert!(!ran.load(Ordering::SeqCst));
        });
    }

    #[tokio::test]
    async fn operator_verification_shares_capacity_and_preserves_overload_error() {
        let _auth_test = AUTH_TEST_LOCK.lock().await;
        let held = KEY_VERIFICATION_SLOTS.acquire_many(2).await.unwrap();
        let token = crate::operators::StoredOperatorToken {
            id: Uuid::new_v4(),
            token_prefix: "op_live_12345678".into(),
            token_hash: "not-reached".into(),
            roles: vec![],
            scopes: vec![],
            disabled: false,
            revoked_at: None,
        };
        assert_eq!(
            crate::operators::verify_stored_operator_token("op_live_1234567890", &token).await,
            Err(GatewayError::AuthenticationOverloaded)
        );
        assert_eq!(KEY_VERIFICATION_WAITERS.available_permits(), 8);
        drop(held);
    }

    #[tokio::test]
    async fn rejects_capacity_and_malformed_keys_before_store_work() {
        let _auth_test = AUTH_TEST_LOCK.lock().await;
        struct NoLookup;
        #[async_trait]
        impl VirtualKeyLookup for NoLookup {
            async fn find_by_prefix(&self, _: &str) -> GatewayResult<Option<StoredVirtualKey>> {
                panic!("admission must precede store work");
            }
        }
        static SLOTS: Semaphore = Semaphore::const_new(1);
        let held = SLOTS.acquire().await.unwrap();
        let auth = Authenticator {
            store: NoLookup,
            lookup_slots: &SLOTS,
        };
        assert_eq!(
            auth.authenticate_raw_key(Some("rk_live_1234567890"), Utc::now())
                .await,
            Err(GatewayError::AuthenticationOverloaded)
        );
        drop(held);
        for raw in [
            "rk_live_1234,567890".to_owned(),
            "rk_live_1234 567890".into(),
            format!("rk_live_{}", "x".repeat(1024)),
        ] {
            assert_eq!(
                auth.authenticate_raw_key(Some(&raw), Utc::now()).await,
                Err(GatewayError::MalformedAuthorization)
            );
        }
        let key = VirtualKey::parse("rk_live_1234567890-secret").unwrap();
        assert!(!format!("{key:?}").contains("secret"));
    }

    #[tokio::test]
    async fn verifier_waiting_queue_is_finite_and_deadline_releases_it() {
        static SLOTS: Semaphore = Semaphore::const_new(2);
        static WAITERS: Semaphore = Semaphore::const_new(8);
        let held = SLOTS.acquire_many(2).await.unwrap();
        let mut jobs = Vec::new();
        for _ in 0..8 {
            jobs.push(tokio::spawn(verify_in_background(
                &SLOTS,
                &WAITERS,
                || -> GatewayResult<()> { panic!("timed out work must not run") },
            )));
        }
        while WAITERS.available_permits() != 0 {
            tokio::task::yield_now().await;
        }
        assert_eq!(
            verify_in_background(&SLOTS, &WAITERS, || Ok(())).await,
            Err(GatewayError::AuthenticationOverloaded)
        );
        for job in jobs {
            assert_eq!(
                job.await.unwrap(),
                Err(GatewayError::AuthenticationOverloaded)
            );
        }
        assert_eq!(WAITERS.available_permits(), 8);
        drop(held);
        verify_in_background(&SLOTS, &WAITERS, || Ok(()))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn cold_verification_refreshes_state_before_returning_identity() {
        let _auth_test = AUTH_TEST_LOCK.lock().await;
        struct ChangingLookup {
            initial: StoredVirtualKey,
            refreshed: Option<StoredVirtualKey>,
            calls: std::sync::atomic::AtomicUsize,
        }
        #[async_trait]
        impl VirtualKeyLookup for ChangingLookup {
            async fn find_by_prefix(&self, _: &str) -> GatewayResult<Option<StoredVirtualKey>> {
                if self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
                    Ok(Some(self.initial.clone()))
                } else {
                    Ok(self.refreshed.clone())
                }
            }
        }
        for change in 0..7 {
            let raw = format!("rk_live_{}", Uuid::new_v4().simple());
            let initial = stored(&raw);
            let mut refreshed = initial.clone();
            let now = Utc::now();
            let expected = match change {
                0 => {
                    refreshed.disabled = true;
                    Some(GatewayError::DisabledVirtualKey)
                }
                1 => {
                    refreshed.revoked_at = Some(now);
                    Some(GatewayError::RevokedVirtualKey)
                }
                2 => {
                    refreshed.expires_at = Some(now);
                    Some(GatewayError::ExpiredVirtualKey)
                }
                3 => {
                    refreshed.key_hash.push('x');
                    Some(GatewayError::InvalidVirtualKey)
                }
                4 => {
                    refreshed.id = Uuid::new_v4();
                    Some(GatewayError::InvalidVirtualKey)
                }
                5 => Some(GatewayError::InvalidVirtualKey),
                _ => {
                    refreshed.project_id = Some(Uuid::new_v4());
                    None
                }
            };
            let expected_project = refreshed.project_id;
            let auth = Authenticator::new(ChangingLookup {
                initial,
                refreshed: (change != 5).then_some(refreshed),
                calls: std::sync::atomic::AtomicUsize::new(0),
            });
            let result = auth.authenticate_raw_key(Some(&raw), now).await;
            if let Some(error) = expected {
                assert_eq!(result, Err(error));
            } else {
                assert_eq!(result.unwrap().project_id, expected_project);
            }
        }
    }

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
        let _auth_test = AUTH_TEST_LOCK.lock().await;
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
        let _auth_test = AUTH_TEST_LOCK.lock().await;
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
        let _auth_test = AUTH_TEST_LOCK.lock().await;
        let raw = format!("{RELAYNA_KEY_PREFIX}{}", Uuid::new_v4().simple());
        let lookup = MemoryLookup {
            value: Arc::new(Mutex::new(Some(stored(&raw)))),
        };
        let authenticator = Authenticator::new(lookup);
        let requests = async {
            for _ in 0..4 {
                authenticator
                    .authenticate_raw_key(Some(&raw), Utc::now())
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
        let _auth_test = AUTH_TEST_LOCK.lock().await;
        let raw = format!("{RELAYNA_KEY_PREFIX}{}", Uuid::new_v4().simple());
        let wrong_secret = format!("{raw}_wrong");
        let lookup = MemoryLookup {
            value: Arc::new(Mutex::new(Some(stored(&raw)))),
        };
        assert_eq!(
            Authenticator::new(lookup)
                .authenticate_raw_key(Some(&wrong_secret), Utc::now())
                .await
                .unwrap_err(),
            GatewayError::InvalidVirtualKey
        );
    }

    #[tokio::test]
    async fn verified_secret_hit_skips_workers_but_refreshes_key_state() {
        let _auth_test = AUTH_TEST_LOCK.lock().await;
        let raw = format!("{RELAYNA_KEY_PREFIX}{}", Uuid::new_v4().simple());
        let value = stored(&raw);
        let lookup = MemoryLookup {
            value: Arc::new(Mutex::new(Some(value.clone()))),
        };
        let authenticator = Authenticator::new(lookup.clone());
        authenticator
            .authenticate_raw_key(Some(&raw), Utc::now())
            .await
            .unwrap();
        let held = KEY_VERIFICATION_SLOTS.acquire_many(2).await.unwrap();
        let changed_project = Some(Uuid::new_v4());
        lookup.value.lock().unwrap().as_mut().unwrap().project_id = changed_project;
        let hit = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            authenticator.authenticate_raw_key(Some(&raw), Utc::now()),
        )
        .await;
        drop(held);
        assert_eq!(hit.unwrap().unwrap().project_id, changed_project);

        for (disabled, revoked, expired, expected) in [
            (true, false, false, GatewayError::DisabledVirtualKey),
            (false, true, false, GatewayError::RevokedVirtualKey),
            (false, false, true, GatewayError::ExpiredVirtualKey),
        ] {
            let mut changed = value.clone();
            changed.disabled = disabled;
            changed.revoked_at = revoked.then(Utc::now);
            changed.expires_at = expired.then(|| Utc::now() - Duration::seconds(1));
            *lookup.value.lock().unwrap() = Some(changed);
            assert_eq!(
                authenticator
                    .authenticate_raw_key(Some(&raw), Utc::now())
                    .await,
                Err(expected)
            );
        }
        *lookup.value.lock().unwrap() = None;
        assert_eq!(
            authenticator
                .authenticate_raw_key(Some(&raw), Utc::now())
                .await,
            Err(GatewayError::InvalidVirtualKey)
        );

        let mut rotated = value;
        let new_raw = format!("{raw}_rotated");
        rotated.key_hash = hash(&new_raw);
        *lookup.value.lock().unwrap() = Some(rotated);
        assert_eq!(
            authenticator
                .authenticate_raw_key(Some(&raw), Utc::now())
                .await,
            Err(GatewayError::InvalidVirtualKey)
        );
        authenticator
            .authenticate_raw_key(Some(&new_raw), Utc::now())
            .await
            .unwrap();
        assert_eq!(
            authenticator
                .authenticate_raw_key(Some(&format!("{new_raw}_wrong")), Utc::now())
                .await,
            Err(GatewayError::InvalidVirtualKey)
        );
    }

    #[tokio::test]
    async fn verified_secret_hit_still_marks_usage_and_fails_closed_on_store_error() {
        let _auth_test = AUTH_TEST_LOCK.lock().await;
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
        struct ObservedLookup {
            key: StoredVirtualKey,
            unavailable: AtomicBool,
            used: AtomicUsize,
        }
        #[async_trait]
        impl VirtualKeyLookup for ObservedLookup {
            async fn find_by_prefix(&self, _: &str) -> GatewayResult<Option<StoredVirtualKey>> {
                if self.unavailable.load(Ordering::SeqCst) {
                    Err(GatewayError::StoreUnavailable)
                } else {
                    Ok(Some(self.key.clone()))
                }
            }
            async fn mark_key_used(&self, _: Uuid, _: DateTime<Utc>) -> GatewayResult<()> {
                self.used.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        }
        let raw = format!("{RELAYNA_KEY_PREFIX}{}", Uuid::new_v4().simple());
        let lookup = Arc::new(ObservedLookup {
            key: stored(&raw),
            unavailable: AtomicBool::new(false),
            used: AtomicUsize::new(0),
        });
        let authenticator = Authenticator::new(lookup.clone());
        for _ in 0..2 {
            authenticator
                .authenticate_raw_key(Some(&raw), Utc::now())
                .await
                .unwrap();
        }
        assert_eq!(lookup.used.load(Ordering::SeqCst), 2);
        lookup.unavailable.store(true, Ordering::SeqCst);
        assert_eq!(
            authenticator
                .authenticate_raw_key(Some(&raw), Utc::now())
                .await,
            Err(GatewayError::StoreUnavailable)
        );
        assert_eq!(lookup.used.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn verification_workers_are_bounded() {
        static SLOTS: Semaphore = Semaphore::const_new(2);
        static WAITERS: Semaphore = Semaphore::const_new(8);
        let (started_tx, mut started_rx) = tokio::sync::mpsc::unbounded_channel();
        let mut jobs = Vec::new();
        let mut releases = Vec::new();
        for index in 0..4 {
            let (release_tx, release_rx) = std::sync::mpsc::channel();
            releases.push(release_tx);
            let started = started_tx.clone();
            jobs.push(tokio::spawn(verify_in_background(
                &SLOTS,
                &WAITERS,
                move || {
                    started.send(index).unwrap();
                    release_rx.recv().unwrap();
                    Ok(())
                },
            )));
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
        static WAITERS: Semaphore = Semaphore::const_new(8);
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let job = tokio::spawn(verify_in_background(&SLOTS, &WAITERS, move || {
            started_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            Ok(())
        }));
        started_rx.await.unwrap();
        job.abort();
        assert!(job.await.unwrap_err().is_cancelled());
        assert_eq!(SLOTS.available_permits(), 0);

        let (next_tx, mut next_rx) = tokio::sync::oneshot::channel();
        let next = tokio::spawn(verify_in_background(&SLOTS, &WAITERS, move || {
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
        static WAITERS: Semaphore = Semaphore::const_new(8);
        let held = SLOTS.acquire().await.unwrap();
        let ran = Arc::new(AtomicBool::new(false));
        let observed = ran.clone();
        let job = tokio::spawn(verify_in_background(&SLOTS, &WAITERS, move || {
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
        static WAITERS: Semaphore = Semaphore::const_new(8);
        assert_eq!(
            verify_in_background::<()>(&SLOTS, &WAITERS, || panic!("simulated verifier failure"))
                .await,
            Err(GatewayError::InvalidVirtualKey)
        );
        assert_eq!(SLOTS.available_permits(), 1);
    }

    #[tokio::test]
    async fn rejects_disabled_key() {
        let _auth_test = AUTH_TEST_LOCK.lock().await;
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
        let _auth_test = AUTH_TEST_LOCK.lock().await;
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
        let _auth_test = AUTH_TEST_LOCK.lock().await;
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
