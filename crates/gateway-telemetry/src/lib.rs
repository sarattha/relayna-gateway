use tracing_subscriber::{fmt, EnvFilter};

pub fn init(log_level: &str) {
    let filter =
        EnvFilter::try_new(log_level).unwrap_or_else(|_| EnvFilter::new("gateway_api=info"));
    let _ = fmt().with_env_filter(filter).json().try_init();
}

use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicI64, AtomicU64, Ordering},
        Mutex, OnceLock,
    },
};

static REQUESTS_TOTAL: AtomicU64 = AtomicU64::new(0);
static ERRORS_TOTAL: AtomicU64 = AtomicU64::new(0);
static RATE_LIMIT_REJECTIONS_TOTAL: AtomicU64 = AtomicU64::new(0);
static BUDGET_REJECTIONS_TOTAL: AtomicU64 = AtomicU64::new(0);
static TOKENS_TOTAL: AtomicU64 = AtomicU64::new(0);
static ESTIMATED_COST_MICRO_USD_TOTAL: AtomicU64 = AtomicU64::new(0);
static ACTIVE_STREAMS: AtomicI64 = AtomicI64::new(0);
static STREAM_ABORTS_TOTAL: AtomicU64 = AtomicU64::new(0);
static FIRST_TOKEN_LATENCY_MS_TOTAL: AtomicU64 = AtomicU64::new(0);
static FIRST_TOKEN_LATENCY_SAMPLES: AtomicU64 = AtomicU64::new(0);
static PROVIDER_SELECTIONS_TOTAL: AtomicU64 = AtomicU64::new(0);
static PROVIDER_FALLBACKS_TOTAL: AtomicU64 = AtomicU64::new(0);
static CIRCUIT_TRANSITIONS_TOTAL: AtomicU64 = AtomicU64::new(0);
static GUARDRAIL_METRICS: OnceLock<Mutex<BTreeMap<GuardrailMetricKey, GuardrailMetricValue>>> =
    OnceLock::new();

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct GuardrailMetricKey {
    guardrail: String,
    mode: String,
    action: String,
    failure_policy: String,
}

#[derive(Debug, Clone, Default)]
struct GuardrailMetricValue {
    executions: u64,
    failures: u64,
    latency_ms_total: u64,
}

pub fn record_request(status_code: u16) {
    REQUESTS_TOTAL.fetch_add(1, Ordering::Relaxed);
    if status_code >= 400 {
        ERRORS_TOTAL.fetch_add(1, Ordering::Relaxed);
    }
}

pub fn record_rate_limit_rejection() {
    RATE_LIMIT_REJECTIONS_TOTAL.fetch_add(1, Ordering::Relaxed);
}

pub fn record_budget_rejection() {
    BUDGET_REJECTIONS_TOTAL.fetch_add(1, Ordering::Relaxed);
}

pub fn record_tokens(tokens: i64) {
    if let Ok(tokens) = u64::try_from(tokens.max(0)) {
        TOKENS_TOTAL.fetch_add(tokens, Ordering::Relaxed);
    }
}

pub fn record_estimated_cost_usd(cost: f64) {
    if cost.is_finite() && cost > 0.0 {
        ESTIMATED_COST_MICRO_USD_TOTAL.fetch_add((cost * 1_000_000.0) as u64, Ordering::Relaxed);
    }
}

pub fn stream_started() {
    ACTIVE_STREAMS.fetch_add(1, Ordering::Relaxed);
}

pub fn stream_finished(aborted: bool) {
    ACTIVE_STREAMS.fetch_sub(1, Ordering::Relaxed);
    if aborted {
        STREAM_ABORTS_TOTAL.fetch_add(1, Ordering::Relaxed);
    }
}

pub fn record_first_token_latency_ms(latency_ms: u64) {
    FIRST_TOKEN_LATENCY_MS_TOTAL.fetch_add(latency_ms, Ordering::Relaxed);
    FIRST_TOKEN_LATENCY_SAMPLES.fetch_add(1, Ordering::Relaxed);
}

pub fn record_provider_selection() {
    PROVIDER_SELECTIONS_TOTAL.fetch_add(1, Ordering::Relaxed);
}

pub fn record_provider_fallback() {
    PROVIDER_FALLBACKS_TOTAL.fetch_add(1, Ordering::Relaxed);
}

pub fn record_circuit_transition() {
    CIRCUIT_TRANSITIONS_TOTAL.fetch_add(1, Ordering::Relaxed);
}

pub fn record_guardrail_execution(
    guardrail: &str,
    mode: &str,
    action: &str,
    failure_policy: &str,
    latency_ms: u64,
    failed: bool,
) {
    let metrics = GUARDRAIL_METRICS.get_or_init(|| Mutex::new(BTreeMap::new()));
    let mut metrics = metrics.lock().expect("guardrail metric lock poisoned");
    let value = metrics
        .entry(GuardrailMetricKey {
            guardrail: sanitize_label(guardrail),
            mode: sanitize_label(mode),
            action: sanitize_label(action),
            failure_policy: sanitize_label(failure_policy),
        })
        .or_default();
    value.executions = value.executions.saturating_add(1);
    value.latency_ms_total = value.latency_ms_total.saturating_add(latency_ms);
    if failed {
        value.failures = value.failures.saturating_add(1);
    }
}

pub fn prometheus() -> String {
    let cost = ESTIMATED_COST_MICRO_USD_TOTAL.load(Ordering::Relaxed) as f64 / 1_000_000.0;
    let mut metrics = format!(
        "\
# TYPE gateway_requests_total counter
gateway_requests_total {}
# TYPE gateway_errors_total counter
gateway_errors_total {}
# TYPE gateway_rate_limit_rejections_total counter
gateway_rate_limit_rejections_total {}
# TYPE gateway_budget_rejections_total counter
gateway_budget_rejections_total {}
# TYPE gateway_tokens_total counter
gateway_tokens_total {}
# TYPE gateway_estimated_cost_total counter
gateway_estimated_cost_total {:.6}
# TYPE gateway_active_streams gauge
gateway_active_streams {}
# TYPE gateway_stream_aborts_total counter
gateway_stream_aborts_total {}
# TYPE gateway_first_token_latency_ms counter
gateway_first_token_latency_ms {}
# TYPE gateway_first_token_latency_samples counter
gateway_first_token_latency_samples {}
# TYPE gateway_provider_selections_total counter
gateway_provider_selections_total {}
# TYPE gateway_provider_fallbacks_total counter
gateway_provider_fallbacks_total {}
# TYPE gateway_circuit_transitions_total counter
gateway_circuit_transitions_total {}
",
        REQUESTS_TOTAL.load(Ordering::Relaxed),
        ERRORS_TOTAL.load(Ordering::Relaxed),
        RATE_LIMIT_REJECTIONS_TOTAL.load(Ordering::Relaxed),
        BUDGET_REJECTIONS_TOTAL.load(Ordering::Relaxed),
        TOKENS_TOTAL.load(Ordering::Relaxed),
        cost,
        ACTIVE_STREAMS.load(Ordering::Relaxed),
        STREAM_ABORTS_TOTAL.load(Ordering::Relaxed),
        FIRST_TOKEN_LATENCY_MS_TOTAL.load(Ordering::Relaxed),
        FIRST_TOKEN_LATENCY_SAMPLES.load(Ordering::Relaxed),
        PROVIDER_SELECTIONS_TOTAL.load(Ordering::Relaxed),
        PROVIDER_FALLBACKS_TOTAL.load(Ordering::Relaxed),
        CIRCUIT_TRANSITIONS_TOTAL.load(Ordering::Relaxed),
    );
    metrics.push_str("# TYPE gateway_guardrail_executions_total counter\n");
    metrics.push_str("# TYPE gateway_guardrail_failures_total counter\n");
    metrics.push_str("# TYPE gateway_guardrail_latency_ms_total counter\n");
    if let Some(guardrail_metrics) = GUARDRAIL_METRICS.get() {
        for (key, value) in guardrail_metrics
            .lock()
            .expect("guardrail metric lock poisoned")
            .iter()
        {
            let labels = format!(
                "guardrail=\"{}\",mode=\"{}\",action=\"{}\",failure_policy=\"{}\"",
                key.guardrail, key.mode, key.action, key.failure_policy
            );
            metrics.push_str(&format!(
                "gateway_guardrail_executions_total{{{labels}}} {}\n",
                value.executions
            ));
            metrics.push_str(&format!(
                "gateway_guardrail_failures_total{{{labels}}} {}\n",
                value.failures
            ));
            metrics.push_str(&format!(
                "gateway_guardrail_latency_ms_total{{{labels}}} {}\n",
                value.latency_ms_total
            ));
        }
    }
    metrics
}

fn sanitize_label(value: &str) -> String {
    value
        .chars()
        .filter(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
        })
        .take(128)
        .collect()
}

pub fn is_sensitive_field(name: &str) -> bool {
    let normalized = name.to_ascii_lowercase();
    normalized == "authorization"
        || normalized == "proxy-authorization"
        || normalized == "x-api-key"
        || normalized.ends_with("_key")
        || normalized.ends_with("_token")
        || normalized.contains("secret")
        || normalized.contains("password")
}

#[cfg(test)]
mod tests {
    #[test]
    fn renders_expected_metric_names() {
        let metrics = super::prometheus();
        assert!(metrics.contains("gateway_requests_total"));
        assert!(metrics.contains("gateway_active_streams"));
        assert!(metrics.contains("gateway_first_token_latency_ms"));
        assert!(metrics.contains("gateway_provider_selections_total"));
        assert!(metrics.contains("gateway_provider_fallbacks_total"));
    }

    #[test]
    fn identifies_sensitive_fields_for_redaction() {
        assert!(super::is_sensitive_field("Authorization"));
        assert!(super::is_sensitive_field("provider_secret"));
        assert!(super::is_sensitive_field("internal_service_token"));
        assert!(!super::is_sensitive_field("request_id"));
        assert!(!super::is_sensitive_field("project_id"));
    }
}
