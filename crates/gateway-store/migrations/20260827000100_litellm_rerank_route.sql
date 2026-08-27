ALTER TABLE openai_route_settings
    DROP CONSTRAINT IF EXISTS openai_route_settings_route_id_check,
    DROP CONSTRAINT IF EXISTS openai_route_settings_route_check;

ALTER TABLE openai_route_settings
    ADD CONSTRAINT openai_route_settings_route_id_check
        CHECK (route_id IN ('chat-completions', 'responses', 'embeddings', 'rerank')),
    ADD CONSTRAINT openai_route_settings_route_check
        CHECK (route IN ('/v1/chat/completions', '/v1/responses', '/v1/embeddings', '/v1/rerank'));

INSERT INTO openai_route_settings (route_id, route, enabled)
VALUES ('rerank', '/v1/rerank', true)
ON CONFLICT (route_id) DO NOTHING;
