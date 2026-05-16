CREATE TABLE IF NOT EXISTS studio_connection_settings (
    singleton boolean PRIMARY KEY DEFAULT true,
    base_url text,
    bearer_token_secret text,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT studio_connection_settings_singleton CHECK (singleton),
    CONSTRAINT studio_connection_settings_base_url_format
        CHECK (base_url IS NULL OR base_url ~ '^https?://')
);
