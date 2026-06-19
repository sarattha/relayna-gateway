ALTER TABLE openai_route_settings
ADD COLUMN IF NOT EXISTS mode text NOT NULL DEFAULT 'managed_by_gateway',
ADD CONSTRAINT openai_route_settings_mode_check
    CHECK (mode IN ('managed_by_gateway', 'direct_litellm_passthrough'));

CREATE TABLE IF NOT EXISTS litellm_passthrough_settings (
    id boolean PRIMARY KEY DEFAULT true,
    enabled boolean NOT NULL DEFAULT false,
    allowed_paths text[] NOT NULL DEFAULT ARRAY['/v1/*']::text[],
    allowed_methods text[] NOT NULL DEFAULT ARRAY['GET', 'POST']::text[],
    ui_exposure text NOT NULL DEFAULT 'disabled',
    admin_api_exposure text NOT NULL DEFAULT 'disabled',
    updated_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT litellm_passthrough_settings_singleton_check CHECK (id),
    CONSTRAINT litellm_passthrough_settings_paths_check CHECK (cardinality(allowed_paths) > 0),
    CONSTRAINT litellm_passthrough_settings_methods_check CHECK (cardinality(allowed_methods) > 0),
    CONSTRAINT litellm_passthrough_settings_ui_exposure_check
        CHECK (ui_exposure IN ('disabled', 'operator_only', 'explicitly_exposed', 'trusted_ingress')),
    CONSTRAINT litellm_passthrough_settings_admin_api_exposure_check
        CHECK (admin_api_exposure IN ('disabled', 'operator_only', 'explicitly_exposed'))
);

INSERT INTO litellm_passthrough_settings (id)
VALUES (true)
ON CONFLICT (id) DO NOTHING;
