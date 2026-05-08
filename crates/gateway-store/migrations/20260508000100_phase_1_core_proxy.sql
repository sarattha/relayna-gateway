CREATE EXTENSION IF NOT EXISTS pgcrypto;

CREATE TABLE IF NOT EXISTS api_keys (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id uuid NOT NULL,
    key_prefix text NOT NULL UNIQUE,
    key_hash text NOT NULL,
    disabled boolean NOT NULL DEFAULT false,
    expires_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS route_policies (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    key_id uuid NOT NULL REFERENCES api_keys(id) ON DELETE CASCADE,
    route text NOT NULL,
    allowed boolean NOT NULL DEFAULT true,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (key_id, route)
);

CREATE TABLE IF NOT EXISTS usage_events (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    request_id text NOT NULL,
    key_id uuid NOT NULL REFERENCES api_keys(id) ON DELETE RESTRICT,
    project_id uuid NOT NULL,
    route text NOT NULL,
    model text,
    provider text NOT NULL,
    status text NOT NULL,
    status_code integer NOT NULL,
    latency_ms bigint NOT NULL,
    input_tokens bigint,
    output_tokens bigint,
    estimated_cost numeric(20, 8),
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS usage_events_key_created_at_idx
    ON usage_events (key_id, created_at DESC);

CREATE INDEX IF NOT EXISTS usage_events_project_created_at_idx
    ON usage_events (project_id, created_at DESC);

CREATE INDEX IF NOT EXISTS usage_events_request_id_idx
    ON usage_events (request_id);
