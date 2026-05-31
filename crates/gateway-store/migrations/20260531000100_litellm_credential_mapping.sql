ALTER TABLE provider_configs
ADD COLUMN IF NOT EXISTS credential_header_mode text NOT NULL DEFAULT 'authorization_bearer',
ADD COLUMN IF NOT EXISTS credential_header_name text,
ADD CONSTRAINT provider_configs_credential_header_mode_check
    CHECK (credential_header_mode IN ('authorization_bearer', 'custom_header')),
ADD CONSTRAINT provider_configs_custom_header_name_check
    CHECK (
        credential_header_mode <> 'custom_header'
        OR credential_header_name IS NOT NULL
    );

CREATE TABLE IF NOT EXISTS litellm_credential_mappings (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    scope text NOT NULL,
    key_id uuid REFERENCES api_keys(id) ON DELETE CASCADE,
    project_id uuid REFERENCES projects(id) ON DELETE CASCADE,
    enabled boolean NOT NULL DEFAULT true,
    credential_secret text,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT litellm_credential_mappings_scope_check
        CHECK (scope IN ('key', 'project')),
    CONSTRAINT litellm_credential_mappings_target_check
        CHECK (
            (scope = 'key' AND key_id IS NOT NULL AND project_id IS NULL)
            OR (scope = 'project' AND project_id IS NOT NULL AND key_id IS NULL)
        )
);

CREATE UNIQUE INDEX IF NOT EXISTS litellm_credential_mappings_key_idx
    ON litellm_credential_mappings (key_id)
    WHERE scope = 'key';

CREATE UNIQUE INDEX IF NOT EXISTS litellm_credential_mappings_project_idx
    ON litellm_credential_mappings (project_id)
    WHERE scope = 'project';
