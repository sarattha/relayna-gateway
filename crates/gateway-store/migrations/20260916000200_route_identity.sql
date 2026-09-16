CREATE TABLE route_identity_settings (
    route TEXT PRIMARY KEY,
    access JSONB NOT NULL DEFAULT '{}',
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
