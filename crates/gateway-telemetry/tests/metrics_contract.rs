use gateway_telemetry::{
    gateway_request_span, init, is_sensitive_field, phase_span, prometheus, record_auth_failure,
    record_budget_rejection, record_circuit_transition, record_estimated_cost_usd,
    record_first_token_latency_ms, record_guardrail_execution, record_policy_denial,
    record_provider_fallback, record_provider_fallback_with_dimensions, record_provider_selection,
    record_rate_limit_rejection, record_request, record_request_with_dimensions, record_tokens,
    record_upstream_duration_ms, request_finished, request_started, set_circuit_state,
    stream_finished, stream_started,
};

#[test]
fn metrics_contract_records_every_counter_histogram_and_dimension() {
    init("not a valid directive[");
    init("gateway_telemetry=debug");
    request_started();
    record_request_with_dimensions("chat_completions", "litellm", 101, 1, false);
    record_request_with_dimensions("custom /route", "provider!", 204, 4, false);
    record_request_with_dimensions("responses", "litellm", 302, 26, true);
    record_request_with_dimensions("ocr", "internal-service", 404, 501, false);
    record_request_with_dimensions("unknown", "provider", 503, 121_000, true);
    record_request_with_dimensions("unknown", "provider", 700, 1, false);
    record_request(500);
    record_upstream_duration_ms("summary", "internal-service", false, 12);
    record_auth_failure("invalid token!");
    record_policy_denial("translation", "denied");
    record_rate_limit_rejection("ocr", "rpm");
    record_budget_rejection("embeddings", "daily budget");
    record_tokens(17);
    record_tokens(-1);
    record_estimated_cost_usd(0.25);
    record_estimated_cost_usd(f64::NAN);
    record_estimated_cost_usd(-1.0);
    stream_started();
    record_first_token_latency_ms(75);
    stream_finished(true);
    stream_started();
    stream_finished(false);
    record_provider_selection();
    record_provider_fallback();
    record_provider_fallback_with_dimensions("primary!", "fallback@", "timeout reason");
    record_circuit_transition();
    set_circuit_state(
        "internal-service",
        &"very-long-service-name".repeat(8),
        "half open",
        true,
    );
    set_circuit_state("internal-service", "service", "closed", false);
    record_guardrail_execution(
        "custom guardrail!",
        "pre_call",
        "block",
        "fail_closed",
        31,
        true,
    );
    request_finished();

    let request_span = gateway_request_span("request", None, None, None);
    request_span.record("http.status_code", 200);
    let _phase = phase_span("coverage", "request");
    let metrics = prometheus();
    for expected in [
        "gateway_requests_total",
        "gateway_errors_total",
        "gateway_denials_total",
        "gateway_first_token_latency_ms_bucket",
        "gateway_provider_fallbacks_by_provider_total",
        "gateway_circuit_breaker_state",
        "gateway_guardrail_failures_total",
        "status_class=\"1xx\"",
        "status_class=\"2xx\"",
        "status_class=\"3xx\"",
        "status_class=\"4xx\"",
        "status_class=\"5xx\"",
        "status_class=\"unknown\"",
    ] {
        assert!(metrics.contains(expected), "missing metric {expected}");
    }
    assert!(is_sensitive_field("proxy-authorization"));
    assert!(is_sensitive_field("db_password"));
    assert!(!is_sensitive_field("safe-field"));
}
