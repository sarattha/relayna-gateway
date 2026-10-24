-- Unscoped recent Usage pages previously sorted the entire matching ledger.
-- The mixed direction matches recent events; backwards traversal and exports
-- use the ascending index. UUID makes duplicate request IDs a total order.
CREATE INDEX IF NOT EXISTS usage_events_chronological_idx
    ON usage_events (created_at ASC, request_id ASC, id ASC);

CREATE INDEX IF NOT EXISTS usage_events_recent_chronological_idx
    ON usage_events (created_at DESC, request_id ASC, id ASC);

-- Guardrail actions are counted only after selecting a bounded event page.
CREATE INDEX IF NOT EXISTS guardrail_execution_events_usage_identity_idx
    ON guardrail_execution_events (request_id, key_id, project_id);
