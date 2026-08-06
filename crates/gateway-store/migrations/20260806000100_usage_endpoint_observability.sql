ALTER TABLE usage_events
ADD COLUMN IF NOT EXISTS http_method text,
ADD COLUMN IF NOT EXISTS endpoint_path text,
ADD COLUMN IF NOT EXISTS endpoint_template text;

CREATE INDEX IF NOT EXISTS usage_events_service_endpoint_status_created_at_idx
ON usage_events (
    service_name,
    http_method,
    (COALESCE(endpoint_template, endpoint_path)),
    status_code,
    created_at DESC
)
WHERE endpoint_path IS NOT NULL;
