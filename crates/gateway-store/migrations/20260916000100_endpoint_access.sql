ALTER TABLE service_registrations ADD COLUMN access JSONB NOT NULL DEFAULT '{}'::jsonb;
