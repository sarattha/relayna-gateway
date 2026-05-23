use tracing::{field, Span};
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
static AUTH_FAILURES_TOTAL: AtomicU64 = AtomicU64::new(0);
static POLICY_DENIALS_TOTAL: AtomicU64 = AtomicU64::new(0);
static RATE_LIMIT_REJECTIONS_TOTAL: AtomicU64 = AtomicU64::new(0);
static BUDGET_REJECTIONS_TOTAL: AtomicU64 = AtomicU64::new(0);
static TOKENS_TOTAL: AtomicU64 = AtomicU64::new(0);
static ESTIMATED_COST_MICRO_USD_TOTAL: AtomicU64 = AtomicU64::new(0);
static ACTIVE_REQUESTS: AtomicI64 = AtomicI64::new(0);
static ACTIVE_STREAMS: AtomicI64 = AtomicI64::new(0);
static STREAM_ABORTS_TOTAL: AtomicU64 = AtomicU64::new(0);
static FIRST_TOKEN_LATENCY_MS_TOTAL: AtomicU64 = AtomicU64::new(0);
static FIRST_TOKEN_LATENCY_SAMPLES: AtomicU64 = AtomicU64::new(0);
static PROVIDER_SELECTIONS_TOTAL: AtomicU64 = AtomicU64::new(0);
static PROVIDER_FALLBACKS_TOTAL: AtomicU64 = AtomicU64::new(0);
static CIRCUIT_TRANSITIONS_TOTAL: AtomicU64 = AtomicU64::new(0);
static GUARDRAIL_METRICS: OnceLock<Mutex<BTreeMap<GuardrailMetricKey, GuardrailMetricValue>>> =
    OnceLock::new();
static REQUEST_METRICS: OnceLock<Mutex<BTreeMap<RequestMetricKey, u64>>> = OnceLock::new();
static DENIAL_METRICS: OnceLock<Mutex<BTreeMap<DenialMetricKey, u64>>> = OnceLock::new();
static PROVIDER_FALLBACK_METRICS: OnceLock<Mutex<BTreeMap<ProviderFallbackMetricKey, u64>>> =
    OnceLock::new();
static CIRCUIT_STATE_METRICS: OnceLock<Mutex<BTreeMap<CircuitStateMetricKey, i64>>> =
    OnceLock::new();
static HISTOGRAMS: OnceLock<Mutex<BTreeMap<HistogramKey, HistogramValue>>> = OnceLock::new();

const DURATION_BUCKETS_MS: &[u64] = &[
    5, 10, 25, 50, 100, 250, 500, 1_000, 2_500, 5_000, 10_000, 30_000, 60_000, 120_000,
];
const FIRST_TOKEN_BUCKETS_MS: &[u64] =
    &[25, 50, 100, 250, 500, 1_000, 2_500, 5_000, 10_000, 30_000];

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct RequestMetricKey {
    route: String,
    provider: String,
    status_class: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct DenialMetricKey {
    kind: String,
    route: String,
    reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ProviderFallbackMetricKey {
    from_provider: String,
    to_provider: String,
    reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct CircuitStateMetricKey {
    provider: String,
    name: String,
    state: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct HistogramKey {
    metric: String,
    route: String,
    provider: String,
    stream: String,
}

#[derive(Debug, Clone, Default)]
struct HistogramValue {
    buckets: BTreeMap<u64, u64>,
    sum_ms: u64,
    count: u64,
}

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

pub fn record_request_with_dimensions(
    route: &str,
    provider: &str,
    status_code: u16,
    latency_ms: u64,
    stream: bool,
) {
    record_request(status_code);
    increment_request_metric(route, provider, status_code);
    observe_histogram(
        "gateway_request_duration_ms",
        route,
        provider,
        stream,
        latency_ms,
        DURATION_BUCKETS_MS,
    );
}

pub fn request_started() {
    ACTIVE_REQUESTS.fetch_add(1, Ordering::Relaxed);
}

pub fn request_finished() {
    ACTIVE_REQUESTS.fetch_sub(1, Ordering::Relaxed);
}

pub fn record_upstream_duration_ms(route: &str, provider: &str, stream: bool, latency_ms: u64) {
    observe_histogram(
        "gateway_upstream_duration_ms",
        route,
        provider,
        stream,
        latency_ms,
        DURATION_BUCKETS_MS,
    );
}

pub fn record_auth_failure(reason: &str) {
    AUTH_FAILURES_TOTAL.fetch_add(1, Ordering::Relaxed);
    increment_denial_metric("auth", "unknown", reason);
}

pub fn record_policy_denial(route: &str, reason: &str) {
    POLICY_DENIALS_TOTAL.fetch_add(1, Ordering::Relaxed);
    increment_denial_metric("policy", route, reason);
}

pub fn record_rate_limit_rejection(route: &str, reason: &str) {
    RATE_LIMIT_REJECTIONS_TOTAL.fetch_add(1, Ordering::Relaxed);
    increment_denial_metric("rate_limit", route, reason);
}

pub fn record_budget_rejection(route: &str, reason: &str) {
    BUDGET_REJECTIONS_TOTAL.fetch_add(1, Ordering::Relaxed);
    increment_denial_metric("budget", route, reason);
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
    observe_histogram(
        "gateway_first_token_latency_ms",
        "all",
        "all",
        true,
        latency_ms,
        FIRST_TOKEN_BUCKETS_MS,
    );
}

pub fn record_provider_selection() {
    PROVIDER_SELECTIONS_TOTAL.fetch_add(1, Ordering::Relaxed);
}

pub fn record_provider_fallback() {
    record_provider_fallback_with_dimensions("openai_compatible", "litellm", "proxy_error");
}

pub fn record_provider_fallback_with_dimensions(
    from_provider: &str,
    to_provider: &str,
    reason: &str,
) {
    PROVIDER_FALLBACKS_TOTAL.fetch_add(1, Ordering::Relaxed);
    let metrics = PROVIDER_FALLBACK_METRICS.get_or_init(|| Mutex::new(BTreeMap::new()));
    let mut metrics = metrics
        .lock()
        .expect("provider fallback metric lock poisoned");
    let key = ProviderFallbackMetricKey {
        from_provider: sanitize_label(from_provider),
        to_provider: sanitize_label(to_provider),
        reason: sanitize_label(reason),
    };
    *metrics.entry(key).or_default() += 1;
}

pub fn record_circuit_transition() {
    CIRCUIT_TRANSITIONS_TOTAL.fetch_add(1, Ordering::Relaxed);
}

pub fn set_circuit_state(provider: &str, name: &str, state: &str, active: bool) {
    let metrics = CIRCUIT_STATE_METRICS.get_or_init(|| Mutex::new(BTreeMap::new()));
    let mut metrics = metrics.lock().expect("circuit state metric lock poisoned");
    metrics.insert(
        CircuitStateMetricKey {
            provider: sanitize_label(provider),
            name: bounded_name_label(name),
            state: sanitize_label(state),
        },
        if active { 1 } else { 0 },
    );
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
    observe_histogram(
        "gateway_guardrail_duration_ms",
        "all",
        "all",
        false,
        latency_ms,
        DURATION_BUCKETS_MS,
    );
}

pub fn prometheus() -> String {
    let cost = ESTIMATED_COST_MICRO_USD_TOTAL.load(Ordering::Relaxed) as f64 / 1_000_000.0;
    let mut metrics = format!(
        "\
# TYPE gateway_requests_total counter
gateway_requests_total {}
# TYPE gateway_errors_total counter
gateway_errors_total {}
# TYPE gateway_auth_failures_total counter
gateway_auth_failures_total {}
# TYPE gateway_policy_denials_total counter
gateway_policy_denials_total {}
# TYPE gateway_rate_limit_rejections_total counter
gateway_rate_limit_rejections_total {}
# TYPE gateway_budget_rejections_total counter
gateway_budget_rejections_total {}
# TYPE gateway_tokens_total counter
gateway_tokens_total {}
# TYPE gateway_estimated_cost_total counter
gateway_estimated_cost_total {:.6}
# TYPE gateway_active_requests gauge
gateway_active_requests {}
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
        AUTH_FAILURES_TOTAL.load(Ordering::Relaxed),
        POLICY_DENIALS_TOTAL.load(Ordering::Relaxed),
        RATE_LIMIT_REJECTIONS_TOTAL.load(Ordering::Relaxed),
        BUDGET_REJECTIONS_TOTAL.load(Ordering::Relaxed),
        TOKENS_TOTAL.load(Ordering::Relaxed),
        cost,
        ACTIVE_REQUESTS.load(Ordering::Relaxed),
        ACTIVE_STREAMS.load(Ordering::Relaxed),
        STREAM_ABORTS_TOTAL.load(Ordering::Relaxed),
        FIRST_TOKEN_LATENCY_MS_TOTAL.load(Ordering::Relaxed),
        FIRST_TOKEN_LATENCY_SAMPLES.load(Ordering::Relaxed),
        PROVIDER_SELECTIONS_TOTAL.load(Ordering::Relaxed),
        PROVIDER_FALLBACKS_TOTAL.load(Ordering::Relaxed),
        CIRCUIT_TRANSITIONS_TOTAL.load(Ordering::Relaxed),
    );
    render_request_metrics(&mut metrics);
    render_denial_metrics(&mut metrics);
    render_provider_fallback_metrics(&mut metrics);
    render_circuit_state_metrics(&mut metrics);
    render_histograms(&mut metrics);
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

pub fn gateway_request_span(
    request_id: &str,
    route: Option<&str>,
    provider: Option<&str>,
    trace_id: Option<&str>,
) -> Span {
    tracing::info_span!(
        "gateway.request",
        relayna.request_id = %request_id,
        relayna.route = route.unwrap_or("unknown"),
        relayna.provider = provider.unwrap_or("unknown"),
        otel.trace_id = trace_id.unwrap_or(""),
        http.status_code = field::Empty,
    )
}

pub fn phase_span(name: &'static str, request_id: &str) -> Span {
    tracing::info_span!("gateway.phase", relayna.phase = name, relayna.request_id = %request_id)
}

fn increment_request_metric(route: &str, provider: &str, status_code: u16) {
    let metrics = REQUEST_METRICS.get_or_init(|| Mutex::new(BTreeMap::new()));
    let mut metrics = metrics.lock().expect("request metric lock poisoned");
    let key = RequestMetricKey {
        route: bounded_route_label(route),
        provider: sanitize_label(provider),
        status_class: status_class(status_code).to_owned(),
    };
    *metrics.entry(key).or_default() += 1;
}

fn increment_denial_metric(kind: &str, route: &str, reason: &str) {
    let metrics = DENIAL_METRICS.get_or_init(|| Mutex::new(BTreeMap::new()));
    let mut metrics = metrics.lock().expect("denial metric lock poisoned");
    let key = DenialMetricKey {
        kind: sanitize_label(kind),
        route: bounded_route_label(route),
        reason: sanitize_label(reason),
    };
    *metrics.entry(key).or_default() += 1;
}

fn observe_histogram(
    metric: &str,
    route: &str,
    provider: &str,
    stream: bool,
    latency_ms: u64,
    buckets: &[u64],
) {
    let histograms = HISTOGRAMS.get_or_init(|| Mutex::new(BTreeMap::new()));
    let mut histograms = histograms.lock().expect("histogram lock poisoned");
    let value = histograms
        .entry(HistogramKey {
            metric: metric.to_owned(),
            route: bounded_route_label(route),
            provider: sanitize_label(provider),
            stream: if stream { "true" } else { "false" }.to_owned(),
        })
        .or_default();
    value.sum_ms = value.sum_ms.saturating_add(latency_ms);
    value.count = value.count.saturating_add(1);
    for bucket in buckets {
        if latency_ms <= *bucket {
            *value.buckets.entry(*bucket).or_default() += 1;
        }
    }
}

fn render_request_metrics(metrics: &mut String) {
    metrics.push_str("# TYPE gateway_requests_by_dimension_total counter\n");
    if let Some(values) = REQUEST_METRICS.get() {
        for (key, value) in values.lock().expect("request metric lock poisoned").iter() {
            metrics.push_str(&format!(
                "gateway_requests_by_dimension_total{{route=\"{}\",provider=\"{}\",status_class=\"{}\"}} {}\n",
                key.route, key.provider, key.status_class, value
            ));
        }
    }
}

fn render_denial_metrics(metrics: &mut String) {
    metrics.push_str("# TYPE gateway_denials_total counter\n");
    if let Some(values) = DENIAL_METRICS.get() {
        for (key, value) in values.lock().expect("denial metric lock poisoned").iter() {
            metrics.push_str(&format!(
                "gateway_denials_total{{kind=\"{}\",route=\"{}\",reason=\"{}\"}} {}\n",
                key.kind, key.route, key.reason, value
            ));
        }
    }
}

fn render_provider_fallback_metrics(metrics: &mut String) {
    metrics.push_str("# TYPE gateway_provider_fallbacks_by_provider_total counter\n");
    if let Some(values) = PROVIDER_FALLBACK_METRICS.get() {
        for (key, value) in values
            .lock()
            .expect("provider fallback metric lock poisoned")
            .iter()
        {
            metrics.push_str(&format!(
                "gateway_provider_fallbacks_by_provider_total{{from_provider=\"{}\",to_provider=\"{}\",reason=\"{}\"}} {}\n",
                key.from_provider, key.to_provider, key.reason, value
            ));
        }
    }
}

fn render_circuit_state_metrics(metrics: &mut String) {
    metrics.push_str("# TYPE gateway_circuit_breaker_state gauge\n");
    if let Some(values) = CIRCUIT_STATE_METRICS.get() {
        for (key, value) in values
            .lock()
            .expect("circuit state metric lock poisoned")
            .iter()
        {
            metrics.push_str(&format!(
                "gateway_circuit_breaker_state{{provider=\"{}\",name=\"{}\",state=\"{}\"}} {}\n",
                key.provider, key.name, key.state, value
            ));
        }
    }
}

fn render_histograms(metrics: &mut String) {
    if let Some(values) = HISTOGRAMS.get() {
        let values = values.lock().expect("histogram lock poisoned");
        let mut rendered_types = BTreeMap::new();
        for (key, value) in values.iter() {
            if rendered_types.insert(key.metric.clone(), ()).is_none() {
                metrics.push_str(&format!("# TYPE {} histogram\n", key.metric));
            }
            let labels = format!(
                "route=\"{}\",provider=\"{}\",stream=\"{}\"",
                key.route, key.provider, key.stream
            );
            for (bucket, count) in &value.buckets {
                metrics.push_str(&format!(
                    "{}_bucket{{{},le=\"{}\"}} {}\n",
                    key.metric, labels, bucket, count
                ));
            }
            metrics.push_str(&format!(
                "{}_bucket{{{},le=\"+Inf\"}} {}\n",
                key.metric, labels, value.count
            ));
            metrics.push_str(&format!(
                "{}_sum{{{}}} {}\n",
                key.metric, labels, value.sum_ms
            ));
            metrics.push_str(&format!(
                "{}_count{{{}}} {}\n",
                key.metric, labels, value.count
            ));
        }
    }
}

fn status_class(status_code: u16) -> &'static str {
    match status_code {
        100..=199 => "1xx",
        200..=299 => "2xx",
        300..=399 => "3xx",
        400..=499 => "4xx",
        500..=599 => "5xx",
        _ => "unknown",
    }
}

fn bounded_route_label(route: &str) -> String {
    match route {
        "chat_completions" | "responses" | "direct_openai" | "summary" | "translation" | "ocr"
        | "embeddings" | "service_wildcard" | "unknown" | "all" => route.to_owned(),
        _ => sanitize_label(route),
    }
}

fn bounded_name_label(value: &str) -> String {
    sanitize_label(value).chars().take(64).collect()
}

fn sanitize_label(value: &str) -> String {
    let sanitized: String = value
        .chars()
        .filter(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
        })
        .take(128)
        .collect();
    if sanitized.is_empty() {
        "unknown".to_owned()
    } else {
        sanitized
    }
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
        super::record_upstream_duration_ms("chat_completions", "litellm", false, 24);
        super::record_guardrail_execution(
            "pii_redact",
            "pre_call",
            "allow",
            "fail_closed",
            8,
            false,
        );
        let metrics = super::prometheus();
        assert!(metrics.contains("gateway_requests_total"));
        assert!(metrics.contains("gateway_active_streams"));
        assert!(metrics.contains("gateway_active_requests"));
        assert!(metrics.contains("gateway_first_token_latency_ms"));
        assert!(metrics.contains("gateway_request_duration_ms"));
        assert!(metrics.contains("gateway_upstream_duration_ms"));
        assert!(metrics.contains("gateway_guardrail_duration_ms"));
        assert!(metrics.contains("gateway_provider_selections_total"));
        assert!(metrics.contains("gateway_provider_fallbacks_total"));
    }

    #[test]
    fn renders_bounded_metric_labels() {
        super::record_request_with_dimensions("chat_completions", "litellm", 200, 42, false);
        super::record_policy_denial("chat_completions", "policy_denied");
        let metrics = super::prometheus();
        assert!(metrics.contains("route=\"chat_completions\""));
        assert!(metrics.contains("provider=\"litellm\""));
        assert!(!metrics.contains("request_id"));
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
