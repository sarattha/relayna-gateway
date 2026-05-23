ALTER TABLE api_keys
ADD COLUMN IF NOT EXISTS rotation_due_at timestamptz,
ADD COLUMN IF NOT EXISTS last_used_at timestamptz;

ALTER TABLE key_policies
ADD COLUMN IF NOT EXISTS deny boolean NOT NULL DEFAULT false,
ADD COLUMN IF NOT EXISTS max_requests_per_day integer,
ADD COLUMN IF NOT EXISTS max_tokens_per_day integer,
ADD COLUMN IF NOT EXISTS max_cost_per_request double precision,
ADD COLUMN IF NOT EXISTS max_input_tokens_per_request integer,
ADD COLUMN IF NOT EXISTS max_output_tokens_per_request integer,
ADD COLUMN IF NOT EXISTS allowed_hours_utc integer[] NOT NULL DEFAULT ARRAY[]::integer[],
ADD COLUMN IF NOT EXISTS unused_key_auto_disable_after_days integer,
ADD COLUMN IF NOT EXISTS max_request_body_bytes bigint,
ADD COLUMN IF NOT EXISTS max_response_body_bytes bigint,
ADD COLUMN IF NOT EXISTS max_stream_duration_seconds integer,
ADD COLUMN IF NOT EXISTS max_sse_event_bytes bigint,
ADD COLUMN IF NOT EXISTS max_tool_call_count integer,
ADD COLUMN IF NOT EXISTS max_tool_schema_bytes bigint,
ADD COLUMN IF NOT EXISTS policy_version bigint NOT NULL DEFAULT 1;

CREATE TABLE IF NOT EXISTS policy_layers (
    id uuid PRIMARY KEY,
    layer_kind text NOT NULL,
    scope_id text,
    policy jsonb NOT NULL DEFAULT '{}'::jsonb,
    guardrail_policy jsonb NOT NULL DEFAULT '{}'::jsonb,
    policy_version bigint NOT NULL DEFAULT 1,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (layer_kind, scope_id)
);

CREATE INDEX IF NOT EXISTS idx_policy_layers_kind_scope
ON policy_layers (layer_kind, scope_id);

CREATE UNIQUE INDEX IF NOT EXISTS policy_layers_kind_scope_unique_idx
ON policy_layers (layer_kind, COALESCE(scope_id, ''));
