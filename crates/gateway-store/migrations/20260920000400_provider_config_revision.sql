-- Cache invalidation must follow row-update order, not transaction start time.
ALTER TABLE provider_configs
    ADD COLUMN config_revision bigint NOT NULL DEFAULT 1 CHECK (config_revision > 0);

CREATE FUNCTION advance_provider_config_revision() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    NEW.config_revision := OLD.config_revision + 1;
    RETURN NEW;
END;
$$;

CREATE TRIGGER provider_config_revision
BEFORE UPDATE ON provider_configs
FOR EACH ROW EXECUTE FUNCTION advance_provider_config_revision();
