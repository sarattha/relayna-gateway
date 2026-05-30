CREATE TABLE IF NOT EXISTS gateway_auth_settings (
    singleton boolean PRIMARY KEY DEFAULT true,
    entra_enabled boolean NOT NULL DEFAULT false,
    tenant_id text,
    audience text,
    issuer text,
    oidc_discovery_url text,
    required_scope text,
    required_role text,
    allowed_groups text[] NOT NULL DEFAULT ARRAY[]::text[],
    accepted_algorithms text[] NOT NULL DEFAULT ARRAY['RS256']::text[],
    relayna_key_header text,
    jwks_cache_ttl_seconds bigint,
    clock_skew_seconds bigint,
    apigee_trusted_header_enabled boolean NOT NULL DEFAULT false,
    apigee_trusted_header_secret text,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT gateway_auth_settings_singleton CHECK (singleton),
    CONSTRAINT gateway_auth_settings_oidc_discovery_url_format
        CHECK (oidc_discovery_url IS NULL OR oidc_discovery_url ~* '^https?://')
);
