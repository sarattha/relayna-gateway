BEGIN;

INSERT INTO projects (id, name)
VALUES
    ('10000000-0000-0000-0000-000000000001', 'Analytics Platform'),
    ('10000000-0000-0000-0000-000000000002', 'Order Operations')
ON CONFLICT (id) DO UPDATE SET name = EXCLUDED.name, updated_at = now();

INSERT INTO service_registrations (
    name, route_pattern, upstream_base_url, enabled, allowed_methods, cost_mode,
    estimated_cost_usd, source, sync_status, project_id, health_check_path,
    pricing_rules, endpoint_pricing_rules
)
VALUES
    (
        'analytics-api', '/internal/analytics/*', 'http://mock-upstream:4000', true,
        ARRAY['GET', 'POST'], 'fixed', 0.0025, 'gateway', 'local',
        '10000000-0000-0000-0000-000000000001', '/health',
        '[{"name":"analytics-default","cost_usd":0.0025}]'::jsonb,
        '[{"name":"query-report","method":"POST","path":"/v1/reports/query","cost_usd":0.0035}]'::jsonb
    ),
    (
        'document-search', '/internal/search/*', 'http://mock-upstream:4000', true,
        ARRAY['GET', 'POST'], 'fixed', 0.0015, 'gateway', 'local',
        '10000000-0000-0000-0000-000000000001', '/health',
        '[{"name":"search-default","cost_usd":0.0015}]'::jsonb,
        '[]'::jsonb
    ),
    (
        'orders-api', '/internal/orders/*', 'http://mock-upstream:4000', true,
        ARRAY['GET', 'POST'], 'fixed', 0.0010, 'gateway', 'local',
        '10000000-0000-0000-0000-000000000002', '/health',
        '[]'::jsonb, '[]'::jsonb
    )
ON CONFLICT (name) DO UPDATE SET
    upstream_base_url = EXCLUDED.upstream_base_url,
    enabled = EXCLUDED.enabled,
    project_id = EXCLUDED.project_id,
    pricing_rules = EXCLUDED.pricing_rules,
    endpoint_pricing_rules = EXCLUDED.endpoint_pricing_rules,
    updated_at = now();

INSERT INTO project_service_links (project_id, service_name)
VALUES
    ('10000000-0000-0000-0000-000000000001', 'analytics-api'),
    ('10000000-0000-0000-0000-000000000001', 'document-search'),
    ('10000000-0000-0000-0000-000000000002', 'orders-api')
ON CONFLICT DO NOTHING;

INSERT INTO api_keys (id, project_id, key_prefix, key_hash, owner_type, disabled)
VALUES
    ('20000000-0000-0000-0000-000000000001', '10000000-0000-0000-0000-000000000001', 'demo_analytics_key', 'local-inspection-not-for-authentication', 'project', false),
    ('20000000-0000-0000-0000-000000000002', '10000000-0000-0000-0000-000000000002', 'demo_orders_key', 'local-inspection-not-for-authentication', 'project', false)
ON CONFLICT (id) DO UPDATE
SET key_prefix = EXCLUDED.key_prefix,
    disabled = false,
    updated_at = now();

INSERT INTO key_service_links (key_id, service_name)
VALUES
    ('20000000-0000-0000-0000-000000000001', 'analytics-api'),
    ('20000000-0000-0000-0000-000000000001', 'document-search'),
    ('20000000-0000-0000-0000-000000000002', 'orders-api')
ON CONFLICT DO NOTHING;

INSERT INTO portal_members (id, tenant_id, object_id, email, display_name, status, roles)
VALUES
    ('30000000-0000-0000-0000-000000000001', '00000000-0000-0000-0000-000000000001', '00000000-0000-0000-0000-000000000002', 'gateway.admin@relayna.dev', 'Gateway Administrator', 'active', ARRAY['admin']),
    ('30000000-0000-0000-0000-000000000002', '00000000-0000-0000-0000-000000000001', '00000000-0000-0000-0000-000000000005', 'analytics.owner@relayna.dev', 'Analytics Project Owner', 'active', ARRAY[]::text[]),
    ('30000000-0000-0000-0000-000000000003', '00000000-0000-0000-0000-000000000001', '00000000-0000-0000-0000-000000000004', 'orders.owner@relayna.dev', 'Orders Service Owner', 'active', ARRAY[]::text[])
ON CONFLICT (tenant_id, object_id) DO UPDATE SET
    email = EXCLUDED.email,
    display_name = EXCLUDED.display_name,
    status = EXCLUDED.status,
    roles = EXCLUDED.roles,
    updated_at = now();

INSERT INTO project_memberships (member_id, project_id, role)
VALUES ('30000000-0000-0000-0000-000000000002', '10000000-0000-0000-0000-000000000001', 'owner')
ON CONFLICT (member_id, project_id) DO UPDATE SET role = EXCLUDED.role, updated_at = now();

INSERT INTO service_memberships (member_id, service_name, role)
VALUES ('30000000-0000-0000-0000-000000000003', 'orders-api', 'owner')
ON CONFLICT (member_id, service_name) DO UPDATE SET role = EXCLUDED.role, updated_at = now();

INSERT INTO managed_identity_project_bindings (
    id, tenant_id, client_id, object_id, display_name, project_id, required_role, enabled
)
VALUES (
    '40000000-0000-0000-0000-000000000001',
    '00000000-0000-0000-0000-000000000001',
    '00000000-0000-0000-0000-000000000201',
    '00000000-0000-0000-0000-000000000202',
    'Local monitoring managed identity',
    '10000000-0000-0000-0000-000000000001',
    'gateway.monitor.read',
    true
)
ON CONFLICT (tenant_id, client_id, project_id) DO UPDATE SET
    object_id = EXCLUDED.object_id,
    display_name = EXCLUDED.display_name,
    required_role = EXCLUDED.required_role,
    enabled = true,
    updated_at = now();

INSERT INTO managed_identity_bindings (
    id, tenant_id, client_id, object_id, display_name, service_name, required_role, enabled
)
VALUES (
    '40000000-0000-0000-0000-000000000002',
    '00000000-0000-0000-0000-000000000001',
    '00000000-0000-0000-0000-000000000201',
    '00000000-0000-0000-0000-000000000202',
    'Local monitoring managed identity',
    'analytics-api',
    'gateway.monitor.read',
    true
)
ON CONFLICT (tenant_id, client_id, service_name) DO UPDATE SET
    object_id = EXCLUDED.object_id,
    display_name = EXCLUDED.display_name,
    required_role = EXCLUDED.required_role,
    enabled = true,
    updated_at = now();

INSERT INTO usage_events (
    id, request_id, key_id, project_id, route, model, provider, status,
    status_code, latency_ms, input_tokens, output_tokens, total_tokens,
    estimated_cost, cost_source, cost_mode, pricing_rule_name, service_name,
    service_version, http_method, endpoint_path, endpoint_template, task_id,
    run_id, trace_id, fallback_count, created_at
)
SELECT
    (substr(md5('analytics-usage-' || sample::text), 1, 8) || '-' ||
     substr(md5('analytics-usage-' || sample::text), 9, 4) || '-' ||
     substr(md5('analytics-usage-' || sample::text), 13, 4) || '-' ||
     substr(md5('analytics-usage-' || sample::text), 17, 4) || '-' ||
     substr(md5('analytics-usage-' || sample::text), 21, 12))::uuid,
    'demo-analytics-' || lpad(sample::text, 3, '0'),
    '20000000-0000-0000-0000-000000000001',
    '10000000-0000-0000-0000-000000000001',
    CASE WHEN sample % 3 = 0 THEN '/internal/search/query' ELSE '/internal/analytics/report' END,
    CASE WHEN sample % 4 = 0 THEN 'gpt-4.1' ELSE 'gpt-4.1-mini' END,
    CASE WHEN sample % 5 = 0 THEN 'openai-direct' ELSE 'litellm' END,
    CASE WHEN sample % 11 = 0 THEN 'failure' ELSE 'success' END,
    CASE WHEN sample % 11 = 0 THEN CASE WHEN sample % 2 = 0 THEN 429 ELSE 500 END ELSE 200 END,
    145 + ((sample * 37) % 1100),
    90 + ((sample * 29) % 850),
    24 + ((sample * 17) % 240),
    114 + ((sample * 46) % 1090),
    round((0.0008 + ((sample % 13) * 0.00037))::numeric, 8),
    'service_pricing', 'fixed',
    CASE WHEN sample % 3 = 0 THEN 'search-default' ELSE 'analytics-default' END,
    CASE WHEN sample % 3 = 0 THEN 'document-search' ELSE 'analytics-api' END,
    CASE WHEN sample < 48 THEN '2026.08.2' ELSE '2026.08.1' END,
    'POST',
    CASE WHEN sample % 3 = 0 THEN '/v1/search' ELSE '/v1/reports/query' END,
    CASE WHEN sample % 3 = 0 THEN '/v1/search' ELSE '/v1/reports/query' END,
    'task-' || (sample % 8),
    'run-' || (sample % 24),
    'trace-demo-' || lpad(sample::text, 3, '0'),
    CASE WHEN sample % 17 = 0 THEN 1 ELSE 0 END,
    now() - make_interval(hours => sample)
FROM generate_series(0, 167) AS samples(sample)
ON CONFLICT (id) DO UPDATE SET created_at = EXCLUDED.created_at;

INSERT INTO request_debug_bundles (
    request_id, project_id, route, provider, service_name, policy_trace,
    guardrail_trace, selection_trace, fallback_history, upstream_latency_ms,
    request_hash, response_hash, redaction_version, trace_id, created_at
)
SELECT
    'demo-analytics-' || lpad(sample::text, 3, '0'),
    '10000000-0000-0000-0000-000000000001',
    '/services/*', 'litellm', 'analytics-api',
    '["project-route-policy:allow"]'::jsonb,
    '["pii-redact:modify"]'::jsonb,
    '["litellm:selected:lowest-latency-healthy"]'::jsonb,
    '[]'::jsonb,
    130 + sample, 'sha256:local-request-redacted', 'sha256:local-response-redacted',
    1, 'trace-demo-' || lpad(sample::text, 3, '0'), now() - make_interval(hours => sample)
FROM generate_series(0, 156, 12) AS samples(sample)
ON CONFLICT (request_id) DO UPDATE SET
    project_id = EXCLUDED.project_id,
    route = EXCLUDED.route,
    provider = EXCLUDED.provider,
    service_name = EXCLUDED.service_name,
    policy_trace = EXCLUDED.policy_trace,
    guardrail_trace = EXCLUDED.guardrail_trace,
    selection_trace = EXCLUDED.selection_trace,
    fallback_history = EXCLUDED.fallback_history,
    upstream_latency_ms = EXCLUDED.upstream_latency_ms,
    request_hash = EXCLUDED.request_hash,
    response_hash = EXCLUDED.response_hash,
    redaction_version = EXCLUDED.redaction_version,
    trace_id = EXCLUDED.trace_id,
    created_at = EXCLUDED.created_at;

INSERT INTO guardrail_execution_events (
    id, request_id, key_id, project_id, route, model, provider, guardrail_name,
    mode, action, failure_policy, latency_ms, reason, metadata, created_at
)
SELECT
    (substr(md5('analytics-guardrail-' || sample::text), 1, 8) || '-' ||
     substr(md5('analytics-guardrail-' || sample::text), 9, 4) || '-' ||
     substr(md5('analytics-guardrail-' || sample::text), 13, 4) || '-' ||
     substr(md5('analytics-guardrail-' || sample::text), 17, 4) || '-' ||
     substr(md5('analytics-guardrail-' || sample::text), 21, 12))::uuid,
    'demo-analytics-' || lpad(sample::text, 3, '0'),
    '20000000-0000-0000-0000-000000000001',
    '10000000-0000-0000-0000-000000000001',
    '/services/*', 'gpt-4.1-mini', 'litellm', 'pii-redact',
    'pre_call', CASE WHEN sample % 18 = 0 THEN 'modify' ELSE 'allow' END,
    'fail_closed', 4 + (sample % 9),
    CASE WHEN sample % 18 = 0 THEN 'Synthetic email address redacted.' ELSE NULL END,
    '{"fixture":"local-project-owner"}'::jsonb,
    now() - make_interval(hours => sample)
FROM generate_series(0, 162, 9) AS samples(sample)
ON CONFLICT (id) DO UPDATE SET created_at = EXCLUDED.created_at;

COMMIT;

SELECT 'Relayna local project-owner demo data is ready.' AS status;
