ALTER TABLE operator_tokens
    ADD COLUMN IF NOT EXISTS roles text[] NOT NULL DEFAULT ARRAY['owner']::text[],
    ADD COLUMN IF NOT EXISTS scopes text[] NOT NULL DEFAULT ARRAY['*']::text[];

CREATE TABLE IF NOT EXISTS audit_events (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    actor_token_id uuid NOT NULL REFERENCES operator_tokens(id),
    action text NOT NULL,
    target_type text NOT NULL,
    target_id text,
    before_json jsonb,
    after_json jsonb,
    request_id text NOT NULL,
    ip text,
    user_agent text,
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS audit_events_created_at_idx
    ON audit_events (created_at DESC);

CREATE INDEX IF NOT EXISTS audit_events_actor_created_at_idx
    ON audit_events (actor_token_id, created_at DESC);

CREATE INDEX IF NOT EXISTS audit_events_target_created_at_idx
    ON audit_events (target_type, target_id, created_at DESC);
