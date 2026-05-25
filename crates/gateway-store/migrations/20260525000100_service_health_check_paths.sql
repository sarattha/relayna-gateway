ALTER TABLE service_registrations
ADD COLUMN IF NOT EXISTS health_check_path text,
ADD COLUMN IF NOT EXISTS health_check_method text NOT NULL DEFAULT 'GET';

ALTER TABLE service_registrations
ADD CONSTRAINT service_registrations_health_check_path_check
CHECK (health_check_path IS NULL OR (health_check_path LIKE '/%' AND health_check_path NOT LIKE '%//%'));

ALTER TABLE service_registrations
ADD CONSTRAINT service_registrations_health_check_method_check
CHECK (health_check_method IN ('GET', 'HEAD'));
