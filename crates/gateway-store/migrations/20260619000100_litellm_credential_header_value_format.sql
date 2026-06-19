ALTER TABLE provider_configs
ADD COLUMN IF NOT EXISTS credential_header_value_format text NOT NULL DEFAULT 'raw';

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM pg_constraint
        WHERE conname = 'provider_configs_credential_header_value_format_check'
    ) THEN
        ALTER TABLE provider_configs
        ADD CONSTRAINT provider_configs_credential_header_value_format_check
            CHECK (credential_header_value_format IN ('raw', 'bearer'));
    END IF;
END $$;
