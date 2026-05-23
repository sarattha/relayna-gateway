ALTER TABLE usage_events
ADD COLUMN IF NOT EXISTS trace_id text;

ALTER TABLE request_debug_bundles
ADD COLUMN IF NOT EXISTS trace_id text;

CREATE INDEX IF NOT EXISTS usage_events_run_created_at_idx
    ON usage_events (run_id, created_at DESC)
    WHERE run_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS usage_events_status_created_at_idx
    ON usage_events (status, created_at DESC);

CREATE INDEX IF NOT EXISTS usage_events_cost_created_at_idx
    ON usage_events (estimated_cost DESC, created_at DESC)
    WHERE estimated_cost IS NOT NULL;

CREATE INDEX IF NOT EXISTS usage_events_trace_id_idx
    ON usage_events (trace_id)
    WHERE trace_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS request_debug_bundles_trace_id_idx
    ON request_debug_bundles (trace_id)
    WHERE trace_id IS NOT NULL;
