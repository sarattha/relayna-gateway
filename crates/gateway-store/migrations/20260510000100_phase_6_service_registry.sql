CREATE TABLE IF NOT EXISTS service_registrations (
    name text PRIMARY KEY,
    studio_service_id text UNIQUE,
    route_pattern text NOT NULL,
    upstream_base_url text,
    enabled boolean NOT NULL DEFAULT false,
    allowed_methods text[] NOT NULL DEFAULT ARRAY['POST']::text[],
    timeout_ms bigint NOT NULL DEFAULT 60000,
    max_body_bytes bigint NOT NULL DEFAULT 2097152,
    cost_mode text NOT NULL DEFAULT 'none',
    estimated_cost_usd double precision,
    credential_secret text,
    fallback_services text[] NOT NULL DEFAULT ARRAY[]::text[],
    source text NOT NULL DEFAULT 'gateway',
    sync_status text NOT NULL DEFAULT 'local',
    last_synced_at timestamptz,
    disabled_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT service_registrations_name_format CHECK (name ~ '^[a-z0-9]([a-z0-9-]{0,62}[a-z0-9])?$'),
    CONSTRAINT service_registrations_source_check CHECK (source IN ('gateway', 'studio')),
    CONSTRAINT service_registrations_sync_status_check CHECK (sync_status IN ('local', 'synced', 'incomplete', 'stale', 'failed')),
    CONSTRAINT service_registrations_cost_mode_check CHECK (cost_mode IN ('fixed', 'passthrough', 'none')),
    CONSTRAINT service_registrations_limits_check CHECK (timeout_ms > 0 AND max_body_bytes > 0)
);

CREATE UNIQUE INDEX IF NOT EXISTS service_registrations_studio_service_id_idx
    ON service_registrations (studio_service_id)
    WHERE studio_service_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS service_registrations_source_status_idx
    ON service_registrations (source, sync_status);
