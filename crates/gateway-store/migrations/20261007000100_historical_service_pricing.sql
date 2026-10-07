-- Reports may be repriced, but budget reconstruction must retain the original
-- committed charge. NULL means the report cost has never been repriced.
ALTER TABLE usage_events
ADD COLUMN IF NOT EXISTS budget_estimated_cost numeric(20, 8);

-- Historical updates page by service and UUID; avoid repeatedly sorting the
-- service's complete history while processing bounded batches.
CREATE INDEX IF NOT EXISTS usage_events_service_id_idx
ON usage_events (service_name, id)
WHERE service_name IS NOT NULL;
