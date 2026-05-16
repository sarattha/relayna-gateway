ALTER TABLE key_guardrail_policies
    ADD COLUMN IF NOT EXISTS guardrail_config_overrides jsonb NOT NULL DEFAULT '{}'::jsonb;
