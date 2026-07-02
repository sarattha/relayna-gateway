CREATE TABLE IF NOT EXISTS anthropic_route_settings (
    route_id text PRIMARY KEY,
    route text NOT NULL UNIQUE,
    enabled boolean NOT NULL DEFAULT true,
    mode text NOT NULL DEFAULT 'managed_by_gateway',
    updated_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT anthropic_route_settings_route_id_check
        CHECK (route_id IN (
            'messages',
            'messages-count-tokens',
            'message-batches',
            'message-batch',
            'message-batch-results',
            'message-batch-cancel',
            'models'
        )),
    CONSTRAINT anthropic_route_settings_route_check
        CHECK (route IN (
            '/v1/messages',
            '/v1/messages/count_tokens',
            '/v1/messages/batches',
            '/v1/messages/batches/*',
            '/v1/messages/batches/*/results',
            '/v1/messages/batches/*/cancel',
            '/v1/models'
        )),
    CONSTRAINT anthropic_route_settings_mode_check
        CHECK (mode IN ('managed_by_gateway', 'direct_litellm_passthrough'))
);

INSERT INTO anthropic_route_settings (route_id, route, enabled)
VALUES
    ('messages', '/v1/messages', true),
    ('messages-count-tokens', '/v1/messages/count_tokens', true),
    ('message-batches', '/v1/messages/batches', true),
    ('message-batch', '/v1/messages/batches/*', true),
    ('message-batch-results', '/v1/messages/batches/*/results', true),
    ('message-batch-cancel', '/v1/messages/batches/*/cancel', true),
    ('models', '/v1/models', true)
ON CONFLICT (route_id) DO NOTHING;
