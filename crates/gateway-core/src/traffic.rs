//! Bounded metadata-only diagnostics, independent of billing identity and storage.
use crate::{GatewayError, GatewayResult};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::{
    borrow::Borrow,
    collections::{HashMap, HashSet, VecDeque},
    sync::{
        atomic::{AtomicU8, Ordering},
        Arc, Mutex, OnceLock,
    },
    time::{Duration, Instant},
};
use uuid::Uuid;

const JOURNAL_CAPACITY: usize = 512;
const TIMELINE_CAPACITY: usize = 32;
const ACTIVE_RECORD_LIMIT: usize = 2048;
const ACTIVE_BYTE_LIMIT: usize = 2 * 1024 * 1024;
const SOCKET_SAMPLE_INTERVAL: Duration = Duration::from_millis(500);
// Reserve space for cumulative counters and activity timestamps so frame hooks
// can coalesce fixed-size metadata without per-frame serialization.
const SOCKET_PROGRESS_RESERVE: usize = 512;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RequestDiagnostics {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authentication_profile: Option<AuthenticationProfileDiagnostics>,
    #[serde(default)]
    pub protocol: Option<String>,
    #[serde(default)]
    pub websocket: Option<WebSocketDiagnostics>,
    #[serde(default)]
    pub entra: Option<EntraDiagnostics>,
    /// Actual request mode; absent for legacy records or unresolved routing.
    #[serde(default)]
    pub routing_mode: Option<String>,
    #[serde(default)]
    pub traffic_id: Option<Uuid>,
    pub failure_stage: Option<String>,
    pub failure_code: Option<String>,
    pub failure_source: Option<String>,
    pub outcome: Option<String>,
    pub upstream_status: Option<u16>,
    pub instance_id: Option<String>,
}

/// Historical metadata, copied from validated configuration; never caller hints.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct AuthenticationProfileDiagnostics {
    pub id: Option<String>,
    pub name: Option<String>,
    pub revision: u64,
    pub authentication_type: Option<String>,
    pub binding_source: String,
    pub outcome: String,
}

/// Metadata observed at proxy frame hooks; counts exclude HTTP headers and TLS overhead.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct WebSocketDiagnostics {
    pub state: String,
    pub app: Option<String>,
    pub channel: String,
    pub opened_at: Option<DateTime<Utc>>,
    pub observed_at: Option<DateTime<Utc>>,
    pub closed_at: Option<DateTime<Utc>>,
    pub duration_ms: u64,
    pub client_bytes: u64,
    pub upstream_bytes: u64,
    pub client_frames: u64,
    pub upstream_frames: u64,
    pub client_close_frame: bool,
    pub upstream_close_frame: bool,
    pub last_client_activity_at: Option<DateTime<Utc>>,
    pub last_upstream_activity_at: Option<DateTime<Utc>>,
    pub idle_timeout_ms: u64,
    pub session_expires_at: Option<DateTime<Utc>>,
    pub max_frame_bytes: u64,
    pub close_cause: Option<String>,
}

/// Bounded allowlist, populated only after successful identity verification.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct EntraDiagnostics {
    pub policy_source: String,
    pub verification: String,
    pub expected_audience: Option<String>,
    pub required_scopes: Vec<String>,
    pub required_roles: Vec<String>,
    pub allowed_groups: Vec<String>,
    pub allow_apigee: bool,
    pub source: Option<String>,
    pub tenant_id: Option<String>,
    pub object_id: Option<String>,
    pub app_id: Option<String>,
    pub authorized_party: Option<String>,
    pub token_version: Option<String>,
    pub expires_at: Option<i64>,
    pub audiences: Vec<String>,
    pub scopes: Vec<String>,
    pub roles: Vec<String>,
    pub groups: Vec<String>,
    pub truncated: bool,
}

impl EntraDiagnostics {
    fn bounded(value: &str, truncated: &mut bool) -> String {
        *truncated |= value.chars().count() > 256;
        value.chars().take(256).collect()
    }

    fn claims(values: &[String], truncated: &mut bool) -> Vec<String> {
        *truncated |= values.len() > 32;
        values
            .iter()
            .take(32)
            .map(|v| Self::bounded(v, truncated))
            .collect()
    }

    pub fn endpoint(policy: &crate::EndpointEntraPolicy) -> Self {
        let mut result = Self {
            policy_source: "endpoint".into(),
            verification: "pending".into(),
            allow_apigee: policy.allow_apigee,
            ..Self::default()
        };
        result.expected_audience = Some(Self::bounded(&policy.audience, &mut result.truncated));
        result.required_scopes = Self::claims(&policy.required_scopes, &mut result.truncated);
        result.required_roles = Self::claims(&policy.required_roles, &mut result.truncated);
        result.allowed_groups = Self::claims(&policy.allowed_groups, &mut result.truncated);
        result
    }

    pub fn verified(&mut self, identity: &crate::EntraIdentityContext) {
        self.verification = "verified".into();
        self.source = Some(
            match identity.source {
                crate::EntraIdentitySource::Jwt => "jwt",
                crate::EntraIdentitySource::ApigeeTrustedHeader => "signed_apigee",
            }
            .into(),
        );
        self.tenant_id = Some(Self::bounded(&identity.tenant_id, &mut self.truncated));
        self.object_id = identity
            .object_id
            .as_deref()
            .map(|v| Self::bounded(v, &mut self.truncated));
        self.app_id = identity
            .app_id
            .as_deref()
            .map(|v| Self::bounded(v, &mut self.truncated));
        self.authorized_party = identity
            .authorized_party
            .as_deref()
            .map(|v| Self::bounded(v, &mut self.truncated));
        self.token_version = Some(Self::bounded(&identity.token_version, &mut self.truncated));
        self.expires_at = identity.expires_at;
        self.audiences = Self::claims(&identity.audiences, &mut self.truncated);
        self.scopes = Self::claims(&identity.scopes, &mut self.truncated);
        self.roles = Self::claims(&identity.roles, &mut self.truncated);
        self.groups = Self::claims(&identity.groups, &mut self.truncated);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrafficStep {
    pub stage: String,
    pub elapsed_ms: i64,
    pub code: Option<String>,
    pub upstream_status: Option<u16>,
    pub attempt: u32,
}

/// Per-attempt durations. None means unmeasured, never an assumed zero.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct UpstreamTiming {
    pub attempt: u32,
    pub provider: Option<String>,
    pub started_elapsed_ms: i64,
    pub dns_status: String,
    pub dns_us: Option<u64>,
    pub connection_reused: Option<bool>,
    pub tls: bool,
    pub tcp_connect_us: Option<u64>,
    pub tls_handshake_us: Option<u64>,
    /// Milestones are relative to this attempt's start, including DNS/connect.
    pub response_headers_ms: Option<i64>,
    pub first_body_byte_ms: Option<i64>,
    pub first_token_ms: Option<i64>,
    pub total_ms: Option<i64>,
    pub upstream_status: Option<u16>,
    pub failure_code: Option<String>,
}

/// Allowlisted usage metadata; payloads and credentials never enter Traffic.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct TrafficUsage {
    pub model: Option<String>,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
    pub estimated_cost_usd: Option<f64>,
    pub cost_source: Option<String>,
    pub pricing_rule_name: Option<String>,
    pub service_version: Option<String>,
    pub trace_id: Option<String>,
    pub task_id: Option<String>,
    pub run_id: Option<String>,
}

impl From<&crate::UsageEvent> for TrafficUsage {
    fn from(event: &crate::UsageEvent) -> Self {
        Self {
            model: event.model.clone(),
            input_tokens: event.input_tokens,
            output_tokens: event.output_tokens,
            total_tokens: event.total_tokens,
            estimated_cost_usd: event.estimated_cost_usd,
            cost_source: event.cost_source.clone(),
            pricing_rule_name: event.pricing_rule_name.clone(),
            service_version: event.service_version.clone(),
            trace_id: event.trace_id.clone(),
            task_id: event.task_id.clone(),
            run_id: event.run_id.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrafficRequest {
    /// Process-local lifecycle identity, shared by clones; never persisted.
    #[serde(skip)]
    pub monitor_state: Arc<AtomicU8>,
    #[serde(skip)]
    pub monitor_sampled_at: Arc<Mutex<Option<Instant>>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<DateTime<Utc>>,
    /// Active registry projections deliberately omit investigation details.
    #[serde(default)]
    pub summary_only: bool,
    #[serde(default)]
    pub upstream_timings: Vec<UpstreamTiming>,
    #[serde(default)]
    pub usage: Option<TrafficUsage>,
    #[serde(default)]
    pub debug_bundle: Option<crate::DebugBundle>,
    #[serde(default)]
    pub key_prefix: Option<String>,
    /// Internal identity prevents clients reusing request IDs from overwriting records.
    pub id: Uuid,
    pub request_id: String,
    pub instance_id: String,
    pub started_at: DateTime<Utc>,
    pub elapsed_ms: i64,
    pub method: String,
    /// Route template only. Raw paths may contain credentials or personal data.
    pub endpoint: Option<String>,
    pub service: Option<String>,
    pub provider: Option<String>,
    pub key_id: Option<Uuid>,
    pub project_id: Option<Uuid>,
    pub stage: String,
    pub client_status: Option<u16>,
    pub attempts: u32,
    pub streaming: bool,
    pub completed: bool,
    pub diagnostics: RequestDiagnostics,
    pub timeline: Vec<TrafficStep>,
    pub timeline_truncated: bool,
    pub recording_failures: Vec<String>,
}

impl Default for TrafficRequest {
    fn default() -> Self {
        Self {
            monitor_state: Arc::default(),
            monitor_sampled_at: Arc::default(),
            completed_at: None,
            summary_only: false,
            upstream_timings: Vec::new(),
            usage: None,
            debug_bundle: None,
            key_prefix: None,
            id: Uuid::new_v4(),
            request_id: String::new(),
            instance_id: monitor().instance_id.clone(),
            started_at: Utc::now(),
            elapsed_ms: 0,
            method: String::new(),
            endpoint: None,
            service: None,
            provider: None,
            key_id: None,
            project_id: None,
            stage: "received".into(),
            client_status: None,
            attempts: 0,
            streaming: false,
            completed: false,
            diagnostics: RequestDiagnostics::default(),
            timeline: Vec::new(),
            timeline_truncated: false,
            recording_failures: Vec::new(),
        }
    }
}

impl TrafficRequest {
    pub fn protocol(&self) -> &str {
        match self.diagnostics.protocol.as_deref() {
            Some("websocket") => "websocket",
            Some("http") => "http",
            _ if self.diagnostics.websocket.is_some() => "websocket",
            _ => "unknown",
        }
    }

    pub fn step(&mut self, stage: &str, elapsed_ms: i64, code: Option<&str>) {
        self.stage = stage.into();
        self.elapsed_ms = elapsed_ms;
        if self.timeline.len() == TIMELINE_CAPACITY {
            // Preserve arrival and the most recent steps.
            self.timeline.remove(1);
            self.timeline_truncated = true;
        }
        self.timeline.push(TrafficStep {
            stage: stage.into(),
            elapsed_ms,
            code: code.map(str::to_owned),
            upstream_status: self.diagnostics.upstream_status,
            attempt: self.attempts,
        });
        monitor().publish(self.clone());
    }

    pub fn fail(&mut self, source: &str, code: &str) {
        if self.diagnostics.failure_code.is_none() {
            self.diagnostics.failure_stage = Some(self.stage.clone());
            self.diagnostics.failure_code = Some(code.into());
            self.diagnostics.failure_source = Some(source.into());
        }
    }

    pub fn recording_failed(&mut self, destination: &str) {
        if !self
            .recording_failures
            .iter()
            .any(|value| value == destination)
        {
            self.recording_failures.push(destination.into());
            gateway_telemetry::runtime::record_recording_failure(destination);
        }
        // Only fixed destination names and internal IDs; never raw database errors.
        tracing::error!(request_id = %self.request_id, diagnostic_id = %self.id,
            instance_id = %self.instance_id, destination, "gateway diagnostic recording failed");
        monitor().publish(self.clone());
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct TrafficQuery {
    /// Opt-in live-feed registry projection; ignored by saved history.
    pub snapshot: Option<bool>,
    pub id: Option<Uuid>,
    pub request_id: Option<String>,
    pub service: Option<String>,
    pub project_id: Option<Uuid>,
    pub key_id: Option<Uuid>,
    pub status: Option<u16>,
    pub failures_only: Option<bool>,
    pub failure_code: Option<String>,
    pub from: Option<DateTime<Utc>>,
    pub to: Option<DateTime<Utc>>,
    pub before: Option<DateTime<Utc>>,
    pub before_id: Option<Uuid>,
    pub limit: Option<i64>,
    /// Omitted preserves the released start-time history window and cursor.
    pub time_basis: Option<String>,
    pub protocol: Option<String>,
}

impl TrafficQuery {
    pub fn validate(&self) -> GatewayResult<()> {
        if self.limit.is_some_and(|v| !(1..=200).contains(&v))
            || self.status.is_some_and(|v| !(100..=599).contains(&v))
            || self.from.zip(self.to).is_some_and(|(from, to)| from > to)
            || self.before.is_some() != self.before_id.is_some()
            || self.request_id.as_ref().is_some_and(|v| v.len() > 128)
            || self.failure_code.as_ref().is_some_and(|v| v.len() > 80)
            || self.service.as_ref().is_some_and(|v| v.len() > 256)
            || self
                .time_basis
                .as_deref()
                .is_some_and(|v| !matches!(v, "started" | "completed"))
            || self
                .protocol
                .as_deref()
                .is_some_and(|v| !matches!(v, "http" | "websocket" | "unknown"))
        {
            return Err(GatewayError::InvalidUsageQuery);
        }
        Ok(())
    }

    /// Live request IDs retain substring matching; history retains exact matching.
    pub fn matches_active(&self, row: &TrafficRequest) -> bool {
        self.id.is_none_or(|v| v == row.id)
            && self
                .request_id
                .as_ref()
                .is_none_or(|v| row.request_id.contains(v))
            && self
                .service
                .as_ref()
                .is_none_or(|v| row.service.as_ref() == Some(v))
            && self.project_id.is_none_or(|v| row.project_id == Some(v))
            && self.key_id.is_none_or(|v| row.key_id == Some(v))
            && self.status.is_none_or(|v| row.client_status == Some(v))
            && self
                .failure_code
                .as_ref()
                .is_none_or(|v| row.diagnostics.failure_code.as_ref() == Some(v))
            && (self.failures_only != Some(true) || row.diagnostics.failure_code.is_some())
            && self.protocol.as_ref().is_none_or(|v| row.protocol() == v)
            && self.from.is_none_or(|v| row.started_at >= v)
            && self.to.is_none_or(|v| row.started_at <= v)
            && self
                .before
                .zip(self.before_id)
                .is_none_or(|v| (row.started_at, row.id) < v)
    }

    fn has_row_filters(&self) -> bool {
        self.id.is_some()
            || self.request_id.is_some()
            || self.service.is_some()
            || self.project_id.is_some()
            || self.key_id.is_some()
            || self.status.is_some()
            || self.failure_code.is_some()
            || self.failures_only == Some(true)
            || self.from.is_some()
            || self.to.is_some()
            || self.before.is_some()
    }
}

#[async_trait]
pub trait TrafficStore: Send + Sync {
    async fn insert_traffic(&self, request: &TrafficRequest) -> GatewayResult<()>;
    async fn traffic_history(&self, query: TrafficQuery) -> GatewayResult<Vec<TrafficRequest>>;
}

/// Exact unfiltered counts are independent of retained summary rows.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TrafficActiveCounts {
    pub http: u64,
    pub websocket: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrafficCoverage {
    pub total: u64,
    pub retained: usize,
    /// Serialized row bytes, including reserved space for coalesced counters.
    pub retained_bytes: usize,
    pub record_limit: usize,
    pub byte_limit: usize,
    pub saturated: bool,
    /// None means that missing summaries prevent an exact filtered count.
    pub matching: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrafficSummarySnapshot {
    pub instance_id: String,
    pub sampled_at: DateTime<Utc>,
    pub counts: TrafficActiveCounts,
    pub http: TrafficCoverage,
    pub websocket: TrafficCoverage,
}

/// A bounded metadata projection suitable for cluster publication. Counts apply
/// to this process; row filters are evaluated before each protocol's page limit.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrafficActiveSnapshot {
    #[serde(flatten)]
    pub summary: TrafficSummarySnapshot,
    pub rows: Vec<TrafficRequest>,
    pub has_more: bool,
    pub truncated: bool,
}

#[derive(Debug)]
struct ActivePartition {
    rows: HashMap<Uuid, (TrafficRequest, usize)>,
    bytes: usize,
    total: u64,
    record_limit: usize,
    byte_limit: usize,
}

impl ActivePartition {
    fn new(record_limit: usize, byte_limit: usize) -> Self {
        Self {
            rows: HashMap::new(),
            bytes: 0,
            total: 0,
            record_limit,
            byte_limit,
        }
    }

    fn remove(&mut self, id: &Uuid) {
        if let Some((_, size)) = self.rows.remove(id) {
            self.bytes = self.bytes.saturating_sub(size);
        }
    }

    fn update(&mut self, request: &TrafficRequest) {
        let row = active_projection(request);
        let size = serde_json::to_vec(&row)
            .map_or(usize::MAX, |json| json.len())
            .saturating_add(if row.diagnostics.websocket.is_some() {
                SOCKET_PROGRESS_RESERVE
            } else {
                0
            });
        let previous = self.rows.get(&row.id).map_or(0, |(_, size)| *size);
        if (self.rows.contains_key(&row.id) || self.rows.len() < self.record_limit)
            && self.bytes.saturating_sub(previous).saturating_add(size) <= self.byte_limit
        {
            self.bytes = self.bytes.saturating_sub(previous).saturating_add(size);
            self.rows.insert(row.id, (row, size));
        } else {
            // A growing row must not leave a misleading stale projection behind.
            self.remove(&request.id);
        }
    }

    fn coverage(&self) -> TrafficCoverage {
        TrafficCoverage {
            total: self.total,
            retained: self.rows.len(),
            retained_bytes: self.bytes,
            record_limit: self.record_limit,
            byte_limit: self.byte_limit,
            saturated: self.total > self.rows.len() as u64,
            matching: Some(self.total),
        }
    }
}

/// The registry stores only this allowlist, never identity claims, payloads,
/// timelines, usage/debug snapshots or raw transport errors.
fn active_projection(request: &TrafficRequest) -> TrafficRequest {
    let bounded = |s: &str| s.chars().take(256).collect::<String>();
    let optional = |s: &Option<String>| s.as_deref().map(bounded);
    let mut socket = request.diagnostics.websocket.clone();
    if let Some(socket) = &mut socket {
        socket.state = bounded(&socket.state);
        socket.app = optional(&socket.app);
        socket.channel = bounded(&socket.channel);
        socket.close_cause = optional(&socket.close_cause);
    }
    TrafficRequest {
        monitor_state: request.monitor_state.clone(),
        monitor_sampled_at: request.monitor_sampled_at.clone(),
        completed_at: request.completed_at,
        summary_only: true,
        upstream_timings: Vec::new(),
        usage: None,
        debug_bundle: None,
        key_prefix: optional(&request.key_prefix),
        id: request.id,
        request_id: bounded(&request.request_id),
        instance_id: bounded(&request.instance_id),
        started_at: request.started_at,
        elapsed_ms: request.elapsed_ms,
        method: bounded(&request.method),
        endpoint: optional(&request.endpoint),
        service: optional(&request.service),
        provider: optional(&request.provider),
        key_id: request.key_id,
        project_id: request.project_id,
        stage: bounded(&request.stage),
        client_status: request.client_status,
        attempts: request.attempts,
        streaming: request.streaming,
        completed: request.completed,
        diagnostics: RequestDiagnostics {
            protocol: request.diagnostics.protocol.as_deref().map(bounded),
            websocket: socket,
            routing_mode: optional(&request.diagnostics.routing_mode),
            traffic_id: request.diagnostics.traffic_id,
            failure_stage: optional(&request.diagnostics.failure_stage),
            failure_code: optional(&request.diagnostics.failure_code),
            failure_source: optional(&request.diagnostics.failure_source),
            outcome: optional(&request.diagnostics.outcome),
            upstream_status: request.diagnostics.upstream_status,
            instance_id: Some(bounded(&request.instance_id)),
            ..Default::default()
        },
        timeline: Vec::new(),
        timeline_truncated: request.timeline_truncated,
        recording_failures: request
            .recording_failures
            .iter()
            .take(3)
            .map(|s| bounded(s))
            .collect(),
    }
}

/// Copy only fixed-size cumulative progress. A burst followed by a quiet socket
/// still exposes its final counters, without cloning the timeline on each frame.
fn coalesce_socket_progress(target: &mut TrafficRequest, source: &TrafficRequest) {
    target.elapsed_ms = source.elapsed_ms;
    if let Some((target, source)) = target
        .diagnostics
        .websocket
        .as_mut()
        .zip(source.diagnostics.websocket.as_ref())
    {
        target.observed_at = source.observed_at;
        target.duration_ms = source.duration_ms;
        target.client_bytes = source.client_bytes;
        target.upstream_bytes = source.upstream_bytes;
        target.client_frames = source.client_frames;
        target.upstream_frames = source.upstream_frames;
        target.client_close_frame = source.client_close_frame;
        target.upstream_close_frame = source.upstream_close_frame;
        target.last_client_activity_at = source.last_client_activity_at;
        target.last_upstream_activity_at = source.last_upstream_activity_at;
    }
}

#[derive(Debug)]
struct MonitorState {
    sequence: u64,
    journal: VecDeque<(u64, TrafficRequest)>,
    socket_updates: HashMap<Uuid, (Instant, TrafficRequest)>,
    http: ActivePartition,
    websocket: ActivePartition,
}

pub struct TrafficMonitor {
    pub instance_id: String,
    state: Arc<Mutex<MonitorState>>,
    capacity: usize,
}

/// Context-owned cleanup, independent of journal clones. Aborted tasks must not
/// leave immortal active rows or gauges when terminal logging cannot run.
#[derive(Debug)]
pub struct TrafficActiveLease {
    id: Uuid,
    lifecycle: Arc<AtomicU8>,
    state: Arc<Mutex<MonitorState>>,
}

impl Drop for TrafficActiveLease {
    fn drop(&mut self) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        match self.lifecycle.swap(3, Ordering::Relaxed) {
            1 => state.http.total = state.http.total.saturating_sub(1),
            2 => state.websocket.total = state.websocket.total.saturating_sub(1),
            _ => return,
        }
        state.http.remove(&self.id);
        state.websocket.remove(&self.id);
        state.socket_updates.remove(&self.id);
    }
}

#[derive(Serialize)]
pub struct TrafficBatch {
    pub instance_id: String,
    pub cursor: String,
    pub gap: bool,
    pub evicted_updates: u64,
    pub rows: Vec<TrafficRequest>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active: Option<TrafficActiveSnapshot>,
}

impl TrafficMonitor {
    pub fn new(capacity: usize) -> Self {
        Self::with_active_limits(capacity, ACTIVE_RECORD_LIMIT, ACTIVE_BYTE_LIMIT)
    }

    fn with_active_limits(capacity: usize, records: usize, bytes: usize) -> Self {
        Self {
            instance_id: Uuid::new_v4().to_string(),
            state: Arc::new(Mutex::new(MonitorState {
                sequence: 0,
                journal: VecDeque::new(),
                socket_updates: HashMap::new(),
                http: ActivePartition::new(records, bytes),
                websocket: ActivePartition::new(records, bytes),
            })),
            capacity: capacity.max(1),
        }
    }

    pub fn active_lease(&self, request: &TrafficRequest) -> TrafficActiveLease {
        TrafficActiveLease {
            id: request.id,
            lifecycle: request.monitor_state.clone(),
            state: self.state.clone(),
        }
    }

    // The shared lifecycle marker makes counts exact even when row budgets fill,
    // without an unbounded set of IDs. Late cloned socket samples cannot reopen
    // a completed request. Only this monitor's serialized lifecycle path writes it.
    fn observe(state: &mut MonitorState, request: &TrafficRequest) -> bool {
        let previous = request.monitor_state.load(Ordering::Relaxed);
        if previous == 3 {
            return false;
        }
        let protocol = if request.protocol() == "websocket" {
            2
        } else {
            1
        };
        let next = if request.completed { 3 } else { protocol };
        if previous != next {
            if previous == 1 {
                state.http.total = state.http.total.saturating_sub(1);
            }
            if previous == 2 {
                state.websocket.total = state.websocket.total.saturating_sub(1);
            }
            if next == 1 {
                state.http.total = state.http.total.saturating_add(1);
            }
            if next == 2 {
                state.websocket.total = state.websocket.total.saturating_add(1);
            }
            request.monitor_state.store(next, Ordering::Relaxed);
        }
        if request.completed || protocol != 1 {
            state.http.remove(&request.id);
        }
        if request.completed || protocol != 2 {
            state.websocket.remove(&request.id);
        }
        if !request.completed {
            if protocol == 2 {
                state.websocket.update(request);
            } else {
                state.http.update(request);
            }
        }
        true
    }

    /// Accept a reference so frame hooks never clone the full record before
    /// throttling. The latest cumulative sample is retained at most twice/sec.
    pub fn publish_socket_progress(&self, request: impl Borrow<TrafficRequest>) {
        let request = request.borrow();
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if request.completed || request.monitor_state.load(Ordering::Relaxed) == 3 {
            return;
        }
        if let Some((row, _)) = state.websocket.rows.get_mut(&request.id) {
            coalesce_socket_progress(row, request);
        }
        if let Some((_, row)) = state.socket_updates.get_mut(&request.id) {
            coalesce_socket_progress(row, request);
        }
        {
            let mut last = request
                .monitor_sampled_at
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if last.is_some_and(|at| at.elapsed() < SOCKET_SAMPLE_INTERVAL) {
                return;
            }
            *last = Some(Instant::now());
        }
        if !Self::observe(&mut state, request) {
            return;
        }
        if state.socket_updates.len() < self.capacity
            || state.socket_updates.contains_key(&request.id)
        {
            state
                .socket_updates
                .insert(request.id, (Instant::now(), request.clone()));
        }
    }

    pub fn publish(&self, request: TrafficRequest) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.socket_updates.remove(&request.id);
        // Subsequent terminal publications carry recording results and usage.
        if !Self::observe(&mut state, &request) && !request.completed {
            return;
        }
        Self::append(&mut state, self.capacity, request);
    }

    fn append(state: &mut MonitorState, capacity: usize, request: TrafficRequest) {
        state.sequence += 1;
        let sequence = state.sequence;
        state.journal.push_back((sequence, request));
        while state.journal.len() > capacity {
            state.journal.pop_front();
        }
    }

    pub fn summary_snapshot(&self) -> TrafficSummarySnapshot {
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        TrafficSummarySnapshot {
            instance_id: self.instance_id.clone(),
            sampled_at: Utc::now(),
            counts: TrafficActiveCounts {
                http: state.http.total,
                websocket: state.websocket.total,
            },
            http: state.http.coverage(),
            websocket: state.websocket.coverage(),
        }
    }

    pub fn active_snapshot(&self, query: &TrafficQuery) -> TrafficActiveSnapshot {
        self.active_snapshot_with_limit(query, query.limit.unwrap_or(100).clamp(1, 200) as usize)
    }

    /// All retained projections (at most 4096 rows / 4 MiB of row JSON), for
    /// bounded Redis publication. API readers should use active_snapshot instead.
    pub fn publication_snapshot(&self) -> TrafficActiveSnapshot {
        self.active_snapshot_with_limit(&TrafficQuery::default(), ACTIVE_RECORD_LIMIT)
    }

    fn active_snapshot_with_limit(
        &self,
        query: &TrafficQuery,
        limit: usize,
    ) -> TrafficActiveSnapshot {
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let mut rows = Vec::new();
        let mut has_more = false;
        let mut http = state.http.coverage();
        let mut websocket = state.websocket.coverage();
        for (protocol, partition, coverage) in [
            ("http", &state.http, &mut http),
            ("websocket", &state.websocket, &mut websocket),
        ] {
            let mut matches: Vec<_> = partition
                .rows
                .values()
                .map(|(row, _)| row)
                .filter(|row| query.matches_active(row))
                .collect();
            matches.sort_unstable_by_key(|row| std::cmp::Reverse((row.started_at, row.id)));
            coverage.matching = if coverage.saturated && query.has_row_filters() {
                None
            } else {
                Some(matches.len() as u64)
            };
            // A protocol filter selects a whole partition, including overflow.
            if !query.has_row_filters() {
                coverage.matching =
                    Some(if query.protocol.as_deref().is_none_or(|p| p == protocol) {
                        partition.total
                    } else {
                        0
                    });
            }
            has_more |= matches.len() > limit;
            rows.extend(matches.into_iter().take(limit).cloned());
        }
        TrafficActiveSnapshot {
            truncated: http.saturated || websocket.saturated,
            summary: TrafficSummarySnapshot {
                instance_id: self.instance_id.clone(),
                sampled_at: Utc::now(),
                counts: TrafficActiveCounts {
                    http: state.http.total,
                    websocket: state.websocket.total,
                },
                http,
                websocket,
            },
            rows,
            has_more,
        }
    }

    pub fn batch(&self, cursor: Option<&str>) -> TrafficBatch {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let pending = std::mem::take(&mut state.socket_updates);
        for (_, (_, request)) in pending {
            // Completion removes pending samples while holding the same lock.
            Self::append(&mut state, self.capacity, request);
        }
        let parsed = cursor.and_then(|v| v.rsplit_once(':'));
        let sequence = parsed.and_then(|(instance, seq)| {
            (instance == self.instance_id)
                .then(|| seq.parse::<u64>().ok())
                .flatten()
        });
        let oldest = state
            .journal
            .front()
            .map_or(state.sequence + 1, |(seq, _)| *seq);
        let gap = cursor.is_some()
            && sequence.is_none_or(|seq| seq.saturating_add(1) < oldest || seq > state.sequence);
        let after = if gap { 0 } else { sequence.unwrap_or(0) };
        let mut seen = HashSet::new();
        let rows = state
            .journal
            .iter()
            .rev()
            .filter(|(seq, request)| *seq > after && seen.insert(request.id))
            .map(|(_, request)| request.clone())
            .collect();
        TrafficBatch {
            instance_id: self.instance_id.clone(),
            cursor: format!("{}:{}", self.instance_id, state.sequence),
            gap,
            evicted_updates: oldest.saturating_sub(1),
            rows,
            active: None,
        }
    }
}

pub fn monitor() -> &'static TrafficMonitor {
    static MONITOR: OnceLock<TrafficMonitor> = OnceLock::new();
    MONITOR.get_or_init(|| TrafficMonitor::new(JOURNAL_CAPACITY))
}

/// Accept only bounded correlation tokens; never log arbitrary header contents.
pub fn correlation_id(value: Option<&str>) -> String {
    value
        .filter(|v| {
            !v.is_empty()
                && v.len() <= 128
                && v.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_.:".contains(&b))
        })
        .map(str::to_owned)
        .unwrap_or_else(|| Uuid::new_v4().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identity_snapshot_bounds_claims_and_excludes_private_token_fields() {
        let policy = crate::EndpointEntraPolicy {
            audience: "api://channel".into(),
            required_scopes: vec!["run".into()],
            required_roles: vec![],
            allowed_groups: vec![],
            allow_apigee: false,
        };
        let mut snapshot = EntraDiagnostics::endpoint(&policy);
        assert_eq!(snapshot.verification, "pending");
        assert!(snapshot.audiences.is_empty());
        let identity: crate::EntraIdentityContext = serde_json::from_value(serde_json::json!({
            "tenant_id":"tenant", "audiences":["api://channel"], "expires_at":2000000000,
            "object_id":"user", "app_id":"app", "authorized_party":"party", "subject":"private-subject",
            "email":"private-email", "display_name":"private-name", "nonce":"private-nonce",
            "scopes":["run"], "roles":["role"], "groups":vec!["g".repeat(300); 40],
            "token_version":"2.0", "source":"jwt"
        })).unwrap();
        snapshot.verified(&identity);
        assert_eq!(snapshot.verification, "verified");
        assert_eq!(snapshot.groups.len(), 32);
        assert_eq!(snapshot.groups[0].len(), 256);
        assert!(snapshot.truncated);
        let json = serde_json::to_string(&snapshot).unwrap();
        for private in [
            "private-subject",
            "private-email",
            "private-name",
            "private-nonce",
        ] {
            assert!(!json.contains(private));
        }
        let mut old = serde_json::to_value(RequestDiagnostics::default()).unwrap();
        old.as_object_mut().unwrap().remove("websocket");
        old.as_object_mut().unwrap().remove("entra");
        let old: RequestDiagnostics = serde_json::from_value(old).unwrap();
        assert!(old.websocket.is_none() && old.entra.is_none());
    }

    #[test]
    fn socket_updates_coalesce_flush_and_cannot_overwrite_terminal_records() {
        let monitor = TrafficMonitor::new(2);
        let mut row = TrafficRequest::default();
        for n in 0..100 {
            row.elapsed_ms = n;
            monitor.publish_socket_progress(&row);
        }
        let batch = monitor.batch(None);
        assert_eq!(batch.rows.len(), 1);
        assert_eq!(
            batch.rows[0].elapsed_ms, 99,
            "coalesce progress while throttling full clones"
        );
        assert_eq!(batch.evicted_updates, 0);
        // A live poll must not reset the per-request throttle.
        monitor.publish_socket_progress(&row);
        assert!(monitor.batch(Some(&batch.cursor)).rows.is_empty());
        *row.monitor_sampled_at.lock().unwrap() = Some(Instant::now() - SOCKET_SAMPLE_INTERVAL);
        row.elapsed_ms = 100;
        monitor.publish_socket_progress(&row);
        let late = row.clone();
        row.completed = true;
        monitor.publish(row);
        monitor.publish_socket_progress(late);
        let batch = monitor.batch(Some(&batch.cursor));
        assert_eq!(batch.rows.len(), 1);
        assert!(batch.rows[0].completed);
        for _ in 0..10 {
            monitor.publish_socket_progress(TrafficRequest::default());
        }
        assert_eq!(monitor.state.lock().unwrap().socket_updates.len(), 2);
        let batch = monitor.batch(None);
        assert_eq!(batch.rows.len(), 2);
        assert!(batch.evicted_updates > 0);
    }

    #[test]
    fn quiet_active_requests_survive_churn_reconnect_and_protocol_budgets() {
        let monitor = TrafficMonitor::with_active_limits(2, 2, 16 * 1024);
        let mut http = TrafficRequest::default();
        http.diagnostics.protocol = Some("http".into());
        http.started_at = Utc::now() - chrono::Duration::days(2);
        let mut socket = TrafficRequest::default();
        socket.diagnostics.protocol = Some("websocket".into());
        socket.started_at = http.started_at;
        monitor.publish(http.clone());
        monitor.publish(socket.clone());
        let cursor = monitor.batch(None).cursor;
        for _ in 0..20 {
            let mut row = TrafficRequest::default();
            row.diagnostics.protocol = Some("http".into());
            monitor.publish(row.clone());
            row.completed = true;
            monitor.publish(row);
        }
        assert!(monitor.batch(Some(&cursor)).gap);
        let snapshot = monitor.active_snapshot(&TrafficQuery::default());
        assert_eq!(snapshot.rows.len(), 2);
        assert!(snapshot.rows.iter().any(|r| r.id == socket.id));
        assert!(snapshot.rows.iter().any(|r| r.id == http.id));
        assert_eq!(snapshot.summary.counts.http, 1);
        assert_eq!(snapshot.summary.counts.websocket, 1);
        assert!(!snapshot.truncated);

        // HTTP saturation cannot evict the quiet WS or corrupt exact counts.
        let mut overflow = Vec::new();
        for _ in 0..8 {
            let mut row = TrafficRequest::default();
            row.diagnostics.protocol = Some("http".into());
            monitor.publish(row.clone());
            overflow.push(row);
        }
        let summary = monitor.summary_snapshot();
        assert_eq!(summary.counts.http, 9);
        assert_eq!(summary.http.retained, 2);
        assert!(summary.http.saturated);
        assert!(!summary.websocket.saturated);
        assert_eq!(summary.counts.websocket, 1);
        for mut row in overflow {
            row.completed = true;
            monitor.publish(row);
        }
        assert_eq!(monitor.summary_snapshot().counts.http, 1);
        http.completed = true;
        socket.completed = true;
        monitor.publish(http);
        monitor.publish(socket);
        let summary = monitor.summary_snapshot();
        assert_eq!(summary.counts.http + summary.counts.websocket, 0);
    }

    #[test]
    fn active_filters_precede_limits_and_summary_bytes_are_bounded() {
        let monitor = TrafficMonitor::with_active_limits(2, 4, 8 * 1024);
        let mut target = TrafficRequest {
            service: Some("target".into()),
            ..Default::default()
        };
        target.diagnostics.protocol = Some("http".into());
        target.diagnostics.entra = Some(EntraDiagnostics {
            groups: vec!["private-claim".into()],
            ..Default::default()
        });
        target.timeline.push(TrafficStep {
            stage: "received".into(),
            elapsed_ms: 0,
            code: None,
            upstream_status: None,
            attempt: 0,
        });
        monitor.publish(target.clone());
        for _ in 0..8 {
            monitor.publish(TrafficRequest::default());
        }
        let query = TrafficQuery {
            service: Some("target".into()),
            limit: Some(1),
            ..Default::default()
        };
        let snapshot = monitor.active_snapshot(&query);
        assert_eq!(snapshot.rows.len(), 1);
        assert_eq!(snapshot.rows[0].id, target.id);
        assert!(snapshot.rows[0].summary_only);
        assert!(snapshot.rows[0].timeline.is_empty());
        assert!(snapshot.rows[0].diagnostics.entra.is_none());
        assert!(snapshot.summary.http.retained_bytes <= 8 * 1024);
        assert_eq!(
            snapshot.summary.http.matching, None,
            "overflow makes filtered counts unknown"
        );
        let snapshot = monitor.publication_snapshot();
        assert_eq!(snapshot.summary.http.matching, Some(9));
        assert!(snapshot.truncated);

        // No row fits, but exact active counts and explicit partial coverage remain.
        let tiny = TrafficMonitor::with_active_limits(2, 4, 1);
        tiny.publish(TrafficRequest::default());
        let snapshot = tiny.active_snapshot(&TrafficQuery::default());
        assert!(snapshot.rows.is_empty());
        assert_eq!(snapshot.summary.counts.http, 1);
        assert!(snapshot.summary.http.saturated);
    }

    #[test]
    fn completion_and_protocol_filters_are_additive_and_validated() {
        assert!(TrafficQuery::default().validate().is_ok());
        for basis in ["started", "completed"] {
            assert!(TrafficQuery {
                time_basis: Some(basis.into()),
                ..Default::default()
            }
            .validate()
            .is_ok());
        }
        for protocol in ["http", "websocket", "unknown"] {
            assert!(TrafficQuery {
                protocol: Some(protocol.into()),
                ..Default::default()
            }
            .validate()
            .is_ok());
        }
        assert!(TrafficQuery {
            protocol: Some("payload".into()),
            ..Default::default()
        }
        .validate()
        .is_err());
        assert!(TrafficQuery {
            time_basis: Some("invalid".into()),
            ..Default::default()
        }
        .validate()
        .is_err());
        let mut row = TrafficRequest::default();
        let monitor = TrafficMonitor::new(2);
        monitor.publish(row.clone());
        row.diagnostics.protocol = Some("websocket".into());
        monitor.publish(row.clone());
        let summary = monitor.summary_snapshot();
        assert_eq!(summary.counts.http, 0);
        assert_eq!(summary.counts.websocket, 1);
        let late = row.clone();
        row.completed = true;
        monitor.publish(row.clone());
        monitor.publish(late.clone());
        monitor.publish_socket_progress(late);
        monitor.publish(row);
        let snapshot = monitor.active_snapshot(&TrafficQuery::default());
        assert!(snapshot.rows.is_empty());
        assert_eq!(snapshot.summary.counts.websocket, 0);
        assert!(monitor.batch(None).rows[0].completed);
    }

    #[test]
    fn context_drop_retires_active_state_even_while_journal_clones_survive() {
        let monitor = TrafficMonitor::new(2);
        let row = TrafficRequest::default();
        let lease = monitor.active_lease(&row);
        monitor.publish(row.clone());
        assert_eq!(monitor.summary_snapshot().counts.http, 1);
        drop(lease);
        assert_eq!(monitor.summary_snapshot().counts.http, 0);
        assert!(monitor.publication_snapshot().rows.is_empty());
        monitor.publish_socket_progress(&row);
        monitor.publish(row);
        assert!(monitor.publication_snapshot().rows.is_empty());
    }

    #[test]
    fn quiet_socket_keeps_final_burst_counters_without_cloning_investigation_metadata() {
        let monitor = TrafficMonitor::new(2);
        let mut row = TrafficRequest::default();
        row.diagnostics.protocol = Some("websocket".into());
        row.diagnostics.websocket = Some(WebSocketDiagnostics {
            state: "open".into(),
            ..Default::default()
        });
        monitor.publish(row.clone());
        monitor.publish_socket_progress(&row);
        // This investigation field must only be copied on the next full sample.
        row.endpoint = Some("/changed-after-sample".into());
        let socket = row.diagnostics.websocket.as_mut().unwrap();
        socket.client_bytes = u64::MAX;
        socket.upstream_bytes = u64::MAX;
        socket.client_frames = u64::MAX;
        socket.upstream_frames = u64::MAX;
        socket.duration_ms = u64::MAX;
        socket.observed_at = Some(Utc::now());
        socket.last_client_activity_at = socket.observed_at;
        socket.last_upstream_activity_at = socket.observed_at;
        monitor.publish_socket_progress(&row);
        let snapshot = monitor.publication_snapshot();
        assert_eq!(
            snapshot.rows[0]
                .diagnostics
                .websocket
                .as_ref()
                .unwrap()
                .upstream_bytes,
            u64::MAX
        );
        assert!(snapshot.rows[0].endpoint.is_none());
        let json_bytes = serde_json::to_vec(&snapshot.rows[0]).unwrap().len();
        assert!(json_bytes <= snapshot.summary.websocket.retained_bytes);
        assert_eq!(
            monitor.batch(None).rows[0]
                .diagnostics
                .websocket
                .as_ref()
                .unwrap()
                .client_frames,
            u64::MAX
        );
    }

    #[test]
    fn legacy_traffic_json_remains_readable_without_fabricated_timings() {
        let mut json = serde_json::to_value(TrafficRequest::default()).unwrap();
        for field in [
            "upstream_timings",
            "usage",
            "debug_bundle",
            "key_prefix",
            "completed_at",
            "summary_only",
        ] {
            json.as_object_mut().unwrap().remove(field);
        }
        json["diagnostics"]
            .as_object_mut()
            .unwrap()
            .remove("traffic_id");
        json["diagnostics"]
            .as_object_mut()
            .unwrap()
            .remove("routing_mode");
        let old: TrafficRequest = serde_json::from_value(json).unwrap();
        assert!(old.upstream_timings.is_empty());
        assert!(old.usage.is_none());
        assert!(old.debug_bundle.is_none());
        assert!(old.completed_at.is_none());
        assert!(!old.summary_only);
        assert!(old.diagnostics.traffic_id.is_none());
        assert!(old.diagnostics.routing_mode.is_none());
    }

    #[test]
    fn reconnect_reports_eviction_instance_change_and_duplicate_client_ids() {
        let monitor = TrafficMonitor::new(2);
        let request = TrafficRequest {
            request_id: "same".into(),
            ..Default::default()
        };
        monitor.publish(request.clone());
        let cursor = monitor.batch(None).cursor;
        for _ in 0..3 {
            monitor.publish(TrafficRequest {
                request_id: "same".into(),
                ..Default::default()
            });
        }
        let batch = monitor.batch(Some(&cursor));
        assert!(batch.gap);
        assert_eq!(batch.rows.len(), 2);
        assert_ne!(batch.rows[0].id, batch.rows[1].id);
        assert!(monitor.batch(Some("other:1")).gap);
        assert!(monitor.batch(Some(&batch.cursor)).rows.is_empty());
    }
    #[test]
    fn timeline_is_bounded_and_preserves_first_failure() {
        let mut request = TrafficRequest::default();
        request.step("received", 0, None);
        request.fail("gateway", "control_state_unavailable");
        for n in 1..100 {
            request.step("budget", n, None);
        }
        request.fail("gateway", "other");
        assert_eq!(request.timeline.len(), TIMELINE_CAPACITY);
        assert_eq!(request.timeline[0].stage, "received");
        assert!(request.timeline_truncated);
        assert_eq!(
            request.diagnostics.failure_code.as_deref(),
            Some("control_state_unavailable")
        );
    }
    #[test]
    fn correlation_tokens_are_bounded() {
        assert_eq!(correlation_id(Some("request-123")), "request-123");
        assert_ne!(correlation_id(Some("Bearer secret")), "Bearer secret");
        assert!(correlation_id(Some(&"a".repeat(129))).len() < 129);
    }
}
