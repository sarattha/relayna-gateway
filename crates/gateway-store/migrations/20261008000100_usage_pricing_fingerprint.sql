-- A nullable proof of the selector chosen when the event was priced.
-- Never infer a fingerprint for legacy rows from the current service config.
ALTER TABLE usage_events
ADD COLUMN IF NOT EXISTS pricing_rule_fingerprint text;
