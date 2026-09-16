//! Bounded metadata-only diagnostics, independent of billing identity and storage.
use crate::{GatewayError, GatewayResult};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::{Mutex, OnceLock},
};
use uuid::Uuid;

const JOURNAL_CAPACITY: usize = 512;
const TIMELINE_CAPACITY: usize = 32;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RequestDiagnostics {
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
        }
        // Only fixed destination names and internal IDs; never raw database errors.
        tracing::error!(request_id = %self.request_id, diagnostic_id = %self.id,
            instance_id = %self.instance_id, destination, "gateway diagnostic recording failed");
        monitor().publish(self.clone());
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct TrafficQuery {
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
        {
            return Err(GatewayError::InvalidUsageQuery);
        }
        Ok(())
    }
}

#[async_trait]
pub trait TrafficStore: Send + Sync {
    async fn insert_traffic(&self, request: &TrafficRequest) -> GatewayResult<()>;
    async fn traffic_history(&self, query: TrafficQuery) -> GatewayResult<Vec<TrafficRequest>>;
}

pub struct TrafficMonitor {
    pub instance_id: String,
    journal: Mutex<(u64, VecDeque<(u64, TrafficRequest)>)>,
    socket_updates: Mutex<HashMap<Uuid, TrafficRequest>>,
    capacity: usize,
}

#[derive(Serialize)]
pub struct TrafficBatch {
    pub instance_id: String,
    pub cursor: String,
    pub gap: bool,
    pub evicted_updates: u64,
    pub rows: Vec<TrafficRequest>,
}

impl TrafficMonitor {
    pub fn new(capacity: usize) -> Self {
        Self {
            instance_id: Uuid::new_v4().to_string(),
            journal: Mutex::new((0, VecDeque::new())),
            socket_updates: Mutex::new(HashMap::new()),
            capacity: capacity.max(1),
        }
    }

    /// Keep only the latest socket sample until the next live poll.
    pub fn publish_socket_progress(&self, request: TrafficRequest) {
        let mut updates = self
            .socket_updates
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if updates.len() < self.capacity || updates.contains_key(&request.id) {
            updates.insert(request.id, request);
        }
    }

    pub fn publish(&self, request: TrafficRequest) {
        let mut updates = self
            .socket_updates
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        updates.remove(&request.id);
        let mut journal = self.journal.lock().unwrap_or_else(|e| e.into_inner());
        journal.0 += 1;
        let sequence = journal.0;
        journal.1.push_back((sequence, request));
        while journal.1.len() > self.capacity {
            journal.1.pop_front();
        }
    }

    pub fn batch(&self, cursor: Option<&str>) -> TrafficBatch {
        let mut updates = self
            .socket_updates
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let mut journal = self.journal.lock().unwrap_or_else(|e| e.into_inner());
        for (_, request) in updates.drain() {
            journal.0 += 1;
            let sequence = journal.0;
            journal.1.push_back((sequence, request));
        }
        while journal.1.len() > self.capacity {
            journal.1.pop_front();
        }
        let parsed = cursor.and_then(|v| v.rsplit_once(':'));
        let sequence = parsed.and_then(|(instance, seq)| {
            (instance == self.instance_id)
                .then(|| seq.parse::<u64>().ok())
                .flatten()
        });
        let oldest = journal.1.front().map_or(journal.0 + 1, |(seq, _)| *seq);
        let gap = cursor.is_some()
            && sequence.is_none_or(|seq| seq.saturating_add(1) < oldest || seq > journal.0);
        let after = if gap { 0 } else { sequence.unwrap_or(0) };
        let mut seen = HashSet::new();
        let rows = journal
            .1
            .iter()
            .rev()
            .filter(|(seq, request)| *seq > after && seen.insert(request.id))
            .map(|(_, request)| request.clone())
            .collect();
        TrafficBatch {
            instance_id: self.instance_id.clone(),
            cursor: format!("{}:{}", self.instance_id, journal.0),
            gap,
            evicted_updates: oldest.saturating_sub(1),
            rows,
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
            monitor.publish_socket_progress(row.clone());
        }
        let batch = monitor.batch(None);
        assert_eq!(batch.rows.len(), 1);
        assert_eq!(batch.rows[0].elapsed_ms, 99);
        assert_eq!(batch.evicted_updates, 0);
        row.elapsed_ms = 100;
        monitor.publish_socket_progress(row.clone());
        row.completed = true;
        monitor.publish(row);
        let batch = monitor.batch(Some(&batch.cursor));
        assert_eq!(batch.rows.len(), 1);
        assert!(batch.rows[0].completed);
        for _ in 0..10 {
            monitor.publish_socket_progress(TrafficRequest::default());
        }
        assert_eq!(monitor.socket_updates.lock().unwrap().len(), 2);
        let batch = monitor.batch(None);
        assert_eq!(batch.rows.len(), 2);
        assert!(batch.evicted_updates > 0);
    }

    #[test]
    fn legacy_traffic_json_remains_readable_without_fabricated_timings() {
        let mut json = serde_json::to_value(TrafficRequest::default()).unwrap();
        for field in ["upstream_timings", "usage", "debug_bundle", "key_prefix"] {
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
