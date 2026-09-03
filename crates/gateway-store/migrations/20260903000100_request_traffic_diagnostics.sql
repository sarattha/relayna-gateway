-- Additive: existing billing rows keep empty diagnostics; anonymous traffic lives
-- separately and never requires a fabricated virtual key or billable usage row.
ALTER TABLE usage_events ADD COLUMN diagnostics jsonb NOT NULL DEFAULT '{}'::jsonb;
CREATE TABLE request_traffic (
    id uuid PRIMARY KEY,
    instance_id text NOT NULL,
    request_id text NOT NULL,
    started_at timestamptz NOT NULL,
    project_id uuid,
    key_id uuid,
    service text,
    client_status integer,
    failed boolean NOT NULL,
    record jsonb NOT NULL
);
CREATE INDEX request_traffic_time_idx ON request_traffic (started_at DESC, id DESC);
CREATE INDEX request_traffic_request_idx ON request_traffic (request_id, started_at DESC);
CREATE INDEX request_traffic_failure_idx ON request_traffic (started_at DESC, id DESC) WHERE failed;
