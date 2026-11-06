//! Process-local admission before untrusted requests can reach stores or KDFs.
use crate::{GatewayError, GatewayResult};
use gateway_telemetry::{record_auth_admission_event, AuthAdmissionEvent};
use http::HeaderMap;
use std::{
    collections::HashMap,
    net::IpAddr,
    sync::Mutex,
    time::{Duration, Instant},
};
use tokio::sync::{Semaphore, SemaphorePermit};

pub const DEFAULT_SOURCE_RPS: usize = 100;
pub const DEFAULT_SOURCE_BURST: usize = 200;
const SOURCE_CAPACITY: usize = 4096;
const SOURCE_TTL: Duration = Duration::from_secs(60);
const REQUEST_WAIT: Duration = Duration::from_millis(250);
pub const MAX_KEY_BYTES: usize = 1024;
const MAX_CREDENTIAL_HEADER_BYTES: usize = 16 * 1024;

#[derive(Debug)]
pub struct RequestAdmission {
    slots: Semaphore,
    waiters: Semaphore,
    sources: Mutex<Sources>,
    rps: usize,
    burst: usize,
}

#[derive(Debug)]
struct Sources {
    buckets: HashMap<Option<IpAddr>, Bucket>,
    last_prune: Instant,
}

#[derive(Debug)]
struct Bucket {
    tokens: f64,
    updated: Instant,
}

impl Default for RequestAdmission {
    fn default() -> Self {
        Self::new(DEFAULT_SOURCE_RPS, DEFAULT_SOURCE_BURST).expect("valid defaults")
    }
}

impl RequestAdmission {
    pub fn new(rps: usize, burst: usize) -> GatewayResult<Self> {
        if !(1..=1_000_000).contains(&rps) || !(1..=1_000_000).contains(&burst) {
            return Err(GatewayError::InvalidConfiguration);
        }
        Ok(Self {
            slots: Semaphore::new(64),
            waiters: Semaphore::new(128),
            sources: Mutex::new(Sources {
                buckets: HashMap::new(),
                last_prune: Instant::now(),
            }),
            rps,
            burst,
        })
    }

    /// Only pass the transport peer, never an unvalidated forwarded header.
    /// Unknown peers share one bucket rather than bypassing enforcement.
    pub async fn acquire(&self, peer: Option<IpAddr>) -> GatewayResult<SemaphorePermit<'_>> {
        self.check_source(peer, Instant::now())?;
        if let Ok(permit) = self.slots.try_acquire() {
            return Ok(permit);
        }
        let _waiting = self
            .waiters
            .try_acquire()
            .map_err(|_| overload(AuthAdmissionEvent::RequestQueueFull))?;
        record_auth_admission_event(AuthAdmissionEvent::RequestQueued);
        // No body is polled by the adapters until this returns. Cancellation
        // drops both the queue reservation and semaphore acquisition future.
        tokio::time::timeout(REQUEST_WAIT, self.slots.acquire())
            .await
            .map_err(|_| overload(AuthAdmissionEvent::RequestTimeout))?
            .map_err(|_| overload(AuthAdmissionEvent::RequestQueueFull))
    }

    fn check_source(&self, peer: Option<IpAddr>, now: Instant) -> GatewayResult<()> {
        let peer = peer.map(|ip| match ip {
            IpAddr::V6(v6) => v6.to_ipv4_mapped().map(IpAddr::V4).unwrap_or(ip),
            _ => ip,
        });
        let mut sources = self
            .sources
            .lock()
            .map_err(|_| overload(AuthAdmissionEvent::SourceCapacity))?;
        if now.duration_since(sources.last_prune) >= Duration::from_secs(1) {
            sources.buckets.retain(|_, bucket| {
                let idle = now.duration_since(bucket.updated);
                idle < SOURCE_TTL
                    || bucket.tokens + idle.as_secs_f64() * (self.rps as f64) < self.burst as f64
            });
            sources.last_prune = now;
        }
        // Never evict an active bucket to make room for an attacker-chosen IP.
        if !sources.buckets.contains_key(&peer) && sources.buckets.len() >= SOURCE_CAPACITY {
            return Err(overload(AuthAdmissionEvent::SourceCapacity));
        }
        let bucket = sources.buckets.entry(peer).or_insert(Bucket {
            tokens: self.burst as f64,
            updated: now,
        });
        bucket.tokens = (bucket.tokens
            + now.duration_since(bucket.updated).as_secs_f64() * self.rps as f64)
            .min(self.burst as f64);
        bucket.updated = now;
        if bucket.tokens < 1.0 {
            record_auth_admission_event(AuthAdmissionEvent::SourceRate);
            return Err(GatewayError::SourceRateLimitExceeded);
        }
        bucket.tokens -= 1.0;
        Ok(())
    }
}

pub(crate) fn overload(event: AuthAdmissionEvent) -> GatewayError {
    record_auth_admission_event(event);
    GatewayError::AuthenticationOverloaded
}

/// Cheap credential checks shared by HTTP adapters before routing/store work.
/// JWTs and caller-owned LiteLLM credentials remain valid credential forms.
pub fn validate_credential_headers(headers: &HeaderMap, key_header: &str) -> GatewayResult<()> {
    for name in [
        "authorization",
        key_header,
        "x-apigee-entra-identity",
        "x-apigee-entra-signature",
        "x-relayna-admission-context",
        "x-relayna-worker-token",
    ] {
        let mut values = headers.get_all(name).iter();
        if let Some(value) = values.next() {
            if values.next().is_some() || value.as_bytes().len() > MAX_CREDENTIAL_HEADER_BYTES {
                return Err(GatewayError::MalformedAuthorization);
            }
            let value = value
                .to_str()
                .map_err(|_| GatewayError::MalformedAuthorization)?;
            if name == key_header {
                validate_key_text(value.trim())?;
                if !value.trim().starts_with("rk_live_") || value.trim().len() <= 16 {
                    return Err(GatewayError::MalformedAuthorization);
                }
            } else if name == "authorization" {
                let token = value
                    .strip_prefix("Bearer ")
                    .ok_or(GatewayError::MalformedAuthorization)?
                    .trim();
                if token.is_empty() || !token.bytes().all(|b| b.is_ascii_graphic()) {
                    return Err(GatewayError::MalformedAuthorization);
                }
                if token.starts_with("rk_live_") {
                    validate_key_text(token)?;
                }
            }
        }
    }
    Ok(())
}

pub fn validate_key_text(value: &str) -> GatewayResult<()> {
    if value.len() > MAX_KEY_BYTES || !value.bytes().all(|b| b.is_ascii_graphic() && b != b',') {
        return Err(GatewayError::MalformedAuthorization);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounds_sources_expires_idle_entries_and_preserves_active_limits() {
        let admission = RequestAdmission::new(1, 1).unwrap();
        let now = Instant::now();
        for index in 0..SOURCE_CAPACITY {
            admission
                .check_source(Some(IpAddr::V4((index as u32).into())), now)
                .unwrap();
        }
        assert_eq!(
            admission.check_source(Some("192.0.2.1".parse().unwrap()), now),
            Err(GatewayError::AuthenticationOverloaded)
        );
        assert_eq!(
            admission.check_source(Some(IpAddr::V4(0.into())), now),
            Err(GatewayError::SourceRateLimitExceeded)
        );
        admission.check_source(None, now + SOURCE_TTL).unwrap();
        assert_eq!(admission.sources.lock().unwrap().buckets.len(), 1);
    }

    #[test]
    fn mapped_addresses_share_bucket_and_tokens_refill() {
        let admission = RequestAdmission::new(2, 1).unwrap();
        let now = Instant::now();
        admission
            .check_source(Some("192.0.2.1".parse().unwrap()), now)
            .unwrap();
        assert_eq!(
            admission.check_source(Some("::ffff:192.0.2.1".parse().unwrap()), now),
            Err(GatewayError::SourceRateLimitExceeded)
        );
        admission
            .check_source(
                Some("192.0.2.1".parse().unwrap()),
                now + Duration::from_millis(500),
            )
            .unwrap();
    }

    #[tokio::test]
    async fn admission_queue_is_bounded_and_releases_on_timeout_and_cancellation() {
        let admission = std::sync::Arc::new(RequestAdmission::new(1_000_000, 1_000_000).unwrap());
        let permits = admission.slots.acquire_many(64).await.unwrap();
        let mut jobs = Vec::new();
        for _ in 0..128 {
            let admission = admission.clone();
            jobs.push(tokio::spawn(async move {
                admission.acquire(None).await.map(drop)
            }));
        }
        while admission.waiters.available_permits() != 0 {
            tokio::task::yield_now().await;
        }
        assert_eq!(
            admission.acquire(None).await.unwrap_err(),
            GatewayError::AuthenticationOverloaded
        );
        let cancelled = jobs.pop().unwrap();
        cancelled.abort();
        assert!(cancelled.await.unwrap_err().is_cancelled());
        assert_eq!(admission.waiters.available_permits(), 1);
        for job in jobs {
            assert_eq!(
                job.await.unwrap(),
                Err(GatewayError::AuthenticationOverloaded)
            );
        }
        assert_eq!(admission.waiters.available_permits(), 128);
        drop(permits);
        assert!(admission.acquire(None).await.is_ok());
    }

    #[tokio::test]
    async fn queued_request_uses_released_capacity_but_sources_are_checked_first() {
        let admission = std::sync::Arc::new(RequestAdmission::new(1, 1).unwrap());
        let held = admission.slots.acquire_many(64).await.unwrap();
        let queued = admission.clone();
        let job = tokio::spawn(async move { queued.acquire(None).await.map(drop) });
        while admission.waiters.available_permits() == 128 {
            tokio::task::yield_now().await;
        }
        assert_eq!(
            admission.acquire(None).await.unwrap_err(),
            GatewayError::SourceRateLimitExceeded
        );
        drop(held);
        job.await.unwrap().unwrap();
        assert_eq!(admission.waiters.available_permits(), 128);
        assert_eq!(admission.slots.available_permits(), 64);
    }

    #[test]
    fn rejects_duplicate_oversized_and_malformed_credentials() {
        for name in [
            "authorization",
            "x-custom-key",
            "x-apigee-entra-identity",
            "x-apigee-entra-signature",
            "x-relayna-admission-context",
            "x-relayna-worker-token",
        ] {
            let mut headers = HeaderMap::new();
            headers.append(name, "Bearer abc".parse().unwrap());
            headers.append(name, "Bearer def".parse().unwrap());
            assert_eq!(
                validate_credential_headers(&headers, "x-custom-key"),
                Err(GatewayError::MalformedAuthorization)
            );
        }
        for value in [
            "Bearer ".to_owned(),
            "Bearer abc def".into(),
            format!("Bearer {}", "x".repeat(MAX_CREDENTIAL_HEADER_BYTES)),
            format!("Bearer rk_live_{}", "x".repeat(MAX_KEY_BYTES)),
        ] {
            let mut headers = HeaderMap::new();
            headers.insert("authorization", value.parse().unwrap());
            assert!(validate_credential_headers(&headers, "x-custom-key").is_err());
        }
        let mut headers = HeaderMap::new();
        headers.insert(
            "authorization",
            "Bearer jwt.segment.signature".parse().unwrap(),
        );
        headers.insert("x-custom-key", "rk_live_1234567890".parse().unwrap());
        assert!(validate_credential_headers(&headers, "x-custom-key").is_ok());
    }
}
