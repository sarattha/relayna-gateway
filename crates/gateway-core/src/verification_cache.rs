//! Cache only successful secret verification, never key state or authorization.
//! Every caller must fetch and validate current database state before using it.
use hmac::{Hmac, Mac};
use rand_core::{OsRng, RngCore};
use sha2::Sha256;
use std::{
    collections::{HashMap, VecDeque},
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};
use uuid::Uuid;
use zeroize::{Zeroize, Zeroizing};

type Proof = [u8; 32];
const CAPACITY: usize = 1024;
const TTL: Duration = Duration::from_secs(60);
static CACHE: OnceLock<Option<VerificationCache>> = OnceLock::new();

struct VerificationCache {
    secret: Zeroizing<[u8; 32]>,
    entries: Mutex<Entries>,
}

#[derive(Default)]
struct Entries {
    proofs: HashMap<Proof, Instant>,
    order: VecDeque<Proof>,
}

impl Entries {
    fn remove_oldest(&mut self) {
        if let Some(mut oldest) = self.order.pop_front() {
            if let Some((mut key, _)) = self.proofs.remove_entry(&oldest) {
                key.zeroize();
            }
            oldest.zeroize();
        }
    }

    fn purge_expired(&mut self, now: Instant) {
        while self
            .order
            .front()
            .is_some_and(|proof| !self.contains(proof, now))
        {
            self.remove_oldest();
        }
    }
    fn contains(&self, proof: &Proof, now: Instant) -> bool {
        self.proofs
            .get(proof)
            .is_some_and(|verified| now.duration_since(*verified) < TTL)
    }

    fn insert(&mut self, proof: Proof, now: Instant) {
        if self.contains(&proof, now) {
            return;
        }
        // FIFO expiry bounds both memory and work. Hits do not extend proof age.
        while let Some(oldest) = self.order.front() {
            if self.proofs.len() < CAPACITY && self.contains(oldest, now) {
                break;
            }
            self.remove_oldest();
        }
        if let std::collections::hash_map::Entry::Vacant(entry) = self.proofs.entry(proof) {
            entry.insert(now);
            self.order.push_back(proof);
        }
    }
}

fn cache() -> Option<&'static VerificationCache> {
    CACHE
        .get_or_init(|| {
            let mut secret = Zeroizing::new([0; 32]);
            // Entropy failure disables this optimization; Argon2 still verifies.
            OsRng.try_fill_bytes(secret.as_mut()).ok()?;
            Some(VerificationCache {
                secret,
                entries: Mutex::new(Entries::default()),
            })
        })
        .as_ref()
}

pub(super) fn proof(id: Uuid, raw: &str, encoded_hash: &str) -> Option<Proof> {
    let cache = cache()?;
    let mut mac = Hmac::<Sha256>::new_from_slice(cache.secret.as_ref()).ok()?;
    mac.update(id.as_bytes());
    mac.update(&(encoded_hash.len() as u64).to_le_bytes());
    mac.update(encoded_hash.as_bytes());
    mac.update(raw.as_bytes());
    Some(mac.finalize().into_bytes().into())
}

pub(super) fn contains(proof: Option<Proof>) -> bool {
    let Some((cache, proof)) = cache().zip(proof) else {
        return false;
    };
    cache
        .entries
        .lock()
        .is_ok_and(|entries| entries.contains(&proof, Instant::now()))
}

pub(super) fn insert(proof: Option<Proof>) {
    if let Some((cache, proof)) = cache().zip(proof) {
        if let Ok(mut entries) = cache.entries.lock() {
            entries.insert(proof, Instant::now());
        }
    }
}

pub(super) fn purge_expired() {
    // Maintenance must not initialize the cache or create secrets on its own.
    if let Some(Some(cache)) = CACHE.get() {
        if let Ok(mut entries) = cache.entries.lock() {
            entries.purge_expired(Instant::now());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proofs_expire_without_refresh_on_hits_and_storage_is_bounded() {
        let now = Instant::now();
        let mut entries = Entries::default();
        entries.insert([0; 32], now);
        assert!(entries.contains(&[0; 32], now + TTL - Duration::from_nanos(1)));
        assert!(!entries.contains(&[0; 32], now + TTL));
        entries.insert([0; 32], now + TTL);
        assert!(entries.contains(&[0; 32], now + TTL));
        for index in 1..=CAPACITY {
            let mut key = [0; 32];
            key[..8].copy_from_slice(&(index as u64).to_le_bytes());
            entries.insert(key, now + TTL);
        }
        assert_eq!(entries.proofs.len(), CAPACITY);
        assert_eq!(entries.order.len(), CAPACITY);
        assert!(!entries.contains(&[0; 32], now + TTL));
    }

    #[test]
    fn proofs_bind_the_identity_secret_and_complete_stored_hash() {
        let id = Uuid::new_v4();
        let raw = Uuid::new_v4().to_string();
        let original = proof(id, &raw, "hash").unwrap();
        assert_ne!(Some(original), proof(Uuid::new_v4(), &raw, "hash"));
        assert_ne!(Some(original), proof(id, "different", "hash"));
        assert_ne!(Some(original), proof(id, &raw, "rotated hash"));
        assert_ne!(Some(original), proof(id, "hash", &raw));
    }

    #[test]
    fn maintenance_removes_idle_proofs_without_an_insertion() {
        let now = Instant::now();
        let mut entries = Entries::default();
        entries.insert([1; 32], now);
        entries.insert([2; 32], now + Duration::from_secs(1));
        entries.purge_expired(now + TTL);
        assert_eq!(entries.proofs.len(), 1);
        entries.purge_expired(now + TTL + Duration::from_secs(1));
        assert!(entries.proofs.is_empty());
        assert!(entries.order.is_empty());
    }
}
