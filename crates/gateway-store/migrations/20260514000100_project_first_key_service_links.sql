ALTER TABLE api_keys
    ADD COLUMN IF NOT EXISTS owner_type text NOT NULL DEFAULT 'project',
    ALTER COLUMN project_id DROP NOT NULL;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM pg_constraint
        WHERE conname = 'api_keys_owner_type_check'
    ) THEN
        ALTER TABLE api_keys
            ADD CONSTRAINT api_keys_owner_type_check
            CHECK (owner_type IN ('project', 'individual')) NOT VALID;
    END IF;
END $$;

ALTER TABLE api_keys VALIDATE CONSTRAINT api_keys_owner_type_check;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM pg_constraint
        WHERE conname = 'api_keys_owner_project_check'
    ) THEN
        ALTER TABLE api_keys
            ADD CONSTRAINT api_keys_owner_project_check
            CHECK (
                (owner_type = 'project' AND project_id IS NOT NULL)
                OR (owner_type = 'individual' AND project_id IS NULL)
            ) NOT VALID;
    END IF;
END $$;

ALTER TABLE api_keys VALIDATE CONSTRAINT api_keys_owner_project_check;

ALTER TABLE usage_events
    ALTER COLUMN project_id DROP NOT NULL;

CREATE TABLE IF NOT EXISTS project_service_links (
    project_id uuid NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    service_name text NOT NULL REFERENCES service_registrations(name) ON DELETE CASCADE,
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (project_id, service_name)
);

CREATE INDEX IF NOT EXISTS project_service_links_service_name_idx
    ON project_service_links (service_name);

CREATE TABLE IF NOT EXISTS key_service_links (
    key_id uuid NOT NULL REFERENCES api_keys(id) ON DELETE CASCADE,
    service_name text NOT NULL REFERENCES service_registrations(name) ON DELETE CASCADE,
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (key_id, service_name)
);

CREATE INDEX IF NOT EXISTS key_service_links_service_name_idx
    ON key_service_links (service_name);

INSERT INTO project_service_links (project_id, service_name)
SELECT project_id, name
FROM service_registrations
WHERE project_id IS NOT NULL
ON CONFLICT DO NOTHING;
