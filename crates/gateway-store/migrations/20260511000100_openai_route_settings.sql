CREATE TABLE IF NOT EXISTS openai_route_settings (
    route_id text PRIMARY KEY,
    route text NOT NULL UNIQUE,
    enabled boolean NOT NULL DEFAULT true,
    updated_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT openai_route_settings_route_id_check
        CHECK (route_id IN ('chat-completions', 'responses')),
    CONSTRAINT openai_route_settings_route_check
        CHECK (route IN ('/v1/chat/completions', '/v1/responses'))
);

INSERT INTO openai_route_settings (route_id, route, enabled)
VALUES
    ('chat-completions', '/v1/chat/completions', true),
    ('responses', '/v1/responses', true)
ON CONFLICT (route_id) DO NOTHING;
