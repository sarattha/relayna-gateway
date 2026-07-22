ALTER TABLE service_registrations
ADD COLUMN IF NOT EXISTS openapi_source_path text,
ADD COLUMN IF NOT EXISTS openapi_schema_hash text,
ADD COLUMN IF NOT EXISTS openapi_synced_at timestamptz,
ADD COLUMN IF NOT EXISTS openapi_endpoints jsonb NOT NULL DEFAULT '[]'::jsonb,
ADD COLUMN IF NOT EXISTS endpoint_pricing_rules jsonb NOT NULL DEFAULT '[]'::jsonb;

ALTER TABLE service_registrations
DROP CONSTRAINT IF EXISTS service_registrations_openapi_source_path_check;

ALTER TABLE service_registrations
ADD CONSTRAINT service_registrations_openapi_source_path_check CHECK (
    openapi_source_path IS NULL
    OR (
        length(openapi_source_path) BETWEEN 1 AND 512
        AND left(openapi_source_path, 1) = '/'
        AND left(openapi_source_path, 2) <> '//'
        AND position('//' IN openapi_source_path) = 0
        AND position('?' IN openapi_source_path) = 0
        AND position('#' IN openapi_source_path) = 0
        AND position(E'\\' IN openapi_source_path) = 0
    )
);

CREATE INDEX IF NOT EXISTS service_registrations_openapi_synced_at_idx
ON service_registrations (openapi_synced_at DESC)
WHERE openapi_synced_at IS NOT NULL;
