//! Coalesce secret verification only, never authorization or database state.
use super::{run_key_crypto, verification_cache};
use crate::{auth_admission::overload, GatewayResult};
use gateway_telemetry::{record_auth_admission_event, AuthAdmissionEvent};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, OnceLock},
    time::Duration,
};
use tokio::sync::{watch, OwnedSemaphorePermit, Semaphore};
use zeroize::Zeroize;

type Proof = [u8; 32];
const MAX_FLIGHTS: usize = 10;
const MAX_FOLLOWERS_PER_PROOF: usize = 63;
const PROOF_WAIT: Duration = Duration::from_secs(1);
static FLIGHTS: OnceLock<Flights> = OnceLock::new();

struct Flights {
    entries: Mutex<HashMap<Proof, Flight>>,
    followers: Arc<Semaphore>,
}

impl Default for Flights {
    fn default() -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            followers: Arc::new(Semaphore::new(64)),
        }
    }
}

struct Flight {
    result: watch::Receiver<Option<GatewayResult<()>>>,
    followers: Arc<Semaphore>,
}

struct Waiting {
    result: watch::Receiver<Option<GatewayResult<()>>>,
    _global: Option<OwnedSemaphorePermit>,
    _per_proof: Option<OwnedSemaphorePermit>,
}

struct Leader<'a> {
    flights: &'a Flights,
    proof: Proof,
    result: watch::Sender<Option<GatewayResult<()>>>,
}

impl Drop for Leader<'_> {
    fn drop(&mut self) {
        if let Ok(mut entries) = self.flights.entries.lock() {
            if let Some((mut proof, _)) = entries.remove_entry(&self.proof) {
                proof.zeroize();
            }
        }
        self.proof.zeroize();
        // Dropping the only sender wakes subscribers on panic/runtime shutdown.
    }
}

impl Flights {
    fn join(&self, proof: Proof) -> GatewayResult<(Waiting, Option<Leader<'_>>)> {
        let mut entries = self
            .entries
            .lock()
            .map_err(|_| overload(AuthAdmissionEvent::ProofUnavailable))?;
        if let Some(flight) = entries.get(&proof) {
            let global = self
                .followers
                .clone()
                .try_acquire_owned()
                .map_err(|_| overload(AuthAdmissionEvent::ProofCapacity))?;
            let per_proof = flight
                .followers
                .clone()
                .try_acquire_owned()
                .map_err(|_| overload(AuthAdmissionEvent::ProofCapacity))?;
            record_auth_admission_event(AuthAdmissionEvent::ProofShared);
            return Ok((
                Waiting {
                    result: flight.result.clone(),
                    _global: Some(global),
                    _per_proof: Some(per_proof),
                },
                None,
            ));
        }
        if entries.len() >= MAX_FLIGHTS {
            return Err(overload(AuthAdmissionEvent::ProofCapacity));
        }
        let (tx, rx) = watch::channel(None);
        entries.insert(
            proof,
            Flight {
                result: rx.clone(),
                followers: Arc::new(Semaphore::new(MAX_FOLLOWERS_PER_PROOF)),
            },
        );
        record_auth_admission_event(AuthAdmissionEvent::ProofLeader);
        Ok((
            Waiting {
                result: rx,
                _global: None,
                _per_proof: None,
            },
            Some(Leader {
                flights: self,
                proof,
                result: tx,
            }),
        ))
    }
}

impl Waiting {
    async fn wait(mut self) -> GatewayResult<()> {
        tokio::time::timeout(PROOF_WAIT, async {
            loop {
                if let Some(result) = self.result.borrow_and_update().clone() {
                    return result;
                }
                self.result
                    .changed()
                    .await
                    .map_err(|_| overload(AuthAdmissionEvent::ProofUnavailable))?;
            }
        })
        .await
        .map_err(|_| overload(AuthAdmissionEvent::ProofTimeout))?
    }
}

pub(super) async fn verify(
    proof: Option<Proof>,
    work: impl FnOnce() -> GatewayResult<()> + Send + 'static,
) -> GatewayResult<()> {
    let Some(proof) = proof else {
        return run_key_crypto(work).await;
    };
    if verification_cache::contains(Some(proof)) {
        return Ok(());
    }
    let (waiting, leader) = FLIGHTS.get_or_init(Flights::default).join(proof)?;
    if let Some(leader) = leader {
        // Bounded by MAX_FLIGHTS. A cancelled first request must not cancel
        // verification that other requests already depend on. Only leaders
        // own verifier buffers; the existing executor retains active permits.
        tokio::spawn(async move {
            let result = run_key_crypto(work).await;
            leader.result.send_replace(Some(result));
            drop(leader);
        });
    } else {
        drop(work);
    }
    waiting.wait().await
}

#[cfg(test)]
pub(super) fn follower_count() -> usize {
    64 - FLIGHTS
        .get_or_init(Flights::default)
        .followers
        .available_permits()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::GatewayError;

    #[tokio::test]
    async fn bounds_leaders_and_followers_and_releases_cancelled_waiters() {
        let flights = Flights::default();
        let (first, leader) = flights.join([0; 32]).unwrap();
        let mut followers = Vec::new();
        for _ in 0..63 {
            let (waiting, leader) = flights.join([0; 32]).unwrap();
            assert!(leader.is_none());
            followers.push(waiting);
        }
        assert!(matches!(
            flights.join([0; 32]),
            Err(GatewayError::AuthenticationOverloaded)
        ));
        let (second, second_leader) = flights.join([1; 32]).unwrap();
        let (last_follower, _) = flights.join([1; 32]).unwrap();
        assert!(matches!(
            flights.join([1; 32]),
            Err(GatewayError::AuthenticationOverloaded)
        ));
        drop(last_follower);
        let mut leaders = vec![leader, second_leader];
        for index in 2..10 {
            leaders.push(flights.join([index; 32]).unwrap().1);
        }
        assert!(matches!(
            flights.join([10; 32]),
            Err(GatewayError::AuthenticationOverloaded)
        ));
        drop(followers);
        assert_eq!(flights.followers.available_permits(), 64);
        leaders[0]
            .as_ref()
            .unwrap()
            .result
            .send_replace(Some(Ok(())));
        first.wait().await.unwrap();
        drop(leaders);
        assert_eq!(
            second.wait().await,
            Err(GatewayError::AuthenticationOverloaded)
        );
        assert!(flights.entries.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn deadline_releases_follower_capacity_without_removing_active_work() {
        let flights = Flights::default();
        let (_, leader) = flights.join([0; 32]).unwrap();
        let (follower, _) = flights.join([0; 32]).unwrap();
        assert_eq!(
            follower.wait().await,
            Err(GatewayError::AuthenticationOverloaded)
        );
        assert_eq!(flights.followers.available_permits(), 64);
        assert_eq!(flights.entries.lock().unwrap().len(), 1);
        drop(leader);
        assert!(flights.entries.lock().unwrap().is_empty());
    }
}
