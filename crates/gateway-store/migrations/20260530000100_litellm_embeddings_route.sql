ALTER TABLE openai_route_settings
    DROP CONSTRAINT IF EXISTS openai_route_settings_route_id_check,
    DROP CONSTRAINT IF EXISTS openai_route_settings_route_check;

ALTER TABLE openai_route_settings
    ADD CONSTRAINT openai_route_settings_route_id_check
        CHECK (route_id IN ('chat-completions', 'responses', 'embeddings')),
    ADD CONSTRAINT openai_route_settings_route_check
        CHECK (route IN ('/v1/chat/completions', '/v1/responses', '/v1/embeddings'));

INSERT INTO openai_route_settings (route_id, route, enabled)
VALUES ('embeddings', '/v1/embeddings', true)
ON CONFLICT (route_id) DO NOTHING;
