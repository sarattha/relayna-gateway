//! Real Pingora, disposable PostgreSQL and isolated Redis keys; mock upstream only.
use axum::{
    body::{Body, Bytes},
    extract::OriginalUri,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::any,
    Json, Router,
};
use gateway_core::*;
use gateway_proxy::{PingoraLiteLlmConfig, RelaynaPingoraProxy};
use gateway_store::{PostgresStore, RedisControlState};
use serde_json::{json, Value};
use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};
use uuid::Uuid;

async fn key(store: &PostgresStore, project_id: Uuid, policy: Value) -> (String, Uuid) {
    let material = VirtualKeyMaterial::generate().unwrap();
    let record = store
        .create_admin_key(
            serde_json::from_value(
                json!({"owner_type":"project", "project_id":project_id, "policy":policy}),
            )
            .unwrap(),
            &material,
        )
        .await
        .unwrap();
    (material.raw_key, record.id)
}

async fn project(store: &PostgresStore) -> Uuid {
    let project = store
        .create_project(ProjectCreateRequest {
            name: format!("security-{}", Uuid::new_v4()),
        })
        .await
        .unwrap();
    store
        .upsert_policy_layer(AdminPolicyLayerUpsert {
            kind: PolicyLayerKind::Project,
            scope_id: Some(project.id.to_string()),
            policy: Default::default(),
            guardrail_policy: Default::default(),
        })
        .await
        .unwrap();
    project.id
}

async fn post(
    client: &reqwest::Client,
    url: &str,
    path: &str,
    key: &str,
    body: Value,
) -> reqwest::Response {
    client
        .post(format!("{url}{path}"))
        .bearer_auth(key)
        .json(&body)
        .send()
        .await
        .unwrap_or_else(|error| panic!("POST {path}: {error}"))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn security_controls_deny_before_forwarding_and_account_large_streamed_responses() {
    let (Ok(database), Ok(redis)) = (std::env::var("DATABASE_URL"), std::env::var("REDIS_URL"))
    else {
        eprintln!("security regressions require DATABASE_URL and REDIS_URL");
        return;
    };
    let parent = sqlx::PgPool::connect(&database).await.unwrap();
    let name = format!("security_{}", Uuid::new_v4().simple());
    sqlx::query(&format!("CREATE DATABASE {name}"))
        .execute(&parent)
        .await
        .unwrap();
    let mut database = url::Url::parse(&database).unwrap();
    database.set_path(&name);
    let store = PostgresStore::connect(database.as_str()).await.unwrap();
    store
        .upsert_policy_layer(AdminPolicyLayerUpsert {
            kind: PolicyLayerKind::Global,
            scope_id: None,
            policy: Default::default(),
            guardrail_policy: Default::default(),
        })
        .await
        .unwrap();
    let received = Arc::new(AtomicUsize::new(0));
    let capture = received.clone();
    let finish_response = Arc::new(tokio::sync::Notify::new());
    let upstream_finish = finish_response.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream_url = format!("http://{}", listener.local_addr().unwrap());
    let upstream = Router::new().fallback(any(move |OriginalUri(uri):OriginalUri, headers:HeaderMap, body:Bytes| {
        let capture = capture.clone();
        let finish = upstream_finish.clone();
        async move {
            capture.fetch_add(1, Ordering::SeqCst);
            if uri.path() == "/large" {
                if headers.get("accept-encoding").is_some_and(|value| value.to_str().unwrap_or_default().contains("gzip")) {
                    return Response::builder().header("content-type","application/json").header("content-encoding","gzip")
                        .body(Body::from(Bytes::from_static(include_bytes!("fixtures/security-cost-response.json.gz")))).unwrap();
                }
                let chunks = [format!("{{\"payload\":\"{}", "x".repeat(100_000)), "\",\"usage\":{\"total_cost\":0.4}}".into()];
                let stream = futures_util::stream::unfold((chunks.into_iter(), finish), |(mut chunks, finish)| async move {
                    let chunk = chunks.next()?;
                    if chunk.starts_with('"') { finish.notified().await; }
                    Some((Ok::<_, std::io::Error>(Bytes::from(chunk)), (chunks, finish)))
                });
                return Response::builder().header("content-type","application/json").body(Body::from_stream(stream)).unwrap();
            }
            if serde_json::from_slice::<Value>(&body).ok().is_some_and(|value| value["stream"] == true) {
                return ([ ("content-type","text/event-stream") ], "data: {\"choices\":[{\"delta\":{\"content\":\"ok\"}}]}\n\ndata: [DONE]\n\n").into_response();
            }
            Json(json!({"path":uri.path(),"auth":headers.get("authorization").and_then(|v|v.to_str().ok()),"usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}})).into_response()
        }
    }));
    let upstream_task = tokio::spawn(async move {
        axum::serve(listener, upstream).await.unwrap();
    });
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let state = RedisControlState::new(&redis).unwrap();
    let proxy = RelaynaPingoraProxy::new(
        Arc::new(store.clone()),
        Arc::new(state.clone()),
        PingoraLiteLlmConfig::from_base_url(&upstream_url, "shared-test-upstream").unwrap(),
    );
    std::thread::spawn(move || {
        let mut server = pingora_core::server::Server::new(None).unwrap();
        server.bootstrap();
        let mut service = pingora_proxy::http_proxy_service(&server.configuration, proxy);
        service.add_tcp(&format!("127.0.0.1:{port}"));
        server.add_service(service);
        server.run_forever();
    });
    for attempt in 0..600 {
        if tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .is_ok()
        {
            break;
        }
        assert!(attempt < 599);
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let url = format!("http://127.0.0.1:{port}");
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .unwrap();
    let project_id = project(&store).await;
    let (restricted, restricted_id) = key(&store,project_id,json!({"allowed_routes":["/v1/chat/completions","/v1/responses"],"allowed_models":["allowed"],"allowed_providers":["litellm"],"allow_streaming":false,"allow_tools":false})).await;

    // Both released modes must enforce complete metadata before upstream receipt.
    for mode in [
        OpenAiRouteMode::ManagedByGateway,
        OpenAiRouteMode::DirectLiteLlmPassthrough,
    ] {
        for route in ["chat-completions", "responses"] {
            store.set_openai_route_mode(route, mode).await.unwrap();
        }
        for path in [
            "/v1/chat/completions",
            "/chat/completions",
            "/v1/responses",
            "/responses",
        ] {
            let before = received.load(Ordering::SeqCst);
            for body in [
                json!({"model":"forbidden"}),
                json!({"model":"allowed","stream":true}),
                json!({"model":"allowed","tools":[{"type":"function"}]}),
                json!({"model":"allowed","functions":[{"name":"legacy"}]}),
                json!({"model":"allowed","extra_body":{"stream":true}}),
                json!({"model":"allowed","extra_body":{"tools":[{"type":"function"}]}}),
            ] {
                let response = post(&client, &url, path, &restricted, body).await;
                assert_eq!(response.status(), StatusCode::FORBIDDEN, "{mode:?} {path}");
                assert_eq!(
                    response.json::<Value>().await.unwrap()["error"]["code"],
                    "policy_denied"
                );
            }
            assert_eq!(received.load(Ordering::SeqCst), before);
            let response = post(&client, &url, path, &restricted, json!({"model":"allowed"})).await;
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(
                response.json::<Value>().await.unwrap()["auth"],
                "Bearer shared-test-upstream"
            );
        }
    }
    for (column, status) in [
        ("daily_budget_usd", StatusCode::PAYMENT_REQUIRED),
        ("tpm_limit", StatusCode::TOO_MANY_REQUESTS),
    ] {
        sqlx::query(&format!(
            "UPDATE key_policies SET {column}=0 WHERE key_id=$1"
        ))
        .bind(restricted_id)
        .execute(store.pool())
        .await
        .unwrap();
        let before = received.load(Ordering::SeqCst);
        assert_eq!(
            post(
                &client,
                &url,
                "/responses",
                &restricted,
                json!({"model":"allowed","input":"paid work"})
            )
            .await
            .status(),
            status
        );
        assert_eq!(received.load(Ordering::SeqCst), before);
        sqlx::query(&format!(
            "UPDATE key_policies SET {column}=NULL WHERE key_id=$1"
        ))
        .bind(restricted_id)
        .execute(store.pool())
        .await
        .unwrap();
    }

    let batch_policy = json!({"allowed_routes":["/v1/messages/batches","/v1/messages/batches/*","/v1/messages/batches/*/results","/v1/messages/batches/*/cancel","/litellm/*"],"allowed_providers":["litellm"],"allowed_models":[],"allow_tools":true});
    let (batch_a, id_a) = key(&store, project_id, batch_policy.clone()).await;
    let project_b = project(&store).await;
    let (batch_b, id_b) = key(&store, project_b, batch_policy).await;
    store.patch_litellm_passthrough_settings(serde_json::from_value(json!({"enabled":true,"allowed_paths":["/v1/*"],"allowed_methods":["GET","POST"],"authentication_mode":"gateway"})).unwrap()).await.unwrap();
    for mapping in [
        None,
        Some(LiteLlmCredentialMappingScope::Project),
        Some(LiteLlmCredentialMappingScope::Key),
    ] {
        if let Some(scope) = mapping {
            for target_id in if scope == LiteLlmCredentialMappingScope::Key {
                [id_a, id_b]
            } else {
                [project_id, project_b]
            } {
                store
                    .upsert_litellm_credential_mapping(LiteLlmCredentialMappingUpsertRequest {
                        scope,
                        target_id,
                        enabled: true,
                        credential: Some("same-shared-mapped-secret".into()),
                    })
                    .await
                    .unwrap();
            }
        }
        for bearer in [&batch_a, &batch_b] {
            let before = received.load(Ordering::SeqCst);
            for (method, path) in [
                (reqwest::Method::GET, "/v1/messages/batches"),
                (reqwest::Method::GET, "/v1/messages/batches/other"),
                (reqwest::Method::GET, "/v1/messages/batches/other/results"),
                (reqwest::Method::POST, "/v1/messages/batches/other/cancel"),
                (reqwest::Method::POST, "/v1/messages/batches/"),
                (reqwest::Method::GET, "/v1/messages/batches%2Fother/results"),
                (reqwest::Method::GET, "/v1/messages/batches%2fother/results"),
                (reqwest::Method::GET, "/v1/messages/%62atches/other/results"),
                (
                    reqwest::Method::GET,
                    "/v1/messages/batches%252Fother/results",
                ),
            ] {
                let response = client
                    .request(method, format!("{url}{path}"))
                    .bearer_auth(bearer)
                    .send()
                    .await
                    .unwrap();
                assert_eq!(response.status(), StatusCode::FORBIDDEN, "{path}");
            }
            assert_eq!(received.load(Ordering::SeqCst), before);
        }
    }
    // A large valid batch with one model must remain practical to admit.
    let entries: Vec<_> = (0..3_000)
        .map(|index| {
            json!({"custom_id":index.to_string(),
        "params":{"model":"allowed","max_tokens":1}})
        })
        .collect();
    let before = received.load(Ordering::SeqCst);
    assert_eq!(
        post(
            &client,
            &url,
            "/v1/messages/batches",
            &batch_a,
            json!({"requests":entries})
        )
        .await
        .status(),
        StatusCode::OK
    );
    assert_eq!(received.load(Ordering::SeqCst), before + 1);
    store
        .set_anthropic_route_mode(
            "message-batch-results",
            OpenAiRouteMode::DirectLiteLlmPassthrough,
        )
        .await
        .unwrap();
    let response = client
        .get(format!("{url}/v1/messages/batches/own/results"))
        .bearer_auth("sk-caller-owned-test")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.json::<Value>().await.unwrap()["auth"],
        "Bearer sk-caller-owned-test"
    );

    let (capped,_) = key(&store,project_id,json!({"allowed_routes":["/v1/chat/completions","/v1/responses","/v1/messages/batches"],"allowed_models":["allowed"],"allowed_providers":["litellm"],"allow_streaming":true,"max_output_tokens_per_request":2})).await;
    for mode in [
        OpenAiRouteMode::ManagedByGateway,
        OpenAiRouteMode::DirectLiteLlmPassthrough,
    ] {
        store
            .set_openai_route_mode("responses", mode)
            .await
            .unwrap();
        let before = received.load(Ordering::SeqCst);
        for body in [
            json!({"model":"allowed","stream":true}),
            json!({"model":"allowed","stream":true,"max_output_tokens":3}),
            json!({"model":"allowed","stream":true,"max_output_tokens":2,"max_tokens":100}),
            json!({"model":"allowed","stream":true,"max_output_tokens":2,"extra_body":{"max_output_tokens":100}}),
        ] {
            assert_eq!(
                post(&client, &url, "/responses", &capped, body)
                    .await
                    .status(),
                StatusCode::FORBIDDEN
            );
        }
        assert_eq!(received.load(Ordering::SeqCst), before);
        let response = post(
            &client,
            &url,
            "/responses",
            &capped,
            json!({"model":"allowed","stream":true,"max_output_tokens":2}),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert!(response.text().await.unwrap().contains("data: [DONE]"));
    }

    // Per-request caps cover completion multiplicity and every batch entry.
    let before = received.load(Ordering::SeqCst);
    for (path, body) in [
        (
            "/v1/chat/completions",
            json!({"model":"allowed","stream":true,"max_tokens":2,"n":2}),
        ),
        (
            "/v1/chat/completions",
            json!({"model":"allowed","stream":true,"max_tokens":2,"max_new_tokens":100}),
        ),
        (
            "/v1/messages/batches",
            json!({"requests":[{"custom_id":"one","params":{"model":"allowed","max_tokens":2}},{"custom_id":"two","params":{"model":"allowed","max_tokens":1}}]}),
        ),
    ] {
        assert_eq!(
            post(&client, &url, path, &capped, body).await.status(),
            StatusCode::FORBIDDEN
        );
    }
    assert_eq!(received.load(Ordering::SeqCst), before);
    assert_eq!(post(&client, &url, "/v1/messages/batches", &capped,
        json!({"requests":[{"custom_id":"one","params":{"model":"allowed","max_tokens":1}},{"custom_id":"two","params":{"model":"allowed","max_tokens":1}}]})).await.status(), StatusCode::OK);

    let (wildcard_capped, _) = key(&store, project_id, json!({"allowed_routes":["/litellm/*"],"allowed_providers":["litellm"],"allow_streaming":true,"max_output_tokens_per_request":2})).await;
    let before = received.load(Ordering::SeqCst);
    assert_eq!(
        post(
            &client,
            &url,
            "/v1/unknown-generation",
            &wildcard_capped,
            json!({"stream":true,"max_tokens":2})
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(received.load(Ordering::SeqCst), before);

    let (model_capped, _) = key(&store, project_id, json!({"allowed_routes":["/litellm/*","/v1/chat/completions","/v1/responses","/v1/messages/batches"],"allowed_providers":["litellm"],"allow_streaming":true,"allow_tools":true})).await;
    store
        .upsert_policy_layer(AdminPolicyLayerUpsert {
            kind: PolicyLayerKind::Model,
            scope_id: Some("capped-model".into()),
            policy: serde_json::from_value(json!({"max_output_tokens_per_request":2})).unwrap(),
            guardrail_policy: Default::default(),
        })
        .await
        .unwrap();
    for mode in [
        OpenAiRouteMode::ManagedByGateway,
        OpenAiRouteMode::DirectLiteLlmPassthrough,
    ] {
        for route in ["chat-completions", "responses"] {
            store.set_openai_route_mode(route, mode).await.unwrap();
        }
        let before = received.load(Ordering::SeqCst);
        for path in ["/v1/chat/completions", "/responses"] {
            assert_eq!(
                post(
                    &client,
                    &url,
                    path,
                    &model_capped,
                    json!({"model":"uncapped-model","extra_body":{"model":"capped-model"}})
                )
                .await
                .status(),
                StatusCode::FORBIDDEN,
                "{mode:?} {path}"
            );
        }
        assert_eq!(received.load(Ordering::SeqCst), before);
        assert_eq!(
            post(
                &client,
                &url,
                "/responses",
                &model_capped,
                json!({"model":"uncapped-model"})
            )
            .await
            .status(),
            StatusCode::OK
        );
    }
    // The strictest model cap still bounds the whole mixed-model batch.
    let before = received.load(Ordering::SeqCst);
    assert_eq!(
        post(
            &client,
            &url,
            "/v1/unknown-generation",
            &model_capped,
            json!({"model":"uncapped-model","extra_body":{"model":"capped-model"}})
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        post(
            &client,
            &url,
            "/v1/messages/batches",
            &model_capped,
            json!({"requests":[{"params":{"model":"uncapped-model","max_tokens":2}},
        {"params":{"model":"capped-model","max_tokens":1}}]})
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(received.load(Ordering::SeqCst), before);
    assert_eq!(
        post(
            &client,
            &url,
            "/v1/messages/batches",
            &model_capped,
            json!({"requests":[{"params":{"model":"uncapped-model","max_tokens":1}},
        {"params":{"model":"capped-model","max_tokens":1}}]})
        )
        .await
        .status(),
        StatusCode::OK
    );
    let before = received.load(Ordering::SeqCst);
    assert_eq!(
        post(
            &client,
            &url,
            "/v1/unknown-generation",
            &model_capped,
            json!({"model":"capped-model","stream":true})
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(received.load(Ordering::SeqCst), before);
    assert_eq!(
        post(
            &client,
            &url,
            "/v1/unknown-generation",
            &model_capped,
            json!({"model":"uncapped-model"})
        )
        .await
        .status(),
        StatusCode::OK
    );
    let before = received.load(Ordering::SeqCst);
    assert_eq!(
        client
            .post(format!("{url}/v1/unknown-generation"))
            .bearer_auth(&model_capped)
            .header("content-type", "application/json")
            .body(r#"{"model":"uncapped-model","model":"capped-model","stream":true}"#)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(received.load(Ordering::SeqCst), before);

    // A large response arrives before its final cost chunk; accounting must not
    // buffer the response, lose the charge, or release an uncharged reservation.
    let service_name = format!("large-{}", Uuid::new_v4());
    store.create_service(serde_json::from_value(json!({"name":service_name,"project_id":project_id,"route_pattern":format!("/services/{service_name}/*"),"upstream_base_url":upstream_url,"credential":"service-test-credential","allowed_methods":["POST"],"cost_mode":"passthrough"})).unwrap()).await.unwrap();
    let (service_key,service_id) = key(&store,project_id,json!({"allowed_routes":["/services/*"],"allowed_services":[service_name],"allowed_providers":["internal-service"],"allow_tools":true,"allow_streaming":true,"daily_budget_usd":0.7})).await;
    for expected in [0.4, 0.8] {
        let mut response = client
            .post(format!("{url}/services/{service_name}/large"))
            .bearer_auth(&service_key)
            .header("accept-encoding", "gzip, br")
            .json(&json!({}))
            .send()
            .await
            .unwrap();
        if response.status() != StatusCode::OK {
            panic!(
                "large service response: {} {}",
                response.status(),
                response.text().await.unwrap()
            );
        }
        let first = tokio::time::timeout(Duration::from_secs(5), response.chunk())
            .await
            .expect("first chunk before upstream completion")
            .unwrap()
            .unwrap();
        assert!(!first.is_empty());
        finish_response.notify_one();
        let mut result = first.to_vec();
        while let Some(chunk) = response.chunk().await.unwrap() {
            result.extend_from_slice(&chunk);
        }
        assert_eq!(
            serde_json::from_slice::<Value>(&result).unwrap()["usage"]["total_cost"],
            0.4
        );
        for attempt in 0..100 {
            let (_,cost): (i64,f64) = sqlx::query_as("SELECT COUNT(*), COALESCE(SUM(estimated_cost),0)::double precision FROM usage_events WHERE key_id=$1").bind(service_id).fetch_one(store.pool()).await.unwrap();
            if (cost - expected).abs() < 1e-8 {
                break;
            }
            assert!(attempt < 99, "durable cost must reconcile");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }
    let before = received.load(Ordering::SeqCst);
    assert_eq!(
        post(
            &client,
            &url,
            &format!("/services/{service_name}/large"),
            &service_key,
            json!({})
        )
        .await
        .status(),
        StatusCode::PAYMENT_REQUIRED
    );
    assert_eq!(received.load(Ordering::SeqCst), before);
    assert!(matches!(
        state
            .check_budget(service_id, Some(0.7), None, chrono::Utc::now())
            .await
            .unwrap(),
        BudgetDecision::Exceeded(_)
    ));
    upstream_task.abort();
    store.pool().close().await;
    sqlx::query(&format!("DROP DATABASE {name} WITH (FORCE)"))
        .execute(&parent)
        .await
        .unwrap();
}
