ALTER TABLE provider_configs DROP CONSTRAINT provider_configs_provider_check;
ALTER TABLE provider_configs ADD CONSTRAINT provider_configs_provider_check
    CHECK (provider IN ('litellm', 'internal-service', 'azure-foundry'));
ALTER TABLE provider_configs ADD COLUMN foundry jsonb;
ALTER TABLE provider_configs ADD CONSTRAINT provider_configs_foundry_kind_check
    CHECK ((provider = 'azure-foundry') = (foundry IS NOT NULL));
ALTER TABLE service_registrations ADD COLUMN foundry jsonb;
-- An actual foreign key prevents a connection disappearing while services use it.
ALTER TABLE service_registrations ADD COLUMN foundry_provider_id uuid
    GENERATED ALWAYS AS ((foundry->>'provider_id')::uuid) STORED
    REFERENCES provider_configs(id) ON DELETE RESTRICT;
