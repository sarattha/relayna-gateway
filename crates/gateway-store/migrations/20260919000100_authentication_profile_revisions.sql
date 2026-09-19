-- Route-local profiles live in the existing strict EndpointAccess JSON. Released
-- readers reject the new field instead of silently skipping authentication.
-- Guard writes in PostgreSQL too: old binaries must not erase opted-in policy.
CREATE FUNCTION guard_authentication_profile_revision() RETURNS trigger
LANGUAGE plpgsql AS $$
DECLARE
    previous jsonb;
    incoming jsonb;
    expected bigint;
    binding jsonb;
    caller_project uuid;
BEGIN
    incoming := NEW.access->'authentication_profiles';
    IF TG_OP = 'UPDATE' THEN
        previous := OLD.access->'authentication_profiles';
    END IF;
    IF incoming IS NULL AND previous IS NULL THEN
        RETURN NEW;
    END IF;
    expected := COALESCE((previous->>'revision')::bigint, 0);
    IF incoming IS NULL OR (incoming->>'revision')::bigint IS DISTINCT FROM expected THEN
        RAISE EXCEPTION 'authentication profile revision conflict' USING ERRCODE = '40001';
    END IF;
    FOR binding IN SELECT value FROM jsonb_array_elements(incoming->'bindings') LOOP
        SELECT project_id INTO caller_project FROM api_keys WHERE id = (binding->>'key_id')::uuid FOR KEY SHARE;
        IF NOT FOUND THEN
            RAISE EXCEPTION 'invalid authentication profile binding' USING ERRCODE = '22023';
        END IF;
        IF TG_TABLE_NAME = 'service_registrations' THEN
            IF NEW.project_id IS NOT NULL AND caller_project IS DISTINCT FROM NEW.project_id THEN
                RAISE EXCEPTION 'invalid authentication profile binding' USING ERRCODE = '22023';
            END IF;
        END IF;
    END LOOP;
    -- Unrelated service writes can retain an identical policy without changing
    -- revision; actual edits must present the last observed revision.
    IF TG_OP = 'UPDATE' AND NEW.access = OLD.access THEN
        RETURN NEW;
    END IF;
    NEW.access := jsonb_set(NEW.access, '{authentication_profiles,revision}', to_jsonb(expected + 1));
    RETURN NEW;
END;
$$;
CREATE TRIGGER route_authentication_profile_revision
BEFORE INSERT OR UPDATE ON route_identity_settings
FOR EACH ROW EXECUTE FUNCTION guard_authentication_profile_revision();
CREATE TRIGGER service_authentication_profile_revision
BEFORE INSERT OR UPDATE ON service_registrations
FOR EACH ROW EXECUTE FUNCTION guard_authentication_profile_revision();
