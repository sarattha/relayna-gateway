CREATE TABLE IF NOT EXISTS portal_members (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id text NOT NULL,
    object_id text NOT NULL,
    email text,
    display_name text,
    status text NOT NULL DEFAULT 'pending',
    roles text[] NOT NULL DEFAULT ARRAY[]::text[],
    last_sign_in_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT portal_members_identity_unique UNIQUE (tenant_id, object_id),
    CONSTRAINT portal_members_status_check CHECK (status IN ('pending', 'active', 'blocked')),
    CONSTRAINT portal_members_identity_nonempty CHECK (
        btrim(tenant_id) <> '' AND btrim(object_id) <> ''
    )
);

CREATE INDEX IF NOT EXISTS portal_members_status_created_at_idx
    ON portal_members (status, created_at DESC);

CREATE TABLE IF NOT EXISTS service_memberships (
    member_id uuid NOT NULL REFERENCES portal_members(id) ON DELETE CASCADE,
    service_name text NOT NULL REFERENCES service_registrations(name) ON DELETE CASCADE,
    role text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (member_id, service_name),
    CONSTRAINT service_memberships_role_check CHECK (role IN ('owner', 'viewer'))
);

CREATE INDEX IF NOT EXISTS service_memberships_service_idx
    ON service_memberships (service_name, member_id);

CREATE TABLE IF NOT EXISTS managed_identity_bindings (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id text NOT NULL,
    client_id text NOT NULL,
    object_id text,
    display_name text NOT NULL,
    service_name text NOT NULL REFERENCES service_registrations(name) ON DELETE CASCADE,
    required_role text NOT NULL DEFAULT 'gateway.monitor.read',
    enabled boolean NOT NULL DEFAULT true,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT managed_identity_binding_unique UNIQUE (tenant_id, client_id, service_name),
    CONSTRAINT managed_identity_binding_nonempty CHECK (
        btrim(tenant_id) <> ''
        AND btrim(client_id) <> ''
        AND btrim(display_name) <> ''
        AND btrim(required_role) <> ''
    )
);

CREATE INDEX IF NOT EXISTS managed_identity_bindings_service_enabled_idx
    ON managed_identity_bindings (service_name, enabled);

CREATE TABLE IF NOT EXISTS oidc_login_transactions (
    state_hash text PRIMARY KEY,
    binding_hash text NOT NULL,
    nonce text NOT NULL,
    pkce_verifier text NOT NULL,
    return_to text NOT NULL DEFAULT '/admin-ui',
    expires_at timestamptz NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT oidc_login_transaction_nonempty CHECK (
        btrim(state_hash) <> ''
        AND btrim(binding_hash) <> ''
        AND btrim(nonce) <> ''
        AND btrim(pkce_verifier) <> ''
    ),
    CONSTRAINT oidc_login_return_to_check CHECK (return_to LIKE '/admin-ui%')
);

CREATE INDEX IF NOT EXISTS oidc_login_transactions_expires_idx
    ON oidc_login_transactions (expires_at);

CREATE TABLE IF NOT EXISTS portal_sessions (
    session_hash text PRIMARY KEY,
    member_id uuid NOT NULL REFERENCES portal_members(id) ON DELETE CASCADE,
    csrf_hash text NOT NULL,
    expires_at timestamptz NOT NULL,
    last_seen_at timestamptz NOT NULL DEFAULT now(),
    created_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT portal_sessions_hash_nonempty CHECK (
        btrim(session_hash) <> '' AND btrim(csrf_hash) <> ''
    )
);

CREATE INDEX IF NOT EXISTS portal_sessions_member_expires_idx
    ON portal_sessions (member_id, expires_at DESC);

ALTER TABLE audit_events
    ALTER COLUMN actor_token_id DROP NOT NULL;

ALTER TABLE audit_events
    ADD COLUMN IF NOT EXISTS actor_member_id uuid REFERENCES portal_members(id);

ALTER TABLE audit_events
    DROP CONSTRAINT IF EXISTS audit_events_actor_check;

ALTER TABLE audit_events
    ADD CONSTRAINT audit_events_actor_check CHECK (
        (actor_token_id IS NOT NULL)::integer + (actor_member_id IS NOT NULL)::integer = 1
    );

CREATE INDEX IF NOT EXISTS audit_events_member_created_at_idx
    ON audit_events (actor_member_id, created_at DESC)
    WHERE actor_member_id IS NOT NULL;
