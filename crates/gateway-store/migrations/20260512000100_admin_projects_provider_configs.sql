CREATE TABLE IF NOT EXISTS projects (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    name text NOT NULL UNIQUE,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT projects_name_format CHECK (length(trim(name)) > 0 AND length(name) <= 120)
);

INSERT INTO projects (id, name)
SELECT DISTINCT project_id, 'project-' || left(project_id::text, 8)
FROM api_keys
ON CONFLICT (id) DO NOTHING;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM pg_constraint
        WHERE conname = 'api_keys_project_id_fkey'
    ) THEN
        ALTER TABLE api_keys
            ADD CONSTRAINT api_keys_project_id_fkey
            FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE RESTRICT;
    END IF;
END $$;

ALTER TABLE service_registrations
    ADD COLUMN IF NOT EXISTS project_id uuid REFERENCES projects(id) ON DELETE RESTRICT;

CREATE INDEX IF NOT EXISTS service_registrations_project_id_idx
    ON service_registrations (project_id)
    WHERE project_id IS NOT NULL;

CREATE TABLE IF NOT EXISTS provider_configs (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    provider text NOT NULL,
    name text NOT NULL,
    base_url text NOT NULL,
    enabled boolean NOT NULL DEFAULT true,
    credential_secret text,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT provider_configs_provider_check CHECK (provider IN ('litellm', 'internal-service')),
    CONSTRAINT provider_configs_name_format CHECK (length(trim(name)) > 0 AND length(name) <= 120),
    CONSTRAINT provider_configs_base_url_format CHECK (base_url ~ '^https?://')
);

CREATE UNIQUE INDEX IF NOT EXISTS provider_configs_provider_name_idx
    ON provider_configs (provider, name);

CREATE UNIQUE INDEX IF NOT EXISTS provider_configs_one_enabled_litellm_idx
    ON provider_configs (provider)
    WHERE provider = 'litellm' AND enabled;
