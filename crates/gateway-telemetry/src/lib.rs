use tracing_subscriber::{fmt, EnvFilter};

pub fn init(log_level: &str) {
    let filter =
        EnvFilter::try_new(log_level).unwrap_or_else(|_| EnvFilter::new("gateway_api=info"));
    let _ = fmt().with_env_filter(filter).json().try_init();
}

use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};

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

pub fn prometheus() -> String {
    let cost = ESTIMATED_COST_MICRO_USD_TOTAL.load(Ordering::Relaxed) as f64 / 1_000_000.0;
    format!(
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
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn renders_expected_metric_names() {
        let metrics = super::prometheus();
        assert!(metrics.contains("gateway_requests_total"));
        assert!(metrics.contains("gateway_active_streams"));
        assert!(metrics.contains("gateway_first_token_latency_ms"));
    }
}
