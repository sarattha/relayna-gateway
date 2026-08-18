CREATE TABLE IF NOT EXISTS project_memberships (
    member_id uuid NOT NULL REFERENCES portal_members(id) ON DELETE CASCADE,
    project_id uuid NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    role text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (member_id, project_id),
    CONSTRAINT project_memberships_role_check CHECK (role IN ('owner', 'viewer'))
);

CREATE INDEX IF NOT EXISTS project_memberships_project_idx
    ON project_memberships (project_id, member_id);

CREATE TABLE IF NOT EXISTS managed_identity_project_bindings (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id text NOT NULL,
    client_id text NOT NULL,
    object_id text,
    display_name text NOT NULL,
    project_id uuid NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    required_role text NOT NULL DEFAULT 'gateway.monitor.read',
    enabled boolean NOT NULL DEFAULT true,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT managed_identity_project_binding_unique
        UNIQUE (tenant_id, client_id, project_id),
    CONSTRAINT managed_identity_project_binding_nonempty CHECK (
        btrim(tenant_id) <> ''
        AND btrim(client_id) <> ''
        AND btrim(display_name) <> ''
        AND btrim(required_role) <> ''
    )
);

CREATE INDEX IF NOT EXISTS managed_identity_project_bindings_project_enabled_idx
    ON managed_identity_project_bindings (project_id, enabled);
