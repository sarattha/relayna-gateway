use crate::{GatewayError, GatewayResult, Provider, Route};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoutingStrategy {
    Priority,
    Weighted,
    LeastLatency,
    LeastCost,
    HealthAware,
    BudgetAware,
    RegionAffinity,
    CapabilityAware,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CircuitBreakerState {
    Closed,
    Open,
    HalfOpen,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderHealthStatus {
    Healthy,
    Degraded,
    Unhealthy,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProviderCandidate {
    pub name: String,
    pub provider: Provider,
    pub priority: i32,
    pub weight: u32,
    pub enabled: bool,
    pub health_status: ProviderHealthStatus,
    pub circuit_state: CircuitBreakerState,
    pub average_latency_ms: Option<i64>,
    pub estimated_cost_usd: Option<f64>,
    pub remaining_budget_usd: Option<f64>,
    pub regions: Vec<String>,
    pub capabilities: Vec<String>,
    pub cooldown_until: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RoutingDecisionRequest {
    pub strategy: RoutingStrategy,
    pub route: Route,
    pub preferred_region: Option<String>,
    pub required_capabilities: Vec<String>,
    pub estimated_cost_usd: Option<f64>,
    pub request_hash: Option<u64>,
    pub now: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProviderSelection {
    pub selected: ProviderCandidate,
    pub strategy: RoutingStrategy,
    pub considered: Vec<String>,
    pub rejected: Vec<ProviderRejection>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderRejection {
    pub provider: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FallbackPolicy {
    pub retry_status_codes: Vec<u16>,
    pub max_attempts: u32,
    pub cooldown_seconds: u64,
    pub retry_timeouts: bool,
    pub provider_failover: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProviderHealthState {
    pub name: String,
    pub provider: Provider,
    pub status: ProviderHealthStatus,
    pub circuit_state: CircuitBreakerState,
    pub active_check_ok: Option<bool>,
    pub passive_success_count: i64,
    pub passive_failure_count: i64,
    pub consecutive_failures: i32,
    pub average_latency_ms: Option<i64>,
    pub last_error_code: Option<String>,
    pub cooldown_until: Option<DateTime<Utc>>,
    pub checked_at: Option<DateTime<Utc>>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DebugBundle {
    pub request_id: String,
    pub route: Option<Route>,
    pub provider: Option<Provider>,
    pub service_name: Option<String>,
    pub policy_trace: Vec<String>,
    pub guardrail_trace: Vec<String>,
    pub selection_trace: Vec<String>,
    pub fallback_history: Vec<FallbackAttempt>,
    pub upstream_latency_ms: Option<i64>,
    pub request_hash: Option<String>,
    pub response_hash: Option<String>,
    pub redaction_version: i32,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FallbackAttempt {
    pub from_provider: String,
    pub to_provider: String,
    pub reason: String,
    pub status_code: Option<u16>,
    pub latency_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServiceImportDiff {
    pub added: Vec<String>,
    pub changed: Vec<String>,
    pub removed: Vec<String>,
    pub invalid: Vec<ServiceImportValidationIssue>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServiceImportValidationIssue {
    pub service_name: String,
    pub field: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ServiceRegistrySnapshot {
    pub version: i64,
    pub source: String,
    pub diff: ServiceImportDiff,
    pub services_json: serde_json::Value,
    pub activated_at: Option<DateTime<Utc>>,
    pub rolled_back_from_version: Option<i64>,
    pub created_at: DateTime<Utc>,
}

#[async_trait]
pub trait ProviderIntelligenceStore: Send + Sync {
    async fn list_provider_health_states(&self) -> GatewayResult<Vec<ProviderHealthState>>;
    async fn upsert_provider_health_state(
        &self,
        state: ProviderHealthState,
    ) -> GatewayResult<ProviderHealthState>;
    async fn get_debug_bundle(&self, request_id: &str) -> GatewayResult<Option<DebugBundle>>;
    async fn insert_debug_bundle(&self, bundle: DebugBundle) -> GatewayResult<()>;
    async fn list_service_registry_snapshots(&self) -> GatewayResult<Vec<ServiceRegistrySnapshot>>;
    async fn insert_service_registry_snapshot(
        &self,
        snapshot: ServiceRegistrySnapshot,
    ) -> GatewayResult<ServiceRegistrySnapshot>;
    async fn service_registry_snapshot(
        &self,
        version: i64,
    ) -> GatewayResult<Option<ServiceRegistrySnapshot>>;
}

#[async_trait]
impl<T> ProviderIntelligenceStore for std::sync::Arc<T>
where
    T: ProviderIntelligenceStore + ?Sized,
{
    async fn list_provider_health_states(&self) -> GatewayResult<Vec<ProviderHealthState>> {
        (**self).list_provider_health_states().await
    }

    async fn upsert_provider_health_state(
        &self,
        state: ProviderHealthState,
    ) -> GatewayResult<ProviderHealthState> {
        (**self).upsert_provider_health_state(state).await
    }

    async fn get_debug_bundle(&self, request_id: &str) -> GatewayResult<Option<DebugBundle>> {
        (**self).get_debug_bundle(request_id).await
    }

    async fn insert_debug_bundle(&self, bundle: DebugBundle) -> GatewayResult<()> {
        (**self).insert_debug_bundle(bundle).await
    }

    async fn list_service_registry_snapshots(&self) -> GatewayResult<Vec<ServiceRegistrySnapshot>> {
        (**self).list_service_registry_snapshots().await
    }

    async fn insert_service_registry_snapshot(
        &self,
        snapshot: ServiceRegistrySnapshot,
    ) -> GatewayResult<ServiceRegistrySnapshot> {
        (**self).insert_service_registry_snapshot(snapshot).await
    }

    async fn service_registry_snapshot(
        &self,
        version: i64,
    ) -> GatewayResult<Option<ServiceRegistrySnapshot>> {
        (**self).service_registry_snapshot(version).await
    }
}

impl Default for FallbackPolicy {
    fn default() -> Self {
        Self {
            retry_status_codes: vec![429, 500, 502, 503, 504],
            max_attempts: 2,
            cooldown_seconds: 30,
            retry_timeouts: true,
            provider_failover: true,
        }
    }
}

impl FallbackPolicy {
    pub fn allows_status_retry(&self, status_code: u16, attempt: u32) -> bool {
        attempt < self.max_attempts && self.retry_status_codes.contains(&status_code)
    }

    pub fn allows_timeout_retry(&self, attempt: u32) -> bool {
        attempt < self.max_attempts && self.retry_timeouts
    }
}

pub fn select_provider(
    request: &RoutingDecisionRequest,
    candidates: &[ProviderCandidate],
) -> GatewayResult<ProviderSelection> {
    let mut rejected = Vec::new();
    let mut eligible = Vec::new();
    for candidate in candidates {
        if let Some(reason) = rejection_reason(request, candidate) {
            rejected.push(ProviderRejection {
                provider: candidate.name.clone(),
                reason,
            });
        } else {
            eligible.push(candidate.clone());
        }
    }

    if eligible.is_empty() {
        return Err(GatewayError::PolicyDenied);
    }

    let selected = match request.strategy {
        RoutingStrategy::Priority => eligible
            .into_iter()
            .min_by_key(|candidate| candidate.priority)
            .expect("eligible provider"),
        RoutingStrategy::Weighted => weighted_pick(&eligible, request.request_hash.unwrap_or(0)),
        RoutingStrategy::LeastLatency | RoutingStrategy::HealthAware => eligible
            .into_iter()
            .min_by(compare_latency_then_priority)
            .expect("eligible provider"),
        RoutingStrategy::LeastCost | RoutingStrategy::BudgetAware => eligible
            .into_iter()
            .min_by(compare_cost_then_priority)
            .expect("eligible provider"),
        RoutingStrategy::RegionAffinity | RoutingStrategy::CapabilityAware => eligible
            .into_iter()
            .min_by_key(|candidate| candidate.priority)
            .expect("eligible provider"),
    };

    Ok(ProviderSelection {
        selected,
        strategy: request.strategy,
        considered: candidates
            .iter()
            .map(|candidate| candidate.name.clone())
            .collect(),
        rejected,
    })
}

pub fn circuit_state_after_passive_result(
    previous: CircuitBreakerState,
    consecutive_failures: i32,
    success: bool,
    failure_threshold: i32,
) -> CircuitBreakerState {
    if success {
        return CircuitBreakerState::Closed;
    }
    match previous {
        CircuitBreakerState::Open => CircuitBreakerState::Open,
        CircuitBreakerState::HalfOpen => CircuitBreakerState::Open,
        CircuitBreakerState::Closed if consecutive_failures + 1 >= failure_threshold => {
            CircuitBreakerState::Open
        }
        CircuitBreakerState::Closed => CircuitBreakerState::Closed,
    }
}

fn rejection_reason(
    request: &RoutingDecisionRequest,
    candidate: &ProviderCandidate,
) -> Option<String> {
    if !candidate.enabled {
        return Some("disabled".to_owned());
    }
    if candidate.circuit_state == CircuitBreakerState::Open
        && candidate
            .cooldown_until
            .is_none_or(|cooldown| cooldown > request.now)
    {
        return Some("circuit_open".to_owned());
    }
    if candidate.health_status == ProviderHealthStatus::Unhealthy {
        return Some("unhealthy".to_owned());
    }
    if let (Some(required), Some(remaining)) =
        (request.estimated_cost_usd, candidate.remaining_budget_usd)
    {
        if remaining < required {
            return Some("budget_exhausted".to_owned());
        }
    }
    if let Some(region) = request.preferred_region.as_deref() {
        if request.strategy == RoutingStrategy::RegionAffinity
            && !candidate.regions.iter().any(|value| value == region)
        {
            return Some("region_mismatch".to_owned());
        }
    }
    for required in &request.required_capabilities {
        if !candidate
            .capabilities
            .iter()
            .any(|capability| capability == required)
        {
            return Some(format!("missing_capability:{required}"));
        }
    }
    None
}

fn weighted_pick(candidates: &[ProviderCandidate], request_hash: u64) -> ProviderCandidate {
    let total_weight: u64 = candidates
        .iter()
        .map(|candidate| u64::from(candidate.weight.max(1)))
        .sum();
    let mut slot = request_hash % total_weight;
    for candidate in candidates {
        let weight = u64::from(candidate.weight.max(1));
        if slot < weight {
            return candidate.clone();
        }
        slot -= weight;
    }
    candidates[0].clone()
}

fn compare_latency_then_priority(left: &ProviderCandidate, right: &ProviderCandidate) -> Ordering {
    left.average_latency_ms
        .unwrap_or(i64::MAX)
        .cmp(&right.average_latency_ms.unwrap_or(i64::MAX))
        .then_with(|| left.priority.cmp(&right.priority))
}

fn compare_cost_then_priority(left: &ProviderCandidate, right: &ProviderCandidate) -> Ordering {
    left.estimated_cost_usd
        .unwrap_or(f64::INFINITY)
        .partial_cmp(&right.estimated_cost_usd.unwrap_or(f64::INFINITY))
        .unwrap_or(Ordering::Equal)
        .then_with(|| left.priority.cmp(&right.priority))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn candidate(name: &str, priority: i32) -> ProviderCandidate {
        ProviderCandidate {
            name: name.to_owned(),
            provider: Provider::LiteLlm,
            priority,
            weight: 1,
            enabled: true,
            health_status: ProviderHealthStatus::Healthy,
            circuit_state: CircuitBreakerState::Closed,
            average_latency_ms: None,
            estimated_cost_usd: None,
            remaining_budget_usd: None,
            regions: vec!["us-east".to_owned()],
            capabilities: vec!["chat".to_owned()],
            cooldown_until: None,
        }
    }

    fn request(strategy: RoutingStrategy) -> RoutingDecisionRequest {
        RoutingDecisionRequest {
            strategy,
            route: Route::ChatCompletions,
            preferred_region: None,
            required_capabilities: Vec::new(),
            estimated_cost_usd: None,
            request_hash: Some(0),
            now: Utc.with_ymd_and_hms(2026, 5, 23, 0, 0, 0).unwrap(),
        }
    }

    #[test]
    fn priority_strategy_picks_lowest_priority() {
        let decision = select_provider(
            &request(RoutingStrategy::Priority),
            &[candidate("secondary", 20), candidate("primary", 10)],
        )
        .expect("decision");

        assert_eq!(decision.selected.name, "primary");
    }

    #[test]
    fn weighted_strategy_uses_stable_hash_slot() {
        let mut left = candidate("left", 10);
        left.weight = 1;
        let mut right = candidate("right", 20);
        right.weight = 3;
        let mut req = request(RoutingStrategy::Weighted);
        req.request_hash = Some(2);

        let decision = select_provider(&req, &[left, right]).expect("decision");

        assert_eq!(decision.selected.name, "right");
    }

    #[test]
    fn least_latency_and_least_cost_use_provider_scores() {
        let mut fast = candidate("fast", 20);
        fast.average_latency_ms = Some(40);
        fast.estimated_cost_usd = Some(0.05);
        let mut cheap = candidate("cheap", 30);
        cheap.average_latency_ms = Some(90);
        cheap.estimated_cost_usd = Some(0.01);

        assert_eq!(
            select_provider(
                &request(RoutingStrategy::LeastLatency),
                &[cheap.clone(), fast.clone()]
            )
            .expect("latency")
            .selected
            .name,
            "fast"
        );
        assert_eq!(
            select_provider(&request(RoutingStrategy::LeastCost), &[cheap, fast])
                .expect("cost")
                .selected
                .name,
            "cheap"
        );
    }

    #[test]
    fn constraints_reject_unhealthy_open_circuit_budget_region_and_capability() {
        let mut healthy = candidate("healthy", 10);
        healthy.regions = vec!["eu-west".to_owned()];
        healthy.capabilities = vec!["chat".to_owned(), "tools".to_owned()];
        healthy.remaining_budget_usd = Some(1.0);
        let mut bad = candidate("bad", 1);
        bad.circuit_state = CircuitBreakerState::Open;
        let mut req = request(RoutingStrategy::RegionAffinity);
        req.preferred_region = Some("eu-west".to_owned());
        req.required_capabilities = vec!["tools".to_owned()];
        req.estimated_cost_usd = Some(0.50);

        let decision = select_provider(&req, &[bad, healthy]).expect("decision");

        assert_eq!(decision.selected.name, "healthy");
        assert_eq!(decision.rejected[0].reason, "circuit_open");
    }

    #[test]
    fn fallback_policy_limits_attempts_and_statuses() {
        let policy = FallbackPolicy::default();

        assert!(policy.allows_status_retry(503, 1));
        assert!(!policy.allows_status_retry(400, 1));
        assert!(!policy.allows_status_retry(503, 2));
        assert!(policy.allows_timeout_retry(1));
    }

    #[test]
    fn circuit_opens_on_threshold_and_closes_after_success() {
        assert_eq!(
            circuit_state_after_passive_result(CircuitBreakerState::Closed, 2, false, 3),
            CircuitBreakerState::Open
        );
        assert_eq!(
            circuit_state_after_passive_result(CircuitBreakerState::HalfOpen, 0, true, 3),
            CircuitBreakerState::Closed
        );
    }
}
