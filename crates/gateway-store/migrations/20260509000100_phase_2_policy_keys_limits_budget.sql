ALTER TABLE api_keys
ADD COLUMN IF NOT EXISTS revoked_at timestamptz;

CREATE TABLE IF NOT EXISTS key_policies (
    key_id uuid PRIMARY KEY REFERENCES api_keys(id) ON DELETE CASCADE,
    allowed_routes text[] NOT NULL DEFAULT ARRAY['/v1/chat/completions', '/v1/responses'],
    allowed_models text[] NOT NULL DEFAULT ARRAY[]::text[],
    allowed_providers text[] NOT NULL DEFAULT ARRAY['litellm'],
    rpm_limit integer,
    tpm_limit integer,
    daily_budget_usd double precision,
    monthly_budget_usd double precision,
    allow_streaming boolean NOT NULL DEFAULT false,
    allow_tools boolean NOT NULL DEFAULT false,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_key_policies_limits
ON key_policies (key_id)
WHERE rpm_limit IS NOT NULL
   OR tpm_limit IS NOT NULL
   OR daily_budget_usd IS NOT NULL
   OR monthly_budget_usd IS NOT NULL;
