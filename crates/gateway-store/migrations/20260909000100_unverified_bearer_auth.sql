-- Existing rows retain the released authentication behavior.
ALTER TABLE gateway_auth_settings
    ADD COLUMN unverified_bearer_enabled boolean NOT NULL DEFAULT false,
    ADD CONSTRAINT gateway_auth_settings_unverified_bearer_no_apigee
        CHECK (NOT (unverified_bearer_enabled AND apigee_trusted_header_enabled));
