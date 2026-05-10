CREATE TABLE IF NOT EXISTS operator_tokens (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    token_prefix text NOT NULL UNIQUE,
    token_hash text NOT NULL,
    disabled boolean NOT NULL DEFAULT false,
    revoked_at timestamptz,
    last_used_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS operator_tokens_active_idx
    ON operator_tokens (created_at DESC)
    WHERE disabled = false AND revoked_at IS NULL;

CREATE UNIQUE INDEX IF NOT EXISTS operator_tokens_one_active_idx
    ON operator_tokens ((true))
    WHERE disabled = false AND revoked_at IS NULL;
