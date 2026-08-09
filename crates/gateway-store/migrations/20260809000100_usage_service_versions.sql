ALTER TABLE usage_events
ADD COLUMN IF NOT EXISTS service_version TEXT;
