//! Fixed-cardinality events recorded where authentication capacity is decided.
use std::sync::atomic::{AtomicU64, Ordering};

macro_rules! events {
    ($($variant:ident => $label:literal),+ $(,)?) => {
        #[derive(Clone, Copy)]
        pub enum AuthAdmissionEvent { $($variant),+ }

        const LABELS: &[&str] = &[$($label),+];
        static EVENTS: [AtomicU64; LABELS.len()] = [const { AtomicU64::new(0) }; LABELS.len()];
    };
}

events! {
    RequestQueued => "request_queued",
    RequestQueueFull => "request_queue_full",
    RequestTimeout => "request_timeout",
    SourceCapacity => "source_capacity",
    SourceRate => "source_rate",
    LookupFull => "lookup_full",
    CryptoQueueFull => "crypto_queue_full",
    CryptoTimeout => "crypto_timeout",
    ProofLeader => "proof_leader",
    ProofShared => "proof_shared",
    ProofCapacity => "proof_capacity",
    ProofTimeout => "proof_timeout",
    ProofUnavailable => "proof_unavailable",
    CacheHit => "cache_hit",
}

pub fn record_auth_admission_event(event: AuthAdmissionEvent) {
    EVENTS[event as usize].fetch_add(1, Ordering::Relaxed);
}

pub(super) fn render(metrics: &mut String) {
    metrics.push_str("# TYPE gateway_auth_admission_events_total counter\n");
    for (event, count) in LABELS.iter().zip(EVENTS.iter()) {
        metrics.push_str(&format!(
            "gateway_auth_admission_events_total{{event=\"{event}\"}} {}\n",
            count.load(Ordering::Relaxed)
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exports_distinct_bounded_origins_without_request_identity() {
        record_auth_admission_event(AuthAdmissionEvent::CryptoQueueFull);
        record_auth_admission_event(AuthAdmissionEvent::RequestTimeout);
        let mut output = String::new();
        render(&mut output);
        for event in [
            "crypto_queue_full",
            "crypto_timeout",
            "request_timeout",
            "lookup_full",
        ] {
            assert!(output.contains(&format!("event=\"{event}\"")));
        }
        assert_eq!(output.lines().count(), EVENTS.len() + 1);
    }
}
