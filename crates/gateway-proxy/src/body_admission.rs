use bytes::Bytes;
use gateway_core::{GatewayError, GatewayResult};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

pub const DEFAULT_MAX_BUFFERED_REQUESTS: usize = 8;
pub const DEFAULT_MAX_INFLIGHT_BUFFER_BYTES: usize = 512 * 1024 * 1024;

#[derive(Debug)]
struct BodyAdmissionState {
    max_requests: usize,
    max_bytes: usize,
    active_requests: AtomicUsize,
    active_bytes: AtomicUsize,
}

#[derive(Debug, Clone)]
pub struct BodyAdmissionController {
    state: Arc<BodyAdmissionState>,
}

#[derive(Debug)]
pub struct BodyAdmissionLease {
    state: Arc<BodyAdmissionState>,
    reserved_bytes: usize,
}

struct AdmittedBody {
    // Drop the buffer before returning its capacity to concurrent requests.
    body: Bytes,
    _lease: BodyAdmissionLease,
}

impl AsRef<[u8]> for AdmittedBody {
    fn as_ref(&self) -> &[u8] {
        self.body.as_ref()
    }
}

impl BodyAdmissionController {
    pub fn new(max_requests: usize, max_bytes: usize) -> GatewayResult<Self> {
        if max_requests == 0 || max_bytes == 0 {
            return Err(GatewayError::InvalidConfiguration);
        }
        Ok(Self {
            state: Arc::new(BodyAdmissionState {
                max_requests,
                max_bytes,
                active_requests: AtomicUsize::new(0),
                active_bytes: AtomicUsize::new(0),
            }),
        })
    }

    pub fn try_acquire(&self) -> GatewayResult<BodyAdmissionLease> {
        increment_bounded(&self.state.active_requests, 1, self.state.max_requests).map_err(
            |_| {
                gateway_telemetry::record_body_admission_rejection("concurrency");
                GatewayError::GatewayOverloaded
            },
        )?;
        gateway_telemetry::buffered_request_started();
        Ok(BodyAdmissionLease {
            state: Arc::clone(&self.state),
            reserved_bytes: 0,
        })
    }

    pub fn active_requests(&self) -> usize {
        self.state.active_requests.load(Ordering::Relaxed)
    }

    pub fn active_bytes(&self) -> usize {
        self.state.active_bytes.load(Ordering::Relaxed)
    }

    pub fn max_requests(&self) -> usize {
        self.state.max_requests
    }

    pub fn max_bytes(&self) -> usize {
        self.state.max_bytes
    }
}

impl Default for BodyAdmissionController {
    fn default() -> Self {
        Self::new(
            DEFAULT_MAX_BUFFERED_REQUESTS,
            DEFAULT_MAX_INFLIGHT_BUFFER_BYTES,
        )
        .expect("default body admission limits are valid")
    }
}

impl BodyAdmissionLease {
    pub fn try_reserve(&mut self, additional_bytes: usize) -> GatewayResult<()> {
        if additional_bytes == 0 {
            return Ok(());
        }
        increment_bounded(
            &self.state.active_bytes,
            additional_bytes,
            self.state.max_bytes,
        )
        .map_err(|_| {
            gateway_telemetry::record_body_admission_rejection("bytes");
            GatewayError::GatewayOverloaded
        })?;
        self.reserved_bytes = self.reserved_bytes.saturating_add(additional_bytes);
        gateway_telemetry::buffered_bytes_added(additional_bytes);
        Ok(())
    }

    pub fn reserved_bytes(&self) -> usize {
        self.reserved_bytes
    }

    /// Keep admission with the forwarded allocation until its last reference
    /// is dropped, including slices or clones queued behind upstream writes.
    pub(crate) fn into_bytes(mut self, body: Bytes) -> GatewayResult<Bytes> {
        // Guardrails may expand a body after its original chunks were admitted.
        self.try_reserve(body.len().saturating_sub(self.reserved_bytes))?;
        Ok(Bytes::from_owner(AdmittedBody { body, _lease: self }))
    }
}

impl Drop for BodyAdmissionLease {
    fn drop(&mut self) {
        if self.reserved_bytes > 0 {
            self.state
                .active_bytes
                .fetch_sub(self.reserved_bytes, Ordering::Relaxed);
            gateway_telemetry::buffered_bytes_removed(self.reserved_bytes);
        }
        self.state.active_requests.fetch_sub(1, Ordering::Relaxed);
        gateway_telemetry::buffered_request_finished();
    }
}

fn increment_bounded(counter: &AtomicUsize, amount: usize, maximum: usize) -> Result<(), ()> {
    let mut current = counter.load(Ordering::Relaxed);
    loop {
        let next = current.checked_add(amount).ok_or(())?;
        if next > maximum {
            return Err(());
        }
        match counter.compare_exchange_weak(current, next, Ordering::AcqRel, Ordering::Relaxed) {
            Ok(_) => return Ok(()),
            Err(observed) => current = observed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_documented_memory_budget() {
        let controller = BodyAdmissionController::default();

        assert_eq!(controller.max_requests(), 8);
        assert_eq!(controller.max_bytes(), 536_870_912);
    }

    #[test]
    fn bounds_concurrent_buffered_requests_and_releases_on_drop() {
        let controller = BodyAdmissionController::new(1, 16).expect("controller");
        let lease = controller.try_acquire().expect("first lease");

        assert_eq!(controller.active_requests(), 1);
        assert_eq!(
            controller.try_acquire().unwrap_err(),
            GatewayError::GatewayOverloaded
        );

        drop(lease);
        assert_eq!(controller.active_requests(), 0);
        assert!(controller.try_acquire().is_ok());
    }

    #[test]
    fn bounds_aggregate_bytes_without_leaking_failed_reservations() {
        let controller = BodyAdmissionController::new(2, 10).expect("controller");
        let mut first = controller.try_acquire().expect("first lease");
        let mut second = controller.try_acquire().expect("second lease");

        first.try_reserve(6).expect("first reservation");
        assert_eq!(
            second.try_reserve(5).unwrap_err(),
            GatewayError::GatewayOverloaded
        );
        assert_eq!(controller.active_bytes(), 6);
        assert_eq!(second.reserved_bytes(), 0);

        second.try_reserve(4).expect("remaining capacity");
        assert_eq!(controller.active_bytes(), 10);
        drop(first);
        assert_eq!(controller.active_bytes(), 4);
        drop(second);
        assert_eq!(controller.active_bytes(), 0);
    }

    #[test]
    fn forwarded_clones_and_slices_keep_admission_until_the_last_reference() {
        let controller = BodyAdmissionController::new(1, 8).expect("controller");
        let body = Bytes::from(vec![1, 2, 3, 4]);
        let original_ptr = body.as_ptr();
        let forwarded = controller
            .try_acquire()
            .unwrap()
            .into_bytes(body)
            .expect("admitted body");
        assert_eq!(
            forwarded.as_ptr(),
            original_ptr,
            "do not copy the allocation"
        );
        let queued = forwarded.clone();
        let slice = forwarded.slice(1..3);
        drop(forwarded);
        drop(queued);

        assert_eq!(slice.as_ref(), &[2, 3]);
        assert_eq!(controller.active_requests(), 1);
        assert_eq!(controller.active_bytes(), 4);
        assert_eq!(
            controller.try_acquire().unwrap_err(),
            GatewayError::GatewayOverloaded
        );

        drop(slice);
        assert_eq!(controller.active_requests(), 0);
        assert_eq!(controller.active_bytes(), 0);
        assert!(controller.try_acquire().is_ok());
    }

    #[test]
    fn accounts_for_rewritten_body_growth_and_releases_failed_admission() {
        let controller = BodyAdmissionController::new(1, 8).expect("controller");
        let mut lease = controller.try_acquire().unwrap();
        lease.try_reserve(4).unwrap();
        let forwarded = lease.into_bytes(Bytes::from(vec![0; 8])).unwrap();
        assert_eq!(controller.active_bytes(), 8);
        drop(forwarded);
        assert_eq!(controller.active_bytes(), 0);

        let mut lease = controller.try_acquire().unwrap();
        lease.try_reserve(4).unwrap();
        assert_eq!(
            lease.into_bytes(Bytes::from(vec![0; 9])).unwrap_err(),
            GatewayError::GatewayOverloaded
        );
        assert_eq!(controller.active_requests(), 0);
        assert_eq!(controller.active_bytes(), 0);
    }

    #[tokio::test]
    async fn cancelling_a_queued_write_releases_its_body_admission() {
        let controller = BodyAdmissionController::new(1, 8).expect("controller");
        let body = controller
            .try_acquire()
            .unwrap()
            .into_bytes(Bytes::from(vec![0; 8]))
            .unwrap();
        let (started, ready) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            let _queued_body = body;
            started.send(()).unwrap();
            std::future::pending::<()>().await;
        });
        ready.await.unwrap();
        assert_eq!(controller.active_requests(), 1);
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        assert_eq!(controller.active_requests(), 0);
        assert_eq!(controller.active_bytes(), 0);
    }
}
