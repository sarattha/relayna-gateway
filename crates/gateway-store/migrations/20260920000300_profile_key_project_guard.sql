-- Serialize profile binding writes against key ownership changes. Preserve the
-- previous migration checksum for databases that already applied it.
CREATE OR REPLACE FUNCTION guard_authentication_profile_revision() RETURNS trigger
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
        SELECT project_id INTO caller_project FROM api_keys WHERE id = (binding->>'key_id')::uuid FOR UPDATE;
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

CREATE FUNCTION guard_key_authentication_profile_project() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.project_id IS DISTINCT FROM OLD.project_id AND EXISTS (
        SELECT 1 FROM service_registrations s
        WHERE s.project_id IS NOT NULL
          AND s.project_id IS DISTINCT FROM NEW.project_id
          AND s.access->'authentication_profiles'->'bindings'
              @> jsonb_build_array(jsonb_build_object('key_id', NEW.id::text))
    ) THEN
        RAISE EXCEPTION 'remove service profile assignments before changing key project'
            USING ERRCODE = '23514', CONSTRAINT = 'authentication_profile_key_project';
    END IF;
    RETURN NEW;
END;
$$;
CREATE TRIGGER key_authentication_profile_project
BEFORE UPDATE OF project_id ON api_keys
FOR EACH ROW EXECUTE FUNCTION guard_key_authentication_profile_project();
