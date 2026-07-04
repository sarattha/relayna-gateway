ALTER TABLE service_registrations
ADD COLUMN IF NOT EXISTS pricing_rules jsonb NOT NULL DEFAULT '[]'::jsonb;

ALTER TABLE usage_events
ADD COLUMN IF NOT EXISTS cost_source text,
ADD COLUMN IF NOT EXISTS cost_mode text,
ADD COLUMN IF NOT EXISTS pricing_rule_name text;

CREATE INDEX IF NOT EXISTS usage_events_cost_source_created_at_idx
ON usage_events (cost_source, created_at DESC)
WHERE cost_source IS NOT NULL;

CREATE INDEX IF NOT EXISTS usage_events_pricing_rule_created_at_idx
ON usage_events (pricing_rule_name, created_at DESC)
WHERE pricing_rule_name IS NOT NULL;
