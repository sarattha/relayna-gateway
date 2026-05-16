CREATE TABLE IF NOT EXISTS guardrail_definitions (
    name text PRIMARY KEY,
    description text NOT NULL,
    modes text[] NOT NULL,
    default_on boolean NOT NULL DEFAULT false,
    failure_policy text NOT NULL,
    config_schema jsonb NOT NULL DEFAULT '{}'::jsonb,
    config jsonb NOT NULL DEFAULT '{}'::jsonb,
    enabled boolean NOT NULL DEFAULT true,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now()
);

INSERT INTO guardrail_definitions (
    name,
    description,
    modes,
    default_on,
    failure_policy,
    config_schema,
    config,
    enabled
)
VALUES (
    'pii-redact',
    'Redacts common PII before provider calls and optionally restores placeholders after responses.',
    ARRAY['pre_call', 'post_call', 'during_call'],
    false,
    'fail_closed',
    '{"restore_output":"boolean"}'::jsonb,
    '{"restore_output":true}'::jsonb,
    true
)
ON CONFLICT (name) DO UPDATE SET
    description = EXCLUDED.description,
    modes = EXCLUDED.modes,
    default_on = EXCLUDED.default_on,
    failure_policy = EXCLUDED.failure_policy,
    config_schema = EXCLUDED.config_schema,
    config = EXCLUDED.config,
    enabled = EXCLUDED.enabled,
    updated_at = now();

CREATE TABLE IF NOT EXISTS key_guardrail_policies (
    key_id uuid PRIMARY KEY REFERENCES api_keys(id) ON DELETE CASCADE,
    mandatory_guardrails text[] NOT NULL DEFAULT ARRAY[]::text[],
    optional_guardrails text[] NOT NULL DEFAULT ARRAY[]::text[],
    forbidden_guardrails text[] NOT NULL DEFAULT ARRAY[]::text[],
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS guardrail_execution_events (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    request_id text NOT NULL,
    key_id uuid REFERENCES api_keys(id) ON DELETE SET NULL,
    project_id uuid REFERENCES projects(id) ON DELETE SET NULL,
    route text,
    model text,
    provider text,
    guardrail_name text NOT NULL,
    mode text NOT NULL,
    action text NOT NULL,
    failure_policy text NOT NULL,
    latency_ms bigint NOT NULL,
    reason text,
    metadata jsonb NOT NULL DEFAULT '{}'::jsonb,
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS guardrail_execution_events_request_id_idx
    ON guardrail_execution_events (request_id);

CREATE INDEX IF NOT EXISTS guardrail_execution_events_key_created_at_idx
    ON guardrail_execution_events (key_id, created_at DESC)
    WHERE key_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS guardrail_execution_events_guardrail_created_at_idx
    ON guardrail_execution_events (guardrail_name, created_at DESC);

CREATE INDEX IF NOT EXISTS guardrail_execution_events_project_created_at_idx
    ON guardrail_execution_events (project_id, created_at DESC)
    WHERE project_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS guardrail_execution_events_mode_action_created_at_idx
    ON guardrail_execution_events (mode, action, created_at DESC);
