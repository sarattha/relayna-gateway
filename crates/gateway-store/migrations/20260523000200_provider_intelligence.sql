CREATE TABLE IF NOT EXISTS provider_health_states (
    name text PRIMARY KEY,
    provider text NOT NULL,
    status text NOT NULL DEFAULT 'unknown',
    circuit_state text NOT NULL DEFAULT 'closed',
    active_check_ok boolean,
    passive_success_count bigint NOT NULL DEFAULT 0,
    passive_failure_count bigint NOT NULL DEFAULT 0,
    consecutive_failures integer NOT NULL DEFAULT 0,
    average_latency_ms bigint,
    last_error_code text,
    cooldown_until timestamptz,
    checked_at timestamptz,
    updated_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT provider_health_states_status_check CHECK (status IN ('healthy', 'degraded', 'unhealthy', 'unknown')),
    CONSTRAINT provider_health_states_circuit_check CHECK (circuit_state IN ('closed', 'open', 'half_open')),
    CONSTRAINT provider_health_states_counts_check CHECK (
        passive_success_count >= 0
        AND passive_failure_count >= 0
        AND consecutive_failures >= 0
    )
);

CREATE INDEX IF NOT EXISTS provider_health_states_provider_idx
    ON provider_health_states (provider);

CREATE INDEX IF NOT EXISTS provider_health_states_circuit_idx
    ON provider_health_states (circuit_state, status);

CREATE TABLE IF NOT EXISTS request_debug_bundles (
    request_id text PRIMARY KEY,
    route text,
    provider text,
    service_name text,
    policy_trace jsonb NOT NULL DEFAULT '[]'::jsonb,
    guardrail_trace jsonb NOT NULL DEFAULT '[]'::jsonb,
    selection_trace jsonb NOT NULL DEFAULT '[]'::jsonb,
    fallback_history jsonb NOT NULL DEFAULT '[]'::jsonb,
    upstream_latency_ms bigint,
    request_hash text,
    response_hash text,
    redaction_version integer NOT NULL DEFAULT 1,
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS request_debug_bundles_created_at_idx
    ON request_debug_bundles (created_at DESC);

CREATE TABLE IF NOT EXISTS service_registry_snapshots (
    version bigserial PRIMARY KEY,
    source text NOT NULL,
    diff jsonb NOT NULL,
    services_json jsonb NOT NULL,
    activated_at timestamptz,
    rolled_back_from_version bigint,
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS service_registry_snapshots_created_at_idx
    ON service_registry_snapshots (created_at DESC);
