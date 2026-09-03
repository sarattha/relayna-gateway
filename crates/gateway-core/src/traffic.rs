//! Bounded metadata-only diagnostics, independent of billing identity and storage.
use crate::{GatewayError, GatewayResult};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashSet, VecDeque},
    sync::{Mutex, OnceLock},
};
use uuid::Uuid;

const JOURNAL_CAPACITY: usize = 512;
const TIMELINE_CAPACITY: usize = 32;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RequestDiagnostics {
    pub failure_stage: Option<String>,
    pub failure_code: Option<String>,
    pub failure_source: Option<String>,
    pub outcome: Option<String>,
    pub upstream_status: Option<u16>,
    pub instance_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrafficStep {
    pub stage: String,
    pub elapsed_ms: i64,
    pub code: Option<String>,
    pub upstream_status: Option<u16>,
    pub attempt: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrafficRequest {
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
            capacity: capacity.max(1),
        }
    }

    pub fn publish(&self, request: TrafficRequest) {
        let mut journal = self.journal.lock().unwrap_or_else(|e| e.into_inner());
        journal.0 += 1;
        let sequence = journal.0;
        journal.1.push_back((sequence, request));
        while journal.1.len() > self.capacity {
            journal.1.pop_front();
        }
    }

    pub fn batch(&self, cursor: Option<&str>) -> TrafficBatch {
        let journal = self.journal.lock().unwrap_or_else(|e| e.into_inner());
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
