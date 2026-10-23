-- Additive completion-time browsing. Legacy start-time history stays unchanged.
-- Unknown completion times remain NULL rather than inventing an end timestamp.
ALTER TABLE request_traffic ADD COLUMN completed_at timestamptz;
ALTER TABLE request_traffic ADD COLUMN protocol text;

-- No historical UPDATE/backfill: adding nullable columns does not rewrite the
-- ledger. Protocol filters read legacy metadata when this column is NULL.

CREATE INDEX request_traffic_completed_idx
    ON request_traffic (completed_at DESC, id DESC) WHERE completed_at IS NOT NULL;
CREATE INDEX request_traffic_protocol_completed_idx
    ON request_traffic (protocol, completed_at DESC, id DESC) WHERE completed_at IS NOT NULL;
CREATE INDEX request_traffic_project_completed_idx
    ON request_traffic (project_id, completed_at DESC, id DESC) WHERE completed_at IS NOT NULL;
CREATE INDEX request_traffic_key_completed_idx
    ON request_traffic (key_id, completed_at DESC, id DESC) WHERE completed_at IS NOT NULL;
CREATE INDEX request_traffic_service_completed_idx
    ON request_traffic (service, completed_at DESC, id DESC) WHERE completed_at IS NOT NULL;
