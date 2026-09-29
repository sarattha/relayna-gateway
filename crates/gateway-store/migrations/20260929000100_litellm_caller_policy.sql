-- Defaults preserve released authentication and path eligibility.
ALTER TABLE litellm_passthrough_settings
    ADD COLUMN authentication_mode TEXT NOT NULL DEFAULT 'gateway'
        CHECK (authentication_mode IN ('gateway', 'litellm_bearer')),
    ADD COLUMN blocked_paths TEXT[] NOT NULL DEFAULT '{}';
