ALTER TABLE openai_route_settings
ADD COLUMN IF NOT EXISTS timeout_ms bigint NOT NULL DEFAULT 120000,
ADD COLUMN IF NOT EXISTS max_request_body_bytes bigint NOT NULL DEFAULT 1048576,
ADD COLUMN IF NOT EXISTS max_response_body_bytes bigint NOT NULL DEFAULT 1048576;

ALTER TABLE openai_route_settings
DROP CONSTRAINT IF EXISTS openai_route_settings_runtime_limits_check;

ALTER TABLE openai_route_settings
ADD CONSTRAINT openai_route_settings_runtime_limits_check
    CHECK (
        timeout_ms BETWEEN 1 AND 600000
        AND max_request_body_bytes BETWEEN 1 AND 104857600
        AND max_response_body_bytes BETWEEN 1 AND 104857600
    );

ALTER TABLE anthropic_route_settings
ADD COLUMN IF NOT EXISTS timeout_ms bigint NOT NULL DEFAULT 120000,
ADD COLUMN IF NOT EXISTS max_request_body_bytes bigint NOT NULL DEFAULT 1048576,
ADD COLUMN IF NOT EXISTS max_response_body_bytes bigint NOT NULL DEFAULT 1048576;

ALTER TABLE anthropic_route_settings
DROP CONSTRAINT IF EXISTS anthropic_route_settings_runtime_limits_check;

ALTER TABLE anthropic_route_settings
ADD CONSTRAINT anthropic_route_settings_runtime_limits_check
    CHECK (
        timeout_ms BETWEEN 1 AND 600000
        AND max_request_body_bytes BETWEEN 1 AND 104857600
        AND max_response_body_bytes BETWEEN 1 AND 104857600
    );

ALTER TABLE litellm_passthrough_settings
ADD COLUMN IF NOT EXISTS timeout_ms bigint NOT NULL DEFAULT 120000,
ADD COLUMN IF NOT EXISTS max_request_body_bytes bigint NOT NULL DEFAULT 1048576,
ADD COLUMN IF NOT EXISTS max_response_body_bytes bigint NOT NULL DEFAULT 1048576;

ALTER TABLE litellm_passthrough_settings
DROP CONSTRAINT IF EXISTS litellm_passthrough_settings_runtime_limits_check;

ALTER TABLE litellm_passthrough_settings
ADD CONSTRAINT litellm_passthrough_settings_runtime_limits_check
    CHECK (
        timeout_ms BETWEEN 1 AND 600000
        AND max_request_body_bytes BETWEEN 1 AND 104857600
        AND max_response_body_bytes BETWEEN 1 AND 104857600
    );
