ALTER TABLE key_policies
ADD COLUMN IF NOT EXISTS allowed_services text[] NOT NULL DEFAULT ARRAY[]::text[];

ALTER TABLE usage_events
ADD COLUMN IF NOT EXISTS total_tokens bigint,
ADD COLUMN IF NOT EXISTS service_name text,
ADD COLUMN IF NOT EXISTS fallback_count integer NOT NULL DEFAULT 0;

CREATE INDEX IF NOT EXISTS usage_events_provider_created_at_idx
    ON usage_events (provider, created_at DESC);

CREATE INDEX IF NOT EXISTS usage_events_service_created_at_idx
    ON usage_events (service_name, created_at DESC)
    WHERE service_name IS NOT NULL;

CREATE INDEX IF NOT EXISTS usage_events_model_created_at_idx
    ON usage_events (model, created_at DESC)
    WHERE model IS NOT NULL;
