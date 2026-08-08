use axum::{
    body::{to_bytes, Body},
    http::{Method, Request, StatusCode},
    response::Html,
    routing::get,
    Json, Router,
};
use gateway_api::app;
use gateway_core::{OperatorTokenMaterial, OperatorTokenStore};
use gateway_store::{PostgresStore, RedisReadiness};
use serde_json::{json, Value};
use tower::ServiceExt;
use uuid::Uuid;

const REDIS_TEST_URL: &str = "redis://127.0.0.1:26380";

async fn integration_store() -> Option<PostgresStore> {
    let database_url = match std::env::var("DATABASE_URL") {
        Ok(value) => value,
        Err(_) => {
            eprintln!(
                "skipping PostgreSQL Admin API integration coverage: DATABASE_URL is not set"
            );
            return None;
        }
    };
    Some(
        PostgresStore::connect(&database_url)
            .await
            .expect("connect PostgreSQL test store"),
    )
}

async fn install_known_operator_token(store: &PostgresStore) -> String {
    let material = OperatorTokenMaterial::generate().expect("operator material");
    let active_id = sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM operator_tokens WHERE disabled = false AND revoked_at IS NULL LIMIT 1",
    )
    .fetch_optional(store.pool())
    .await
    .expect("query active operator token");
    if let Some(id) = active_id {
        sqlx::query(
            "UPDATE operator_tokens SET token_prefix = $2, token_hash = $3, roles = ARRAY['owner']::text[], scopes = ARRAY['*']::text[] WHERE id = $1",
        )
        .bind(id)
        .bind(&material.token_prefix)
        .bind(&material.token_hash)
        .execute(store.pool())
        .await
        .expect("replace test operator token");
    } else {
        store
            .bootstrap_operator_token(&material)
            .await
            .expect("bootstrap test operator token")
            .expect("operator token created");
    }
    material.raw_token
}

async fn mock_control_upstream(suffix: &str) -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind control-plane mock");
    let address = listener.local_addr().expect("control-plane mock address");
    let studio_name = format!("studio-coverage-{suffix}");
    let app = Router::new()
        .route(
            "/openapi.json",
            get(|| async {
                Json(json!({
                    "openapi": "3.0.3",
                    "info": {"title": "Coverage Service", "version": "1.0.0"},
                    "paths": {
                        "/run": {"post": {"operationId": "run"}},
                        "/feed": {"get": {"operationId": "relaynaFeed"}},
                        "/status": {"get": {"operationId": "relaynaStatus"}}
                    }
                }))
            }),
        )
        .route(
            "/studio/gateway/services",
            get(move || {
                let studio_name = studio_name.clone();
                async move {
                    Json(json!({"services": [{
                        "studio_service_id": format!("studio-{studio_name}"),
                        "name": studio_name,
                        "route_pattern": format!("/services/{studio_name}/*"),
                        "upstream_base_url": "http://studio-service.internal:8080",
                        "allowed_methods": ["GET", "POST"]
                    }]}))
                }
            }),
        )
        .route("/health", get(|| async { StatusCode::NO_CONTENT }))
        .fallback(|| async {
            Html(
                r#"<html><script src="/ui/app.js"></script><a href="/litellm/models">models</a></html>"#,
            )
        });
    let task = tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("serve control-plane mock");
    });
    (format!("http://{address}"), task)
}

async fn call(
    app: Router,
    method: Method,
    path: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header("x-request-id", format!("coverage-{}", Uuid::new_v4()));
    if let Some(token) = token {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    let request_body = if let Some(body) = body {
        builder = builder.header("content-type", "application/json");
        Body::from(serde_json::to_vec(&body).expect("serialize request body"))
    } else {
        Body::empty()
    };
    let response = app
        .oneshot(builder.body(request_body).expect("build request"))
        .await
        .expect("router response");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 8 * 1024 * 1024)
        .await
        .expect("read response body");
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into_owned()))
    };
    (status, value)
}

fn expect_success(status: StatusCode, value: &Value, operation: &str) {
    assert!(
        status.is_success(),
        "{operation} failed with {status}: {value}"
    );
}

#[tokio::test]
async fn postgres_admin_api_workflow_covers_registered_control_plane() {
    let Some(store) = integration_store().await else {
        return;
    };
    let mut database_lock = store
        .pool()
        .acquire()
        .await
        .expect("acquire integration lock");
    sqlx::query("SELECT pg_advisory_lock(82120260808)")
        .execute(&mut *database_lock)
        .await
        .expect("serialize shared control-plane integration state");
    let redis_url = std::env::var("REDIS_URL").unwrap_or_else(|_| REDIS_TEST_URL.to_owned());
    let token = install_known_operator_token(&store).await;
    let suffix = Uuid::new_v4().simple().to_string();
    let (mock_url, mock_task) = mock_control_upstream(&suffix).await;
    sqlx::query("UPDATE provider_configs SET enabled = false WHERE provider = 'litellm'")
        .execute(store.pool())
        .await
        .expect("disable pre-existing LiteLLM providers");
    sqlx::query(
        r#"
        INSERT INTO provider_configs (
            provider, name, base_url, credential_secret, credential_header_mode,
            credential_header_name, credential_header_value_format, enabled
        ) VALUES (
            'litellm', 'integration-litellm', $1, 'litellm-ui-coverage',
            'authorization_bearer', NULL, 'raw', true
        )
        ON CONFLICT (provider, name) DO UPDATE SET
            base_url = EXCLUDED.base_url,
            credential_secret = EXCLUDED.credential_secret,
            credential_header_mode = EXCLUDED.credential_header_mode,
            credential_header_name = EXCLUDED.credential_header_name,
            credential_header_value_format = EXCLUDED.credential_header_value_format,
            enabled = true,
            updated_at = now()
        "#,
    )
    .bind(&mock_url)
    .execute(store.pool())
    .await
    .expect("point LiteLLM UI proxy at mock");
    let app = app::router_with_studio(
        store,
        RedisReadiness::new(&redis_url).expect("Redis readiness client"),
        Some(app::StudioCatalogClient::new(
            &mock_url,
            Some("studio-coverage-token".to_owned()),
        )),
    );

    let (status, _) = call(app.clone(), Method::GET, "/admin-ui/healthz", None, None).await;
    assert_eq!(status, StatusCode::OK);
    let (status, value) = call(app.clone(), Method::GET, "/admin-ui/readyz", None, None).await;
    expect_success(status, &value, "readiness");
    for path in [
        "/admin-ui/litellm-ui",
        "/admin-ui/litellm-ui/",
        "/admin-ui/litellm-ui/litellm/.well-known/litellm-ui-config",
        "/admin-ui/litellm-ui/litellm/models",
        "/admin-ui/litellm-ui/assets/app.js",
        "/litellm-asset-prefix/app.js",
        "/litellm/.well-known/litellm-ui-config",
        "/v2/model/info",
    ] {
        let (status, _) = call(app.clone(), Method::GET, path, Some(&token), None).await;
        expect_success(status, &Value::Null, path);
    }

    let (status, project) = call(
        app.clone(),
        Method::POST,
        "/admin-ui/admin/projects",
        Some(&token),
        Some(json!({"name": format!("api-coverage-{suffix}")})),
    )
    .await;
    expect_success(status, &project, "create project");
    let project_id = project["id"].as_str().expect("project id");

    let service_name = format!("api-coverage-{suffix}");
    let (status, service) = call(
        app.clone(),
        Method::POST,
        "/admin-ui/admin/services",
        Some(&token),
        Some(json!({
            "name": service_name,
            "project_id": project_id,
            "route_pattern": format!("/services/{service_name}/*"),
            "upstream_base_url": mock_url,
            "health_check_path": "/health",
            "allowed_methods": ["GET", "POST"],
            "credential": "coverage-secret",
            "cost_mode": "fixed",
            "estimated_cost_usd": 0.1,
            "openapi_source_path": "/openapi.json"
        })),
    )
    .await;
    expect_success(status, &service, "create service");

    let (status, key) = call(
        app.clone(),
        Method::POST,
        "/admin-ui/admin/keys",
        Some(&token),
        Some(json!({
            "owner_type": "project",
            "project_id": project_id,
            "service_names": [service_name],
            "expires_at": null,
            "preset": "developer",
            "policy": {
                "allowed_routes": ["/services/*"],
                "allowed_providers": ["internal-service"],
                "allowed_services": [service_name]
            },
            "guardrail_policy": {"optional_guardrails": ["pii-redact"]}
        })),
    )
    .await;
    expect_success(status, &key, "create key");
    let key_id = key["key"]["id"].as_str().expect("key id");
    let raw_key = key["raw_key"].as_str().expect("raw key");

    let (status, value) = call(
        app.clone(),
        Method::PATCH,
        "/admin-ui/admin/studio/connection",
        Some(&token),
        Some(json!({"base_url": mock_url, "token": "studio-coverage-token"})),
    )
    .await;
    expect_success(status, &value, "point Studio connection at mock");

    let missing_id = Uuid::new_v4();
    for (method, path, body) in [
        (
            Method::GET,
            "/admin-ui/admin/studio/services".to_owned(),
            None,
        ),
        (
            Method::POST,
            "/admin-ui/admin/studio/connection/test".to_owned(),
            Some(json!({})),
        ),
        (
            Method::POST,
            "/admin-ui/admin/services/import/preview".to_owned(),
            Some(json!({"services": [{
                "studio_service_id": format!("manual-{suffix}"),
                "name": format!("preview-{suffix}"),
                "route_pattern": format!("/services/preview-{suffix}/*"),
                "upstream_base_url": mock_url,
                "allowed_methods": ["POST"]
            }]})),
        ),
        (
            Method::POST,
            "/admin-ui/admin/services/import".to_owned(),
            Some(json!({
                "studio_service_id": format!("import-{suffix}"),
                "name": format!("imported-{suffix}"),
                "project_id": project_id,
                "route_pattern": format!("/services/imported-{suffix}/*"),
                "upstream_base_url": mock_url,
                "health_check_path": "/health",
                "health_check_method": "GET",
                "allowed_methods": ["GET", "POST"],
                "default_pricing": {"cost_mode": "fixed", "estimated_cost_usd": 0.02}
            })),
        ),
        (
            Method::POST,
            "/admin-ui/admin/services/sync".to_owned(),
            Some(json!({
                "studio_service_id": format!("import-{suffix}"),
                "name": format!("imported-{suffix}"),
                "project_id": project_id,
                "route_pattern": format!("/services/imported-{suffix}/*"),
                "upstream_base_url": mock_url,
                "health_check_path": "/health",
                "health_check_method": "HEAD",
                "allowed_methods": ["POST"]
            })),
        ),
    ] {
        let (status, value) = call(app.clone(), method, &path, Some(&token), body).await;
        expect_success(status, &value, &path);
    }

    let activated_name = format!("activated-{suffix}");
    let activation_request = json!({
        "source": "coverage",
        "services": [{
            "studio_service_id": format!("activated-{suffix}"),
            "name": activated_name,
            "project_id": project_id,
            "route_pattern": format!("/services/activated-{suffix}/*"),
            "upstream_base_url": mock_url,
            "health_check_path": "/health",
            "allowed_methods": ["POST"]
        }]
    });
    let (status, activation) = call(
        app.clone(),
        Method::POST,
        "/admin-ui/admin/services/import/activate",
        Some(&token),
        Some(activation_request),
    )
    .await;
    expect_success(status, &activation, "activate service registry import");
    let snapshot_version = activation["snapshot"]["version"]
        .as_i64()
        .expect("snapshot version");
    let rollback_path = format!("/admin-ui/admin/services/import/rollback/{snapshot_version}");
    let (status, rollback) = call(
        app.clone(),
        Method::POST,
        &rollback_path,
        Some(&token),
        None,
    )
    .await;
    expect_success(status, &rollback, "rollback service registry import");

    let preview_path = format!("/admin-ui/admin/services/{service_name}/openapi/preview");
    let (status, preview) = call(
        app.clone(),
        Method::POST,
        &preview_path,
        Some(&token),
        Some(json!({})),
    )
    .await;
    expect_success(status, &preview, &preview_path);
    let schema_hash = preview["schema_hash"]
        .as_str()
        .expect("OpenAPI preview schema hash");
    let sync_path = format!("/admin-ui/admin/services/{service_name}/openapi/sync");
    let (status, value) = call(
        app.clone(),
        Method::POST,
        &sync_path,
        Some(&token),
        Some(json!({"expected_schema_hash": schema_hash})),
    )
    .await;
    expect_success(status, &value, &sync_path);

    let (status, value) = call(
        app.clone(),
        Method::GET,
        "/admin-ui/v1/guardrails",
        Some(raw_key),
        None,
    )
    .await;
    expect_success(status, &value, "virtual-key guardrail catalog");
    let (status, value) = call(
        app.clone(),
        Method::POST,
        "/admin-ui/v1/guardrails/test",
        Some(raw_key),
        Some(json!({
            "guardrails": ["pii-redact"],
            "mode": "pre_call",
            "input": {"input": "alice@example.com"}
        })),
    )
    .await;
    expect_success(status, &value, "virtual-key guardrail test");

    for path in [
        "/admin-ui/admin/projects",
        "/admin-ui/admin/keys",
        "/admin-ui/admin/services",
        "/admin-ui/admin/providers",
        "/admin-ui/admin/policy-layers",
        "/admin-ui/admin/openai-routes",
        "/admin-ui/admin/anthropic-routes",
        "/admin-ui/admin/providers/litellm-passthrough",
        "/admin-ui/admin/studio/connection",
        "/admin-ui/admin/auth/front-door",
        "/admin-ui/admin/guardrails",
        "/admin-ui/admin/audit-events",
        "/admin-ui/admin/provider-health/state",
        "/admin-ui/admin/services/import/versions",
    ] {
        let (status, value) = call(app.clone(), Method::GET, path, Some(&token), None).await;
        expect_success(status, &value, path);
    }

    for path in [
        format!("/admin-ui/admin/projects/{project_id}"),
        format!("/admin-ui/admin/keys/{key_id}"),
        format!("/admin-ui/admin/keys/{key_id}/usage"),
        format!("/admin-ui/admin/projects/{project_id}/usage"),
        format!("/admin-ui/admin/services/{service_name}"),
        format!("/admin-ui/admin/services/{service_name}/sync-status"),
    ] {
        let (status, value) = call(app.clone(), Method::GET, &path, Some(&token), None).await;
        expect_success(status, &value, &path);
    }

    let mutations = [
        (
            Method::PATCH,
            format!("/admin-ui/admin/projects/{project_id}"),
            json!({"name": format!("api-coverage-updated-{suffix}"), "service_names": [service_name]}),
        ),
        (
            Method::PATCH,
            format!("/admin-ui/admin/keys/{key_id}"),
            json!({"rotation_due_at": null, "policy": {"rpm_limit": 30}}),
        ),
        (
            Method::PATCH,
            format!("/admin-ui/admin/services/{service_name}"),
            json!({"timeout_ms": 45000, "max_body_bytes": 1048576, "openapi_source_path": "/openapi-v2.json"}),
        ),
        (
            Method::POST,
            format!("/admin-ui/admin/keys/{key_id}/disable"),
            Value::Null,
        ),
        (
            Method::POST,
            format!("/admin-ui/admin/keys/{key_id}/enable"),
            Value::Null,
        ),
        (
            Method::POST,
            format!("/admin-ui/admin/services/{service_name}/disable"),
            Value::Null,
        ),
        (
            Method::POST,
            format!("/admin-ui/admin/services/{service_name}/enable"),
            Value::Null,
        ),
    ];
    for (method, path, body) in mutations {
        let body = (!body.is_null()).then_some(body);
        let (status, value) = call(app.clone(), method, &path, Some(&token), body).await;
        expect_success(status, &value, &path);
    }

    let (status, layer) = call(
        app.clone(),
        Method::POST,
        "/admin-ui/admin/policy-layers",
        Some(&token),
        Some(json!({
            "kind": "project",
            "scope_id": project_id,
            "policy": {"rpm_limit": 20},
            "guardrail_policy": {}
        })),
    )
    .await;
    expect_success(status, &layer, "create policy layer");
    let layer_id = layer["id"].as_str().expect("layer id");

    let (status, provider) = call(
        app.clone(),
        Method::POST,
        "/admin-ui/admin/providers",
        Some(&token),
        Some(json!({
            "provider": "internal-service",
            "name": format!("api-provider-{suffix}"),
            "base_url": mock_url,
            "credential": "provider-secret"
        })),
    )
    .await;
    expect_success(status, &provider, "create provider");
    let provider_id = provider["id"].as_str().expect("provider id");
    for (method, suffix_path, body) in [
        (Method::GET, "", None),
        (
            Method::PATCH,
            "",
            Some(json!({"name": format!("api-provider-updated-{suffix}")})),
        ),
        (Method::POST, "/disable", None),
        (Method::POST, "/enable", None),
    ] {
        let path = format!("/admin-ui/admin/providers/{provider_id}{suffix_path}");
        let (status, value) = call(app.clone(), method, &path, Some(&token), body).await;
        expect_success(status, &value, &path);
    }

    let (status, mapping) = call(
        app.clone(),
        Method::POST,
        "/admin-ui/admin/providers/litellm-credentials",
        Some(&token),
        Some(json!({
            "scope": "key",
            "target_id": key_id,
            "credential": "mapped-key"
        })),
    )
    .await;
    expect_success(status, &mapping, "create credential mapping");
    let mapping_id = mapping["id"].as_str().expect("mapping id");
    for path in [
        "/admin-ui/admin/providers/litellm-credentials".to_owned(),
        format!("/admin-ui/admin/providers/litellm-credentials/{mapping_id}/disable"),
        format!("/admin-ui/admin/providers/litellm-credentials/{mapping_id}/enable"),
    ] {
        let method = if path.ends_with("credentials") {
            Method::GET
        } else {
            Method::POST
        };
        let (status, value) = call(app.clone(), method, &path, Some(&token), None).await;
        expect_success(status, &value, &path);
    }

    for (method, path, body) in [
        (
            Method::POST,
            "/admin-ui/admin/openai-routes/chat-completions/disable",
            None,
        ),
        (
            Method::POST,
            "/admin-ui/admin/openai-routes/chat-completions/enable",
            None,
        ),
        (
            Method::PATCH,
            "/admin-ui/admin/openai-routes/responses/mode",
            Some(json!({"mode": "direct_litellm_passthrough"})),
        ),
        (
            Method::PATCH,
            "/admin-ui/admin/openai-routes/embeddings/config",
            Some(
                json!({"timeout_ms": 30000, "max_request_body_bytes": 500000, "max_response_body_bytes": 600000}),
            ),
        ),
        (
            Method::POST,
            "/admin-ui/admin/anthropic-routes/messages/disable",
            None,
        ),
        (
            Method::POST,
            "/admin-ui/admin/anthropic-routes/messages/enable",
            None,
        ),
        (
            Method::PATCH,
            "/admin-ui/admin/anthropic-routes/messages/mode",
            Some(json!({"mode": "managed_by_gateway"})),
        ),
        (
            Method::PATCH,
            "/admin-ui/admin/anthropic-routes/messages/config",
            Some(json!({"timeout_ms": 31000})),
        ),
        (
            Method::PATCH,
            "/admin-ui/admin/providers/litellm-passthrough",
            Some(
                json!({"enabled": true, "allowed_paths": ["/models"], "allowed_methods": ["GET"], "ui_exposure": "operator_only"}),
            ),
        ),
        (
            Method::PATCH,
            "/admin-ui/admin/studio/connection",
            Some(json!({"base_url": "https://studio.example", "token": "studio-secret"})),
        ),
        (
            Method::PATCH,
            "/admin-ui/admin/auth/front-door",
            Some(
                json!({"relayna_key_header": "x-relayna-key", "entra_enabled": false, "apigee_trusted_header_enabled": false}),
            ),
        ),
    ] {
        let (status, value) = call(app.clone(), method, path, Some(&token), body).await;
        expect_success(status, &value, path);
    }

    let guardrail_name = format!("api-guardrail-{suffix}");
    let (status, value) = call(
        app.clone(),
        Method::POST,
        "/admin-ui/admin/guardrails",
        Some(&token),
        Some(json!({
            "name": guardrail_name,
            "description": "API coverage guardrail",
            "endpoint_url": "https://guardrail.example/check",
            "modes": ["pre_call"],
            "failure_policy": "fail_open",
            "bearer_token": "secret"
        })),
    )
    .await;
    expect_success(status, &value, "create guardrail");
    let guardrail_path = format!("/admin-ui/admin/guardrails/{guardrail_name}");
    let (status, value) = call(
        app.clone(),
        Method::PATCH,
        &guardrail_path,
        Some(&token),
        Some(json!({"description": "updated", "enabled": true})),
    )
    .await;
    expect_success(status, &value, "patch guardrail");
    for path in [
        "/admin-ui/admin/guardrails/executions",
        "/admin-ui/admin/guardrails/summary",
    ] {
        let (status, value) = call(app.clone(), Method::GET, path, Some(&token), None).await;
        expect_success(status, &value, path);
    }

    let (status, value) = call(
        app.clone(),
        Method::POST,
        "/admin-ui/admin/policy/simulate",
        Some(&token),
        Some(json!({
            "key_id": key_id,
            "path": "/v1/messages/batches/batch-coverage/cancel",
            "provider": "litellm",
            "model": "coverage-model",
            "stream": true,
            "uses_tools": true,
            "estimated_input_tokens": 10,
            "estimated_output_tokens": 20,
            "estimated_cost_usd": 0.01,
            "policy_patch": {
                "deny": false,
                "allowed_routes": ["/v1/messages/batches/*/cancel"],
                "allowed_providers": ["litellm"],
                "allowed_models": ["coverage-model"],
                "allowed_services": [service_name],
                "rpm_limit": 100,
                "tpm_limit": 1000,
                "daily_budget_usd": 10.0,
                "monthly_budget_usd": 100.0,
                "allow_streaming": true,
                "allow_tools": true,
                "max_requests_per_day": 1000,
                "max_tokens_per_day": 10000,
                "max_cost_per_request": 1.0,
                "max_input_tokens_per_request": 100,
                "max_output_tokens_per_request": 100,
                "allowed_hours_utc": [0, 12, 23],
                "unused_key_auto_disable_after_days": 30,
                "max_request_body_bytes": 1000000,
                "max_response_body_bytes": 1000000,
                "max_stream_duration_seconds": 60,
                "max_sse_event_bytes": 100000,
                "max_tool_call_count": 8,
                "max_tool_schema_bytes": 100000
            }
        })),
    )
    .await;
    expect_success(status, &value, "simulate policy");

    for path in [
        "/admin-ui/admin/usage/summary",
        "/admin-ui/admin/usage/dashboard",
        "/admin-ui/admin/usage/timeseries",
        "/admin-ui/admin/usage/by-key",
        "/admin-ui/admin/usage/by-project",
        "/admin-ui/admin/usage/by-model",
        "/admin-ui/admin/usage/by-provider",
        "/admin-ui/admin/usage/by-service",
        "/admin-ui/admin/usage/by-task",
        "/admin-ui/admin/usage/events",
        "/admin-ui/admin/usage/filter-values?field=service",
        "/admin-ui/admin/usage/unused-keys",
        "/admin-ui/admin/usage/export.json",
        "/admin-ui/admin/usage/export.csv",
        "/admin-ui/admin/provider-health",
        "/admin-ui/admin/debug-bundles/missing-coverage-request",
    ] {
        let (status, value) = call(app.clone(), Method::GET, path, Some(&token), None).await;
        assert!(
            status.is_success() || status == StatusCode::NOT_FOUND,
            "{path} failed with {status}: {value}"
        );
    }

    for (method, path, body) in [
        (
            Method::PATCH,
            format!("/admin-ui/admin/projects/{missing_id}"),
            Some(json!({"name": "missing-project"})),
        ),
        (
            Method::DELETE,
            format!("/admin-ui/admin/projects/{missing_id}"),
            None,
        ),
        (
            Method::PATCH,
            format!("/admin-ui/admin/keys/{missing_id}"),
            Some(json!({"rotation_due_at": null})),
        ),
        (
            Method::POST,
            format!("/admin-ui/admin/keys/{missing_id}/disable"),
            None,
        ),
        (
            Method::POST,
            format!("/admin-ui/admin/keys/{missing_id}/enable"),
            None,
        ),
        (
            Method::POST,
            format!("/admin-ui/admin/keys/{missing_id}/revoke"),
            None,
        ),
        (
            Method::PATCH,
            format!("/admin-ui/admin/providers/{missing_id}"),
            Some(json!({"name": "missing-provider"})),
        ),
        (
            Method::DELETE,
            format!("/admin-ui/admin/providers/{missing_id}"),
            None,
        ),
        (
            Method::POST,
            format!("/admin-ui/admin/providers/{missing_id}/disable"),
            None,
        ),
        (
            Method::POST,
            format!("/admin-ui/admin/providers/{missing_id}/enable"),
            None,
        ),
        (
            Method::DELETE,
            format!("/admin-ui/admin/providers/litellm-credentials/{missing_id}"),
            None,
        ),
        (
            Method::POST,
            format!("/admin-ui/admin/providers/litellm-credentials/{missing_id}/disable"),
            None,
        ),
        (
            Method::POST,
            format!("/admin-ui/admin/providers/litellm-credentials/{missing_id}/enable"),
            None,
        ),
        (
            Method::PATCH,
            "/admin-ui/admin/services/missing-coverage-service".to_owned(),
            Some(json!({"timeout_ms": 1000})),
        ),
        (
            Method::DELETE,
            "/admin-ui/admin/services/missing-coverage-service".to_owned(),
            None,
        ),
        (
            Method::POST,
            "/admin-ui/admin/services/missing-coverage-service/disable".to_owned(),
            None,
        ),
        (
            Method::POST,
            "/admin-ui/admin/services/missing-coverage-service/enable".to_owned(),
            None,
        ),
        (
            Method::DELETE,
            "/admin-ui/admin/guardrails/missing-coverage-guardrail".to_owned(),
            None,
        ),
        (
            Method::DELETE,
            format!("/admin-ui/admin/policy-layers/{missing_id}"),
            None,
        ),
    ] {
        let (status, _) = call(app.clone(), method, &path, Some(&token), body).await;
        assert!(
            status == StatusCode::NOT_FOUND || status == StatusCode::BAD_REQUEST,
            "{path} returned {status}"
        );
    }

    let empty_project_name = format!("empty-coverage-{suffix}");
    let (status, empty_project) = call(
        app.clone(),
        Method::POST,
        "/admin-ui/admin/projects",
        Some(&token),
        Some(json!({"name": empty_project_name})),
    )
    .await;
    expect_success(status, &empty_project, "create disposable project");
    let empty_project_id = empty_project["id"].as_str().expect("empty project id");
    let (status, value) = call(
        app.clone(),
        Method::DELETE,
        &format!("/admin-ui/admin/projects/{empty_project_id}"),
        Some(&token),
        None,
    )
    .await;
    expect_success(status, &value, "delete disposable project");

    let (status, value) = call(
        app.clone(),
        Method::POST,
        "/admin-ui/admin/provider-health/check",
        Some(&token),
        None,
    )
    .await;
    expect_success(status, &value, "run provider health checks");

    for path in [
        format!("/admin-ui/admin/projects/{missing_id}"),
        format!("/admin-ui/admin/keys/{missing_id}"),
        format!("/admin-ui/admin/providers/{missing_id}"),
        format!("/admin-ui/admin/debug-bundles/{missing_id}"),
        format!("/admin-ui/admin/tasks/{missing_id}/usage"),
        "/admin-ui/admin/services/missing-coverage-service".to_owned(),
    ] {
        let (status, _) = call(app.clone(), Method::GET, &path, Some(&token), None).await;
        assert!(
            status == StatusCode::NOT_FOUND || status.is_success(),
            "{path} returned {status}"
        );
    }

    for (method, path) in [
        (
            Method::DELETE,
            format!("/admin-ui/admin/providers/litellm-credentials/{mapping_id}"),
        ),
        (Method::DELETE, guardrail_path),
        (
            Method::DELETE,
            format!("/admin-ui/admin/policy-layers/{layer_id}"),
        ),
        (
            Method::POST,
            format!("/admin-ui/admin/keys/{key_id}/revoke"),
        ),
        (
            Method::DELETE,
            format!("/admin-ui/admin/providers/{provider_id}"),
        ),
        (
            Method::DELETE,
            format!("/admin-ui/admin/services/{service_name}"),
        ),
        (
            Method::DELETE,
            format!("/admin-ui/admin/services/imported-{suffix}"),
        ),
        (
            Method::DELETE,
            format!("/admin-ui/admin/services/activated-{suffix}"),
        ),
    ] {
        let (status, value) = call(app.clone(), method, &path, Some(&token), None).await;
        expect_success(status, &value, &path);
    }

    let (status, _) = call(
        app.clone(),
        Method::DELETE,
        &format!("/admin-ui/admin/projects/{project_id}"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);

    let (status, rotated) = call(
        app,
        Method::POST,
        "/admin-ui/admin/operator-token/rotate",
        Some(&token),
        Some(json!({"label": "coverage-rotation"})),
    )
    .await;
    expect_success(status, &rotated, "rotate operator token");
    assert!(rotated["raw_token"].as_str().is_some());
    mock_task.abort();
}
