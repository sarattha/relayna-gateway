//! Bounded, metadata-only process monitoring, independent of usage persistence.
//!
//! Start a `RuntimeRequest` before authentication. Set its transport once known,
//! and finish it on every terminal path (including un-attributed rejections).
//! `finish` is idempotent; dropping an unfinished request records a failure.
//! Existing Prometheus request accounting remains owned by its existing callers.
//!
//! Snapshots contain one rolling window at one-second resolution. Histograms
//! use fixed, non-cumulative microsecond buckets; merge bucket counts, never
//! percentiles. Quantiles are bucket upper-bound estimates, or null for no
//! samples / an overflowing quantile. Redis envelopes own instance identity,
//! RFC3339 timestamps and freshness; none of those are inferred here.

use serde::{Deserialize, Serialize};
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const RETAIN_SECONDS: u64 = 900;
const HISTOGRAM_BOUNDS_US: [u64; 25] = [
    10,
    25,
    50,
    100,
    250,
    500,
    1_000,
    2_500,
    5_000,
    10_000,
    25_000,
    50_000,
    100_000,
    250_000,
    500_000,
    1_000_000,
    2_500_000,
    5_000_000,
    10_000_000,
    30_000_000,
    60_000_000,
    120_000_000,
    300_000_000,
    900_000_000,
    3_600_000_000,
];
static RUNTIME: OnceLock<Arc<RuntimeMetrics>> = OnceLock::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeTransport {
    Http,
    Sse,
    WebSocket,
}

/// Success is explicit: an interrupted SSE stream can have HTTP status 200.
/// Only supply timing samples actually observed by the proxy. WS session
/// duration must exclude the handshake; unsuccessful handshakes have no session.
#[derive(Debug, Clone, Default)]
pub struct RuntimeOutcome {
    pub success: bool,
    pub rejected: bool,
    pub attributed: bool,
    pub upstream_duration_ms: Option<u64>,
    pub ttfb_ms: Option<u64>,
    pub ttft_ms: Option<u64>,
    pub websocket_handshake_ms: Option<u64>,
    pub websocket_session_ms: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeCounters {
    pub started: u64,
    pub completed: u64,
    pub succeeded: u64,
    pub failed: u64,
    pub rejected: u64,
    pub unattributed: u64,
    pub http_completed: u64,
    pub sse_completed: u64,
    pub websocket_completed: u64,
}

impl RuntimeCounters {
    fn merge(&mut self, other: &Self) {
        macro_rules! add {
            ($($field:ident),+ $(,)?) => { $(self.$field = self.$field.saturating_add(other.$field);)+ };
        }
        add!(
            started,
            completed,
            succeeded,
            failed,
            rejected,
            unattributed,
            http_completed,
            sse_completed,
            websocket_completed
        );
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeActive {
    pub http: u64,
    pub sse: u64,
    pub websocket: u64,
}

impl RuntimeActive {
    fn for_transport(&mut self, transport: RuntimeTransport) -> &mut u64 {
        match transport {
            RuntimeTransport::Http => &mut self.http,
            RuntimeTransport::Sse => &mut self.sse,
            RuntimeTransport::WebSocket => &mut self.websocket,
        }
    }

    fn merge(&mut self, other: &Self) {
        self.http = self.http.saturating_add(other.http);
        self.sse = self.sse.saturating_add(other.sse);
        self.websocket = self.websocket.saturating_add(other.websocket);
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HistogramSnapshot {
    pub upper_bounds_us: [u64; 25],
    /// Non-cumulative; the final bucket is overflow (greater than the last bound).
    pub bucket_counts: [u64; 26],
    pub sample_count: u64,
    pub sum_us: u64,
    pub p50_ms: Option<f64>,
    pub p95_ms: Option<f64>,
    pub p99_ms: Option<f64>,
}

impl Default for HistogramSnapshot {
    fn default() -> Self {
        Self {
            upper_bounds_us: HISTOGRAM_BOUNDS_US,
            bucket_counts: [0; 26],
            sample_count: 0,
            sum_us: 0,
            p50_ms: None,
            p95_ms: None,
            p99_ms: None,
        }
    }
}

impl HistogramSnapshot {
    fn observe_us(&mut self, value: u64) {
        let index = HISTOGRAM_BOUNDS_US.partition_point(|bound| *bound < value);
        self.bucket_counts[index] = self.bucket_counts[index].saturating_add(1);
        self.sample_count = self.sample_count.saturating_add(1);
        self.sum_us = self.sum_us.saturating_add(value);
    }

    fn observe_ms(&mut self, value: Option<u64>) {
        if let Some(value) = value {
            self.observe_us(value.saturating_mul(1_000));
        }
    }

    fn merge(&mut self, other: &Self) {
        for (count, other) in self.bucket_counts.iter_mut().zip(other.bucket_counts) {
            *count = count.saturating_add(other);
        }
        self.sample_count = self.sample_count.saturating_add(other.sample_count);
        self.sum_us = self.sum_us.saturating_add(other.sum_us);
    }

    fn valid(&self) -> bool {
        self.upper_bounds_us == HISTOGRAM_BOUNDS_US
            && self
                .bucket_counts
                .iter()
                .try_fold(0_u64, |sum, n| sum.checked_add(*n))
                == Some(self.sample_count)
    }

    fn quantile(&self, percentile: u64) -> Option<f64> {
        if self.sample_count == 0 {
            return None;
        }
        let rank = (u128::from(self.sample_count) * u128::from(percentile)).div_ceil(100);
        let mut count = 0_u128;
        for (bound, n) in HISTOGRAM_BOUNDS_US.iter().zip(self.bucket_counts) {
            count += u128::from(n);
            if count >= rank {
                return Some(*bound as f64 / 1_000.0);
            }
        }
        None
    }

    fn refresh_quantiles(&mut self) {
        self.p50_ms = self.quantile(50);
        self.p95_ms = self.quantile(95);
        self.p99_ms = self.quantile(99);
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RuntimeLatency {
    pub http_duration: HistogramSnapshot,
    pub http_success_duration: HistogramSnapshot,
    pub upstream_duration: HistogramSnapshot,
    pub ttfb: HistogramSnapshot,
    pub ttft: HistogramSnapshot,
    pub websocket_handshake: HistogramSnapshot,
    pub websocket_session: HistogramSnapshot,
    pub admission_decision: HistogramSnapshot,
    pub admission_rejection: HistogramSnapshot,
    pub body_retention: HistogramSnapshot,
}

impl RuntimeLatency {
    fn histograms(&self) -> [&HistogramSnapshot; 10] {
        [
            &self.http_duration,
            &self.http_success_duration,
            &self.upstream_duration,
            &self.ttfb,
            &self.ttft,
            &self.websocket_handshake,
            &self.websocket_session,
            &self.admission_decision,
            &self.admission_rejection,
            &self.body_retention,
        ]
    }

    fn histograms_mut(&mut self) -> [&mut HistogramSnapshot; 10] {
        [
            &mut self.http_duration,
            &mut self.http_success_duration,
            &mut self.upstream_duration,
            &mut self.ttfb,
            &mut self.ttft,
            &mut self.websocket_handshake,
            &mut self.websocket_session,
            &mut self.admission_decision,
            &mut self.admission_rejection,
            &mut self.body_retention,
        ]
    }

    fn merge(&mut self, other: &Self) {
        for (histogram, other) in self.histograms_mut().into_iter().zip(other.histograms()) {
            histogram.merge(other);
        }
    }

    fn refresh_quantiles(&mut self) {
        for histogram in self.histograms_mut() {
            histogram.refresh_quantiles();
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdmissionDenials {
    pub concurrency: u64,
    pub bytes: u64,
    pub other: u64,
}

impl AdmissionDenials {
    fn merge(&mut self, other: &Self) {
        self.concurrency = self.concurrency.saturating_add(other.concurrency);
        self.bytes = self.bytes.saturating_add(other.bytes);
        self.other = self.other.saturating_add(other.other);
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordingFailures {
    pub usage_events: u64,
    pub debug_bundles: u64,
    pub request_traffic: u64,
}

impl RecordingFailures {
    fn merge(&mut self, other: &Self) {
        self.usage_events = self.usage_events.saturating_add(other.usage_events);
        self.debug_bundles = self.debug_bundles.saturating_add(other.debug_bundles);
        self.request_traffic = self.request_traffic.saturating_add(other.request_traffic);
    }

    fn for_destination(destination: &str) -> Option<Self> {
        let mut failures = Self::default();
        match destination {
            "usage_events" => failures.usage_events = 1,
            "debug_bundles" => failures.debug_bundles = 1,
            "request_traffic" => failures.request_traffic = 1,
            _ => return None,
        }
        Some(failures)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BodyCapacitySnapshot {
    pub max_requests: u64,
    pub used_requests: u64,
    pub free_requests: u64,
    pub max_bytes: u64,
    pub used_bytes: u64,
    pub free_bytes: u64,
}

impl BodyCapacitySnapshot {
    pub fn new(
        max_requests: usize,
        used_requests: usize,
        max_bytes: usize,
        used_bytes: usize,
    ) -> Self {
        Self {
            max_requests: max_requests as u64,
            used_requests: used_requests as u64,
            free_requests: max_requests.saturating_sub(used_requests) as u64,
            max_bytes: max_bytes as u64,
            used_bytes: used_bytes as u64,
            free_bytes: max_bytes.saturating_sub(used_bytes) as u64,
        }
    }

    fn merge(&mut self, other: &Self) {
        self.max_requests = self.max_requests.saturating_add(other.max_requests);
        self.used_requests = self.used_requests.saturating_add(other.used_requests);
        self.free_requests = self.free_requests.saturating_add(other.free_requests);
        self.max_bytes = self.max_bytes.saturating_add(other.max_bytes);
        self.used_bytes = self.used_bytes.saturating_add(other.used_bytes);
        self.free_bytes = self.free_bytes.saturating_add(other.free_bytes);
    }
}

/// Cgroup values describe this process's cgroup, which may include sidecars.
/// Missing/unsupported values stay null; restart counts are not observable here.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ProcessSnapshot {
    pub rss_bytes: Option<u64>,
    pub cgroup_memory_current_bytes: Option<u64>,
    pub cgroup_memory_limit_bytes: Option<u64>,
    pub cgroup_cpu_usage_usec: Option<u64>,
    pub cgroup_cpu_limit_cores: Option<f64>,
    pub cgroup_cpu_periods: Option<u64>,
    pub cgroup_cpu_throttled_periods: Option<u64>,
    pub cgroup_cpu_throttled_usec: Option<u64>,
    pub cgroup_oom_events: Option<u64>,
    pub cgroup_oom_kills: Option<u64>,
    pub restarts: Option<u64>,
}

impl ProcessSnapshot {
    fn merge(&mut self, other: &Self) {
        macro_rules! add {
            ($($field:ident),+ $(,)?) => { $(self.$field = self.$field.zip(other.$field).map(|(a, b)| a.saturating_add(b));)+ };
        }
        add!(
            rss_bytes,
            cgroup_memory_current_bytes,
            cgroup_memory_limit_bytes,
            cgroup_cpu_usage_usec,
            cgroup_cpu_periods,
            cgroup_cpu_throttled_periods,
            cgroup_cpu_throttled_usec,
            cgroup_oom_events,
            cgroup_oom_kills,
            restarts
        );
        self.cgroup_cpu_limit_cores = self
            .cgroup_cpu_limit_cores
            .zip(other.cgroup_cpu_limit_cores)
            .map(|(a, b)| a + b);
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeSnapshot {
    pub schema_version: u32,
    pub window_seconds: u64,
    pub sampled_at_unix_ms: u64,
    /// Null on a cluster aggregate; a cluster has no single process uptime.
    pub uptime_seconds: Option<u64>,
    pub observed_seconds: u64,
    pub instance_count: u64,
    pub incompatible_instances: u64,
    pub counters: RuntimeCounters,
    pub totals: RuntimeCounters,
    pub active: RuntimeActive,
    pub body_capacity: Option<BodyCapacitySnapshot>,
    pub admission_denials: AdmissionDenials,
    pub recording_failures: RecordingFailures,
    pub recording_failures_total: RecordingFailures,
    pub latency: RuntimeLatency,
    pub process: ProcessSnapshot,
}

#[derive(Debug, Clone, Default)]
struct Bucket {
    second: u64,
    counters: RuntimeCounters,
    denials: AdmissionDenials,
    recording_failures: RecordingFailures,
    latency: RuntimeLatency,
}

#[derive(Debug, Default)]
struct State {
    buckets: VecDeque<Arc<Bucket>>,
    totals: RuntimeCounters,
    active: RuntimeActive,
    body_limits: Option<(usize, usize)>,
    admission_denials_total: AdmissionDenials,
    recording_failures_total: RecordingFailures,
    admission_latency_total: RuntimeLatency,
}

impl State {
    fn bucket(&mut self, second: u64) -> &mut Bucket {
        self.prune(second);
        if self
            .buckets
            .back()
            .is_none_or(|bucket| bucket.second != second)
        {
            self.buckets.push_back(Arc::new(Bucket {
                second,
                ..Bucket::default()
            }));
        }
        // Historical buckets are immutable. A concurrent snapshot can require
        // copying only the current second's bucket, never the retained history.
        Arc::make_mut(self.buckets.back_mut().expect("bucket was inserted"))
    }

    fn prune(&mut self, second: u64) {
        while self
            .buckets
            .front()
            .is_some_and(|bucket| second.saturating_sub(bucket.second) >= RETAIN_SECONDS)
        {
            self.buckets.pop_front();
        }
    }
}

#[derive(Debug)]
struct RuntimeMetrics {
    started: Instant,
    state: Mutex<State>,
}

impl RuntimeMetrics {
    fn new() -> Self {
        Self {
            started: Instant::now(),
            state: Mutex::new(State::default()),
        }
    }

    fn snapshot(&self, window_seconds: u64) -> RuntimeSnapshot {
        let (mut snapshot, buckets) = self.snapshot_state(window_seconds);
        aggregate_buckets(&mut snapshot, buckets);
        snapshot.process = super::runtime_process::sample_process();
        snapshot
    }

    fn snapshot_state(&self, window_seconds: u64) -> (RuntimeSnapshot, VecDeque<Arc<Bucket>>) {
        let elapsed = self.started.elapsed().as_secs();
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.prune(elapsed);
        let mut snapshot = empty_snapshot(window_seconds);
        snapshot.uptime_seconds = Some(elapsed);
        snapshot.observed_seconds = elapsed.saturating_add(1).min(snapshot.window_seconds);
        snapshot.instance_count = 1;
        snapshot.totals = state.totals.clone();
        snapshot.recording_failures_total = state.recording_failures_total.clone();
        snapshot.active = state.active.clone();
        snapshot.body_capacity = state.body_limits.map(|(requests, bytes)| {
            BodyCapacitySnapshot::new(
                requests,
                super::BUFFERED_REQUESTS
                    .load(std::sync::atomic::Ordering::Relaxed)
                    .max(0) as usize,
                bytes,
                super::BUFFERED_BODY_BYTES.load(std::sync::atomic::Ordering::Relaxed) as usize,
            )
        });
        // Only copy handles and fixed-size current state under the hot-path
        // mutex. The guard is dropped before histogram aggregation can start.
        (snapshot, state.buckets.clone())
    }
}

fn aggregate_buckets(
    snapshot: &mut RuntimeSnapshot,
    buckets: impl IntoIterator<Item = Arc<Bucket>>,
) {
    let elapsed = snapshot.uptime_seconds.unwrap_or_default();
    for bucket in buckets {
        if elapsed.saturating_sub(bucket.second) < snapshot.window_seconds {
            snapshot.counters.merge(&bucket.counters);
            snapshot.admission_denials.merge(&bucket.denials);
            snapshot
                .recording_failures
                .merge(&bucket.recording_failures);
            snapshot.latency.merge(&bucket.latency);
        }
    }
    snapshot.latency.refresh_quantiles();
}

fn runtime() -> &'static Arc<RuntimeMetrics> {
    RUNTIME.get_or_init(|| Arc::new(RuntimeMetrics::new()))
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

fn normalize_window(window_seconds: u64) -> u64 {
    match window_seconds {
        300 | 900 => window_seconds,
        _ => 60,
    }
}

fn empty_snapshot(window_seconds: u64) -> RuntimeSnapshot {
    RuntimeSnapshot {
        schema_version: 1,
        window_seconds: normalize_window(window_seconds),
        sampled_at_unix_ms: now_unix_ms(),
        uptime_seconds: None,
        observed_seconds: 0,
        instance_count: 0,
        incompatible_instances: 0,
        counters: RuntimeCounters::default(),
        totals: RuntimeCounters::default(),
        active: RuntimeActive::default(),
        body_capacity: None,
        admission_denials: AdmissionDenials::default(),
        recording_failures: RecordingFailures::default(),
        recording_failures_total: RecordingFailures::default(),
        latency: RuntimeLatency::default(),
        process: ProcessSnapshot::default(),
    }
}

/// Initialize at startup if process uptime should include time before the first request.
pub fn init_runtime_monitoring() {
    let _ = runtime();
}

pub fn runtime_snapshot(window_seconds: u64) -> RuntimeSnapshot {
    runtime().snapshot(window_seconds)
}

/// Caller must exclude stale/unavailable envelopes before invoking this function.
/// A mixed schema, histogram layout or window is skipped and explicitly counted.
/// Null process/capacity fields propagate if any included process lacks a sample.
pub fn merge_runtime_snapshots(snapshots: &[RuntimeSnapshot]) -> RuntimeSnapshot {
    let mut merged = empty_snapshot(
        snapshots
            .first()
            .map_or(60, |snapshot| snapshot.window_seconds),
    );
    for snapshot in snapshots {
        if snapshot.schema_version != 1
            || snapshot.window_seconds != merged.window_seconds
            || !snapshot
                .latency
                .histograms()
                .iter()
                .all(|histogram| histogram.valid())
        {
            merged.incompatible_instances = merged
                .incompatible_instances
                .saturating_add(snapshot.instance_count.max(1));
            continue;
        }
        if merged.instance_count == 0 {
            merged.sampled_at_unix_ms = snapshot.sampled_at_unix_ms;
            merged.observed_seconds = snapshot.observed_seconds;
            merged.body_capacity = snapshot.body_capacity.clone();
            merged.process = snapshot.process.clone();
        } else {
            merged.sampled_at_unix_ms = merged.sampled_at_unix_ms.min(snapshot.sampled_at_unix_ms);
            merged.observed_seconds = merged.observed_seconds.min(snapshot.observed_seconds);
            merged.body_capacity = merged
                .body_capacity
                .take()
                .zip(snapshot.body_capacity.as_ref())
                .map(|(mut capacity, other)| {
                    capacity.merge(other);
                    capacity
                });
            merged.process.merge(&snapshot.process);
        }
        merged.instance_count = merged
            .instance_count
            .saturating_add(snapshot.instance_count);
        merged.incompatible_instances = merged
            .incompatible_instances
            .saturating_add(snapshot.incompatible_instances);
        merged.counters.merge(&snapshot.counters);
        merged.totals.merge(&snapshot.totals);
        merged.active.merge(&snapshot.active);
        merged.admission_denials.merge(&snapshot.admission_denials);
        merged
            .recording_failures
            .merge(&snapshot.recording_failures);
        merged
            .recording_failures_total
            .merge(&snapshot.recording_failures_total);
        merged.latency.merge(&snapshot.latency);
    }
    merged.latency.refresh_quantiles();
    merged
}

/// Request-local token: no identifiers, paths, identities or payloads retained.
#[derive(Debug)]
pub struct RuntimeRequest {
    metrics: Arc<RuntimeMetrics>,
    started: Instant,
    transport: RuntimeTransport,
    finished: bool,
}

pub fn runtime_request_started(transport: RuntimeTransport) -> RuntimeRequest {
    RuntimeRequest::start(Arc::clone(runtime()), transport)
}

impl RuntimeRequest {
    fn start(metrics: Arc<RuntimeMetrics>, transport: RuntimeTransport) -> Self {
        {
            let mut state = metrics
                .state
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            state.totals.started = state.totals.started.saturating_add(1);
            let bucket = state.bucket(metrics.started.elapsed().as_secs());
            bucket.counters.started = bucket.counters.started.saturating_add(1);
            let active = state.active.for_transport(transport);
            *active = active.saturating_add(1);
        }
        Self {
            metrics,
            started: Instant::now(),
            transport,
            finished: false,
        }
    }

    /// Reclassifies the active gauge without incrementing arrival counters.
    pub fn set_transport(&mut self, transport: RuntimeTransport) {
        if self.finished || self.transport == transport {
            return;
        }
        let mut state = self
            .metrics
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let previous = state.active.for_transport(self.transport);
        *previous = previous.saturating_sub(1);
        let active = state.active.for_transport(transport);
        *active = active.saturating_add(1);
        self.transport = transport;
    }

    /// Returns true only for the first terminal observation.
    pub fn finish(&mut self, outcome: RuntimeOutcome) -> bool {
        if self.finished {
            return false;
        }
        self.finished = true;
        let counters = RuntimeCounters {
            completed: 1,
            succeeded: u64::from(outcome.success),
            failed: u64::from(!outcome.success),
            rejected: u64::from(outcome.rejected),
            unattributed: u64::from(!outcome.attributed),
            http_completed: u64::from(self.transport == RuntimeTransport::Http),
            sse_completed: u64::from(self.transport == RuntimeTransport::Sse),
            websocket_completed: u64::from(self.transport == RuntimeTransport::WebSocket),
            ..RuntimeCounters::default()
        };
        let mut state = self
            .metrics
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let active = state.active.for_transport(self.transport);
        *active = active.saturating_sub(1);
        state.totals.merge(&counters);
        let bucket = state.bucket(self.metrics.started.elapsed().as_secs());
        bucket.counters.merge(&counters);
        if self.transport != RuntimeTransport::WebSocket {
            let duration_us = duration_us(self.started.elapsed());
            bucket.latency.http_duration.observe_us(duration_us);
            if outcome.success {
                bucket.latency.http_success_duration.observe_us(duration_us);
            }
            bucket
                .latency
                .upstream_duration
                .observe_ms(outcome.upstream_duration_ms);
            bucket.latency.ttfb.observe_ms(outcome.ttfb_ms);
            if self.transport == RuntimeTransport::Sse {
                bucket.latency.ttft.observe_ms(outcome.ttft_ms);
            }
        } else {
            bucket
                .latency
                .websocket_handshake
                .observe_ms(outcome.websocket_handshake_ms);
            bucket
                .latency
                .websocket_session
                .observe_ms(outcome.websocket_session_ms);
        }
        true
    }
}

impl Drop for RuntimeRequest {
    fn drop(&mut self) {
        self.finish(RuntimeOutcome::default());
    }
}

fn duration_us(duration: Duration) -> u64 {
    duration.as_micros().try_into().unwrap_or(u64::MAX)
}

/// Called by the body controller; no queue exists or is reported.
pub fn configure_body_capacity(max_requests: usize, max_bytes: usize) {
    runtime()
        .state
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .body_limits = Some((max_requests, max_bytes));
}

/// Fixed destinations only; arbitrary strings never become metric labels.
/// Invoke once when a diagnostic destination fails, independently of DB usage.
pub fn record_recording_failure(destination: &str) {
    let Some(failure) = RecordingFailures::for_destination(destination) else {
        return;
    };
    let metrics = runtime();
    let mut state = metrics
        .state
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    state.recording_failures_total.merge(&failure);
    state
        .bucket(metrics.started.elapsed().as_secs())
        .recording_failures
        .merge(&failure);
}

pub(super) fn record_admission_denial(reason: &str) {
    let metrics = runtime();
    let mut state = metrics
        .state
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let mut denial = AdmissionDenials::default();
    match reason {
        "concurrency" => denial.concurrency = 1,
        "bytes" => denial.bytes = 1,
        _ => denial.other = 1,
    }
    state.admission_denials_total.merge(&denial);
    state
        .bucket(metrics.started.elapsed().as_secs())
        .denials
        .merge(&denial);
}

pub fn record_body_admission_decision(elapsed: Duration, rejected: bool) {
    let metrics = runtime();
    let mut state = metrics
        .state
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let elapsed_us = duration_us(elapsed);
    state
        .admission_latency_total
        .admission_decision
        .observe_us(elapsed_us);
    if rejected {
        state
            .admission_latency_total
            .admission_rejection
            .observe_us(elapsed_us);
    }
    let bucket = state.bucket(metrics.started.elapsed().as_secs());
    bucket.latency.admission_decision.observe_us(elapsed_us);
    if rejected {
        bucket.latency.admission_rejection.observe_us(elapsed_us);
    }
}

pub fn record_body_retention(elapsed: Duration) {
    let metrics = runtime();
    let mut state = metrics
        .state
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let elapsed_us = duration_us(elapsed);
    state
        .admission_latency_total
        .body_retention
        .observe_us(elapsed_us);
    state
        .bucket(metrics.started.elapsed().as_secs())
        .latency
        .body_retention
        .observe_us(elapsed_us);
}

pub(super) fn render_prometheus(output: &mut String) {
    let metrics = runtime();
    let state = metrics
        .state
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    output.push_str("# TYPE gateway_diagnostic_recording_failures_total counter\n");
    for (destination, count) in [
        ("usage_events", state.recording_failures_total.usage_events),
        (
            "debug_bundles",
            state.recording_failures_total.debug_bundles,
        ),
        (
            "request_traffic",
            state.recording_failures_total.request_traffic,
        ),
    ] {
        output.push_str(&format!("gateway_diagnostic_recording_failures_total{{destination=\"{destination}\"}} {count}\n"));
    }
    for (name, histogram) in [
        (
            "gateway_body_admission_decision_duration_ms",
            &state.admission_latency_total.admission_decision,
        ),
        (
            "gateway_body_admission_rejection_duration_ms",
            &state.admission_latency_total.admission_rejection,
        ),
        (
            "gateway_body_retention_duration_ms",
            &state.admission_latency_total.body_retention,
        ),
    ] {
        output.push_str(&format!("# TYPE {name} histogram\n"));
        let mut cumulative = 0_u64;
        for (bound, count) in HISTOGRAM_BOUNDS_US.iter().zip(histogram.bucket_counts) {
            cumulative = cumulative.saturating_add(count);
            output.push_str(&format!(
                "{name}_bucket{{le=\"{}\"}} {cumulative}\n",
                *bound as f64 / 1_000.0
            ));
        }
        output.push_str(&format!(
            "{name}_bucket{{le=\"+Inf\"}} {}\n{name}_count {}\n{name}_sum {}\n",
            histogram.sample_count,
            histogram.sample_count,
            histogram.sum_us as f64 / 1_000.0
        ));
    }
    if let Some((requests, bytes)) = state.body_limits {
        output.push_str(&format!("# TYPE gateway_body_admission_max_requests gauge\ngateway_body_admission_max_requests {requests}\n# TYPE gateway_body_admission_max_bytes gauge\ngateway_body_admission_max_bytes {bytes}\n"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recording_failure_destinations_are_fixed_and_merge_without_identifiers() {
        let before = runtime_snapshot(60);
        record_recording_failure("usage_events");
        record_recording_failure("debug_bundles");
        record_recording_failure("request_traffic");
        record_recording_failure("unbounded destination");
        let after = runtime_snapshot(60);
        assert_eq!(
            after.recording_failures.usage_events,
            before.recording_failures.usage_events + 1
        );
        assert_eq!(
            after.recording_failures_total.debug_bundles,
            before.recording_failures_total.debug_bundles + 1
        );
        assert_eq!(
            after.recording_failures.request_traffic,
            before.recording_failures.request_traffic + 1
        );
        let mut prometheus = String::new();
        render_prometheus(&mut prometheus);
        assert!(prometheus
            .contains("gateway_diagnostic_recording_failures_total{destination=\"usage_events\"}"));
        assert!(!prometheus.contains("unbounded destination"));
        let mut failures = RecordingFailures::default();
        for destination in ["usage_events", "debug_bundles", "request_traffic"] {
            failures.merge(&RecordingFailures::for_destination(destination).unwrap());
        }
        assert_eq!(failures.usage_events, 1);
        assert_eq!(failures.debug_bundles, 1);
        assert_eq!(failures.request_traffic, 1);
        assert!(RecordingFailures::for_destination("unbounded destination").is_none());
        let mut snapshot = empty_snapshot(60);
        snapshot.instance_count = 1;
        snapshot.recording_failures = failures.clone();
        snapshot.recording_failures_total = failures;
        let merged = merge_runtime_snapshots(&[snapshot.clone(), snapshot]);
        assert_eq!(merged.recording_failures.usage_events, 2);
        assert_eq!(merged.recording_failures_total.debug_bundles, 2);
    }

    #[test]
    fn terminal_accounting_is_idempotent_and_does_not_require_usage_storage() {
        let metrics = Arc::new(RuntimeMetrics::new());
        let mut rejected = RuntimeRequest::start(Arc::clone(&metrics), RuntimeTransport::Http);
        let outcome = RuntimeOutcome {
            rejected: true,
            ..RuntimeOutcome::default()
        };
        assert!(rejected.finish(outcome.clone()));
        assert!(!rejected.finish(outcome));
        drop(rejected);
        let dropped = RuntimeRequest::start(Arc::clone(&metrics), RuntimeTransport::Http);
        drop(dropped);
        let snapshot = metrics.snapshot(60);
        assert_eq!(snapshot.counters.started, 2);
        assert_eq!(snapshot.counters.completed, 2);
        assert_eq!(snapshot.counters.failed, 2);
        assert_eq!(snapshot.counters.rejected, 1);
        assert_eq!(snapshot.counters.unattributed, 2);
        assert_eq!(snapshot.active, RuntimeActive::default());
        assert_eq!(snapshot.latency.http_success_duration.sample_count, 0);
    }

    #[test]
    fn websocket_sessions_and_sse_token_samples_do_not_pollute_http_latency() {
        let metrics = Arc::new(RuntimeMetrics::new());
        let mut socket = RuntimeRequest::start(Arc::clone(&metrics), RuntimeTransport::Http);
        socket.set_transport(RuntimeTransport::WebSocket);
        assert_eq!(metrics.snapshot(60).active.websocket, 1);
        socket.finish(RuntimeOutcome {
            success: true,
            attributed: true,
            websocket_handshake_ms: Some(25),
            websocket_session_ms: Some(120_000),
            ttfb_ms: Some(99),
            ..RuntimeOutcome::default()
        });
        let mut sse = RuntimeRequest::start(Arc::clone(&metrics), RuntimeTransport::Sse);
        sse.finish(RuntimeOutcome {
            success: false, // e.g. interrupted after sending HTTP 200
            attributed: true,
            upstream_duration_ms: Some(100),
            ttfb_ms: Some(0),
            ttft_ms: Some(75),
            ..RuntimeOutcome::default()
        });
        let snapshot = metrics.snapshot(60);
        assert_eq!(snapshot.counters.started, 2);
        assert_eq!(snapshot.counters.succeeded, 1);
        assert_eq!(snapshot.counters.failed, 1);
        assert_eq!(snapshot.latency.http_duration.sample_count, 1);
        assert_eq!(snapshot.latency.http_success_duration.sample_count, 0);
        assert_eq!(snapshot.latency.websocket_handshake.p50_ms, Some(25.0));
        assert_eq!(snapshot.latency.websocket_session.p50_ms, Some(120_000.0));
        assert_eq!(snapshot.latency.ttfb.sample_count, 1);
        assert_eq!(snapshot.latency.ttft.p50_ms, Some(100.0));
        assert_eq!(snapshot.active, RuntimeActive::default());
    }

    #[test]
    fn windows_expire_at_the_boundary_and_retention_is_bounded() {
        let metrics = RuntimeMetrics {
            started: Instant::now() - Duration::from_secs(900),
            state: Mutex::new(State::default()),
        };
        {
            let mut state = metrics.state.lock().unwrap();
            for second in 0..=900 {
                state.bucket(second).counters.completed = 1;
            }
            assert_eq!(state.buckets.len(), 900);
        }
        assert_eq!(metrics.snapshot(60).counters.completed, 60);
        assert_eq!(metrics.snapshot(300).counters.completed, 300);
        assert_eq!(metrics.snapshot(900).counters.completed, 900);
        assert_eq!(metrics.snapshot(u64::MAX).window_seconds, 60);
    }

    #[test]
    fn history_aggregation_allows_request_updates_without_copying_history() {
        let metrics = Arc::new(RuntimeMetrics {
            started: Instant::now() - Duration::from_secs(RETAIN_SECONDS),
            state: Mutex::new(State::default()),
        });
        {
            let mut state = metrics.state.lock().unwrap();
            for second in 1..=RETAIN_SECONDS {
                let bucket = state.bucket(second);
                bucket.counters.completed = 1;
                bucket.latency.http_success_duration.observe_us(1_000);
            }
            state.totals.completed = RETAIN_SECONDS;
        }

        let (mut snapshot, buckets) = metrics.snapshot_state(900);
        let historical = Arc::clone(buckets.front().unwrap());
        let current = Arc::clone(buckets.back().unwrap());
        let mut updates = 0;
        aggregate_buckets(
            &mut snapshot,
            buckets.into_iter().inspect(|_| {
                assert!(
                    metrics.state.try_lock().is_ok(),
                    "history aggregation must leave the request-state mutex available"
                );
                let mut request =
                    RuntimeRequest::start(Arc::clone(&metrics), RuntimeTransport::Http);
                let outcome = RuntimeOutcome {
                    success: true,
                    attributed: true,
                    ..RuntimeOutcome::default()
                };
                assert!(request.finish(outcome.clone()));
                assert!(!request.finish(outcome));
                updates += 1;
            }),
        );

        assert_eq!(updates, RETAIN_SECONDS);
        assert_eq!(snapshot.counters.completed, RETAIN_SECONDS);
        assert_eq!(snapshot.totals.completed, RETAIN_SECONDS);
        assert_eq!(snapshot.latency.http_success_duration.sample_count, 900);
        assert_eq!(snapshot.latency.http_success_duration.p95_ms, Some(1.0));
        assert_eq!(current.counters.completed, 1);
        assert_eq!(current.latency.http_success_duration.sample_count, 1);
        let mut state = metrics.state.try_lock().unwrap();
        assert_eq!(state.totals.completed, RETAIN_SECONDS + updates);
        assert_eq!(state.active, RuntimeActive::default());
        assert_eq!(state.buckets.len(), RETAIN_SECONDS as usize);
        assert!(Arc::ptr_eq(state.buckets.front().unwrap(), &historical));
        assert!(!Arc::ptr_eq(state.buckets.back().unwrap(), &current));
        assert_eq!(Arc::strong_count(state.buckets.back().unwrap()), 1);
        for second in (RETAIN_SECONDS + 1)..=(RETAIN_SECONDS * 2) {
            state.bucket(second);
            assert_eq!(state.buckets.len(), RETAIN_SECONDS as usize);
        }
        assert_eq!(historical.counters.completed, 1);
    }

    #[test]
    fn cluster_quantiles_merge_buckets_and_survive_serialization() {
        let mut fast = empty_snapshot(300);
        fast.instance_count = 1;
        fast.body_capacity = Some(BodyCapacitySnapshot::new(8, 1, 100, 10));
        for _ in 0..99 {
            fast.latency.http_success_duration.observe_us(1_000);
        }
        fast.latency.refresh_quantiles();
        let mut slow = empty_snapshot(300);
        slow.instance_count = 1;
        slow.body_capacity = Some(BodyCapacitySnapshot::new(8, 2, 100, 20));
        slow.latency.http_success_duration.observe_us(1_000_000);
        slow.latency.refresh_quantiles();
        let json = serde_json::to_vec(&fast).unwrap();
        assert!(json.len() < 20_000, "fixed-size full snapshot");
        let decoded: RuntimeSnapshot = serde_json::from_slice(&json).unwrap();
        assert_eq!(decoded, fast);
        let merged = merge_runtime_snapshots(&[decoded, slow]);
        assert_eq!(merged.instance_count, 2);
        assert_eq!(merged.latency.http_success_duration.sample_count, 100);
        assert_eq!(merged.latency.http_success_duration.p95_ms, Some(1.0));
        assert_eq!(merged.latency.http_success_duration.p99_ms, Some(1.0));
        assert_eq!(merged.body_capacity.unwrap().free_bytes, 170);
        assert_eq!(merged.uptime_seconds, None);
        assert_eq!(merged.process.rss_bytes, None);
    }

    #[test]
    fn incompatible_buckets_are_reported_and_overflow_is_not_fabricated() {
        let mut valid = empty_snapshot(60);
        valid.instance_count = 1;
        valid.latency.ttft.observe_us(u64::MAX);
        valid.latency.refresh_quantiles();
        assert_eq!(valid.latency.ttft.sample_count, 1);
        assert_eq!(valid.latency.ttft.p99_ms, None);
        let mut incompatible = valid.clone();
        incompatible.latency.ttft.upper_bounds_us[0] = 1;
        let mut wrong_window = valid.clone();
        wrong_window.window_seconds = 900;
        let merged = merge_runtime_snapshots(&[valid, incompatible, wrong_window]);
        assert_eq!(merged.instance_count, 1);
        assert_eq!(merged.incompatible_instances, 2);
        assert_eq!(merged.latency.ttft.sample_count, 1);
    }
}
