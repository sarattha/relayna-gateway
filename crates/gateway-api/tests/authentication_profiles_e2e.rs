//! Real proxy/admin/database, mock OIDC and upstream. No production credentials.
use axum::{
    extract::OriginalUri,
    http::HeaderMap,
    routing::{any, get},
    Json, Router,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use chrono::Utc;
use gateway_core::*;
use gateway_proxy::{PingoraLiteLlmConfig, RelaynaPingoraProxy};
use gateway_store::{PostgresStore, RedisControlState, RedisReadiness};
use serde_json::{json, Value};
use std::{sync::Arc, time::Duration};
use uuid::Uuid;
async fn serve(app: Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{address}")
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn explicit_profiles_enforce_bindings_across_aliases_and_forwarding_modes() {
    let (Ok(database), Ok(redis)) = (std::env::var("DATABASE_URL"), std::env::var("REDIS_URL"))
    else {
        return;
    };
    let parent = sqlx::PgPool::connect(&database).await.unwrap();
    let name = format!("profiles_{}", Uuid::new_v4().simple());
    sqlx::query(&format!("CREATE DATABASE {name}"))
        .execute(&parent)
        .await
        .unwrap();
    let mut url = url::Url::parse(&database).unwrap();
    url.set_path(&name);
    let store = PostgresStore::connect(url.as_str()).await.unwrap();
    store
        .upsert_policy_layer(AdminPolicyLayerUpsert {
            kind: PolicyLayerKind::Global,
            scope_id: None,
            policy: Default::default(),
            guardrail_policy: Default::default(),
        })
        .await
        .unwrap();
    let operator = OperatorTokenMaterial::generate().unwrap();
    store.bootstrap_operator_token(&operator).await.unwrap();
    let mut keys = vec![];
    let mut ids = vec![];
    for _ in 0..4 {
        let key = VirtualKeyMaterial::generate().unwrap();
        let record=store.create_admin_key(serde_json::from_value(json!({"owner_type":"individual","policy":{"allowed_routes":["/v1/responses","/v1/messages","/litellm/*","/services/*"],"allowed_models":[],"allowed_providers":["litellm","internal-service"],"rpm_limit":1000}})).unwrap(),&key).await.unwrap();
        store
            .upsert_litellm_credential_mapping(LiteLlmCredentialMappingUpsertRequest {
                scope: LiteLlmCredentialMappingScope::Key,
                target_id: record.id,
                enabled: true,
                credential: Some("profile-mapped-upstream".into()),
            })
            .await
            .unwrap();
        ids.push(record.id);
        keys.push(key.raw_key);
    }
    let rsa = openssl::rsa::Rsa::generate(2048).unwrap();
    let signing =
        jsonwebtoken::EncodingKey::from_rsa_pem(&rsa.private_key_to_pem().unwrap()).unwrap();
    let jwks = json!({"keys":[{"kty":"RSA","kid":"profiles","alg":"RS256","n":URL_SAFE_NO_PAD.encode(rsa.n().to_vec()),"e":URL_SAFE_NO_PAD.encode(rsa.e().to_vec())}]});
    let key_server = serve(Router::new().route(
        "/keys",
        get(move || {
            let value = jwks.clone();
            async move { Json(value) }
        }),
    ))
    .await;
    let discovery = serve(Router::new().route(
        "/discovery",
        get(move || {
            let uri = format!("{key_server}/keys");
            async move { Json(json!({"issuer":"https://issuer.test","jwks_uri":uri})) }
        }),
    ))
    .await;
    let upstream=serve(Router::new().fallback(any(|headers:HeaderMap,OriginalUri(uri):OriginalUri|async move {
        Json(json!({"path":uri.path(),"auth":headers.get("authorization").and_then(|v|v.to_str().ok()),"key_leaked":headers.contains_key("x-relayna-key"),"admission_leaked":headers.contains_key("x-relayna-admission-context"),"usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}}))
    }))).await;
    let auth = SharedGatewayAuthRuntime::new(GatewayAuthRuntimeConfig {
        unverified_bearer_enabled: false,
        relayna_key_header: "x-relayna-key".into(),
        entra_auth: Some(EntraAuthConfig {
            tenant_id: "tenant".into(),
            audience: "api://legacy".into(),
            issuer: "https://issuer.test".into(),
            oidc_discovery_url: format!("{discovery}/discovery"),
            required_scope: None,
            required_role: None,
            allowed_groups: vec![],
            accepted_algorithms: vec!["RS256".into()],
            relayna_key_header: "x-relayna-key".into(),
            jwks_cache_ttl_seconds: 300,
            clock_skew_seconds: 60,
        }),
        apigee_trusted_header: Some(ApigeeTrustedHeaderConfig {
            secret: "profile-apigee-test".into(),
            required_scope: None,
            required_role: None,
            allowed_groups: vec![],
        }),
    })
    .unwrap();
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let proxy = RelaynaPingoraProxy::new(
        Arc::new(store.clone()),
        Arc::new(RedisControlState::new(&redis).unwrap()),
        PingoraLiteLlmConfig::from_base_url(&upstream, "profile-service-upstream")
            .unwrap()
            .with_auth_runtime(auth.clone()),
    );
    std::thread::spawn(move || {
        let mut server = pingora_core::server::Server::new(None).unwrap();
        server.bootstrap();
        let mut service = pingora_proxy::http_proxy_service(&server.configuration, proxy);
        service.add_tcp(&format!("127.0.0.1:{port}"));
        server.add_service(service);
        server.run_forever();
    });
    for _ in 0..500 {
        if tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .is_ok()
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let admin = serve(gateway_api::app::router_with_studio_auth_and_litellm(
        store.clone(),
        RedisReadiness::new(&redis).unwrap(),
        None,
        GatewayAuthEnv::default(),
        auth,
        upstream.clone(),
        "profile-service-upstream".into(),
    ))
    .await;
    let client = reqwest::Client::new();
    let endpoint = format!("{admin}/admin-ui/admin/route-identities");
    let mut setting = json!({"route":"/v1/responses","access":{"authentication_profiles":{"revision":0,"profiles":[
        {"id":"employees","name":"Employee applications","enabled":true,"type":"entra_and_relayna_key","entra":{"audience":"api://employees","required_scopes":["responses.invoke"],"required_roles":["invoke"],"allowed_groups":["staff"],"allow_apigee":true}},
        {"id":"backend","name":"Backend services","enabled":true,"type":"entra_and_relayna_key","entra":{"audience":"api://backend","required_roles":["Backend.Invoke"]}},
        {"id":"automation","name":"Automation","enabled":true,"type":"relayna_key_only"}
    ],"bindings":[{"key_id":ids[0],"profile_id":"employees"},{"key_id":ids[1],"profile_id":"backend"},{"key_id":ids[2],"profile_id":"automation"}]}}});
    assert_eq!(
        client
            .put(&endpoint)
            .json(&setting)
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    let response = client
        .put(&endpoint)
        .bearer_auth(&operator.raw_token)
        .json(&setting)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200, "{}", response.text().await.unwrap());
    setting["access"]["authentication_profiles"]["revision"] = json!(1);
    let mut header = jsonwebtoken::Header::new(jsonwebtoken::Algorithm::RS256);
    header.kid = Some("profiles".into());
    let claims = |aud: &str, scope: &str, role: &str| json!({"iss":"https://issuer.test","tid":"tenant","aud":aud,"exp":Utc::now().timestamp()+3600,"iat":Utc::now().timestamp(),"ver":"2.0","oid":"employee-1","scp":scope,"roles":[role],"groups":["staff"]});
    let sign = |claims: &Value| jsonwebtoken::encode(&header, claims, &signing).unwrap();
    let employee = sign(&claims("api://employees", "responses.invoke", "invoke"));
    let backend = sign(&claims("api://backend", "", "Backend.Invoke"));
    let request = |path: &str, index: usize, jwt: Option<&str>| {
        let request = client
            .post(format!("http://127.0.0.1:{port}{path}"))
            .header("x-relayna-key", &keys[index])
            .header("x-relayna-admission-context", "forged")
            .header("x-authentication-profile", "automation")
            .json(&json!({"model":"test","input":"synthetic"}));
        if let Some(jwt) = jwt {
            request.bearer_auth(jwt)
        } else {
            request
        }
    };
    for mode in [
        OpenAiRouteMode::ManagedByGateway,
        OpenAiRouteMode::DirectLiteLlmPassthrough,
    ] {
        store
            .set_openai_route_mode("responses", mode)
            .await
            .unwrap();
        for path in ["/v1/responses", "/responses"] {
            let results = futures_util::future::join_all(
                [
                    (0, Some(employee.as_str())),
                    (1, Some(backend.as_str())),
                    (2, None),
                ]
                .map(|(index, jwt)| request(path, index, jwt).send()),
            )
            .await;
            for result in results {
                let response = result.unwrap();
                assert_eq!(response.status(), 200, "{}", response.text().await.unwrap());
                let body: Value = response.json().await.unwrap();
                assert_eq!(body["auth"], "Bearer profile-mapped-upstream");
                assert_eq!(body["key_leaked"], false);
                assert_eq!(body["admission_leaked"], false);
            }
            for (index, jwt, status) in [
                (0, None, 401),
                (0, Some(backend.as_str()), 401),
                (1, Some(employee.as_str()), 401),
                (2, Some(employee.as_str()), 401),
                (3, None, 403),
                (0, Some("invalid"), 401),
            ] {
                let response = request(path, index, jwt).send().await.unwrap();
                assert_eq!(
                    response.status(),
                    status,
                    "{path} {index}: {}",
                    response.text().await.unwrap()
                );
            }
        }
    }
    // Forwarding mode and selected identity do not bypass rate or budget limits.
    for mode in [
        OpenAiRouteMode::ManagedByGateway,
        OpenAiRouteMode::DirectLiteLlmPassthrough,
    ] {
        store
            .set_openai_route_mode("responses", mode)
            .await
            .unwrap();
        for (index, jwt) in [
            (0, Some(employee.as_str())),
            (1, Some(backend.as_str())),
            (2, None),
        ] {
            sqlx::query("UPDATE key_policies SET rpm_limit=0 WHERE key_id=$1")
                .bind(ids[index])
                .execute(store.pool())
                .await
                .unwrap();
            assert_eq!(
                request("/responses", index, jwt)
                    .send()
                    .await
                    .unwrap()
                    .status(),
                429
            );
            sqlx::query(
                "UPDATE key_policies SET rpm_limit=1000, daily_budget_usd=0 WHERE key_id=$1",
            )
            .bind(ids[index])
            .execute(store.pool())
            .await
            .unwrap();
            assert_eq!(
                request("/responses", index, jwt)
                    .send()
                    .await
                    .unwrap()
                    .status(),
                402
            );
            sqlx::query("UPDATE key_policies SET daily_budget_usd=NULL WHERE key_id=$1")
                .bind(ids[index])
                .execute(store.pool())
                .await
                .unwrap();
        }
    }
    // Bearer key is accepted only on its bound key-only profile; duplicate forms fail.
    assert_eq!(
        client
            .post(format!("http://127.0.0.1:{port}/responses"))
            .bearer_auth(&keys[2])
            .json(&json!({"model":"test"}))
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
    assert_eq!(
        request("/responses", 2, Some(&keys[2]))
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    assert_eq!(
        request("/responses", 2, None)
            .header("authorization", "Bearer native-litellm")
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    assert_eq!(
        client
            .post(format!("http://127.0.0.1:{port}/responses"))
            .bearer_auth("native-litellm")
            .json(&json!({}))
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    for (field, value, expected) in [
        ("scp", json!("wrong"), 403),
        ("roles", json!([]), 403),
        ("groups", json!([]), 403),
        ("tid", json!("other"), 401),
        ("iss", json!("https://wrong.test"), 401),
        ("exp", json!(Utc::now().timestamp() - 120), 401),
        ("nbf", json!(Utc::now().timestamp() + 120), 401),
    ] {
        let mut bad = claims("api://employees", "responses.invoke", "invoke");
        bad[field] = value;
        assert_eq!(
            request("/responses", 0, Some(&sign(&bad)))
                .send()
                .await
                .unwrap()
                .status(),
            expected,
            "{field}"
        );
    }
    let mut skew = claims("api://employees", "responses.invoke", "invoke");
    skew["exp"] = json!(Utc::now().timestamp() - 20);
    assert_eq!(
        request("/responses", 0, Some(&sign(&skew)))
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
    let apigee = ApigeeTrustedHeaderConfig {
        secret: "profile-apigee-test".into(),
        required_scope: None,
        required_role: None,
        allowed_groups: vec![],
    };
    for (offset, expected) in [(3600, 200), (-1, 401)] {
        let payload=json!({"audiences":["api://employees"],"expires_at":Utc::now().timestamp()+offset,"tenant_id":"tenant","object_id":"user","scopes":["responses.invoke"],"roles":["invoke"],"groups":["staff"],"token_version":"2.0","source":"apigee_trusted_header"}).to_string();
        let payload = URL_SAFE_NO_PAD.encode(payload.as_bytes());
        let signature = sign_apigee_trusted_identity(&payload, &apigee).unwrap();
        assert_eq!(
            request("/responses", 0, None)
                .header("x-apigee-entra-identity", &payload)
                .header("x-apigee-entra-signature", &signature)
                .send()
                .await
                .unwrap()
                .status(),
            expected
        );
        assert_eq!(
            request("/responses", 2, None)
                .header("x-apigee-entra-identity", &payload)
                .header("x-apigee-entra-signature", &signature)
                .send()
                .await
                .unwrap()
                .status(),
            401
        );
    }
    for (column, value) in [
        ("disabled", "true"),
        ("expires_at", "now()-interval '1 minute'"),
        ("revoked_at", "now()"),
    ] {
        sqlx::query(&format!("UPDATE api_keys SET {column}={value} WHERE id=$1"))
            .bind(ids[2])
            .execute(store.pool())
            .await
            .unwrap();
        assert_eq!(
            request("/responses", 2, None)
                .send()
                .await
                .unwrap()
                .status(),
            401
        );
        sqlx::query(&format!(
            "UPDATE api_keys SET {column}={} WHERE id=$1",
            if column == "disabled" {
                "false"
            } else {
                "NULL"
            }
        ))
        .bind(ids[2])
        .execute(store.pool())
        .await
        .unwrap();
    }
    // Complete-profile policy checks must not grant a denied route.
    sqlx::query(
        "UPDATE key_policies SET allowed_routes=ARRAY['/v1/chat/completions'] WHERE key_id=$1",
    )
    .bind(ids[2])
    .execute(store.pool())
    .await
    .unwrap();
    assert_eq!(
        request("/responses", 2, None)
            .send()
            .await
            .unwrap()
            .status(),
        403
    );
    sqlx::query("UPDATE key_policies SET allowed_routes=ARRAY['/v1/responses','/v1/messages','/litellm/*','/services/*'] WHERE key_id=$1").bind(ids[2]).execute(store.pool()).await.unwrap();
    let mut other = setting.clone();
    other["route"] = json!("/v1/messages");
    other["access"]["authentication_profiles"]["revision"] = json!(0);
    store
        .set_route_identity(serde_json::from_value(other).unwrap())
        .await
        .unwrap();
    store
        .set_anthropic_route_enabled("messages", true)
        .await
        .unwrap();
    let response = request("/v1/messages", 2, None).send().await.unwrap();
    assert_eq!(
        response.status(),
        200,
        "anthropic: {}",
        response.text().await.unwrap()
    );
    assert_eq!(
        request("/v1/messages", 0, None)
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    store.patch_litellm_passthrough_settings(serde_json::from_value(json!({"enabled":true,"ui_exposure":"trusted_ingress","allowed_paths":["/ui/*"],"allowed_methods":["GET"]})).unwrap()).await.unwrap();
    other = setting.clone();
    other["route"] = json!("/litellm/*");
    other["access"]["authentication_profiles"]["revision"] = json!(0);
    store
        .set_route_identity(serde_json::from_value(other).unwrap())
        .await
        .unwrap();
    for (key, jwt, expected) in [
        (None, None, 401),
        (Some(0), None, 401),
        (Some(0), Some(employee.as_str()), 200),
        (Some(2), None, 200),
        (Some(3), None, 403),
    ] {
        let mut call = client.get(format!("http://127.0.0.1:{port}/ui/"));
        if let Some(index) = key {
            call = call.header("x-relayna-key", &keys[index]);
        }
        if let Some(jwt) = jwt {
            call = call.bearer_auth(jwt);
        }
        let response = call.send().await.unwrap();
        assert_eq!(
            response.status(),
            expected,
            "trusted ingress: {}",
            response.text().await.unwrap()
        );
    }
    // Service CRUD uses the same atomic references and revision rules.
    let mut access = setting["access"].clone();
    access["authentication_profiles"]["revision"] = json!(0);
    let service_body = json!({"name":"profile-service","route_pattern":"/profile-service/*","upstream_base_url":upstream,"credential":"profile-service-secret","allowed_methods":["GET","POST"],"access":access});
    let service = store
        .create_service(serde_json::from_value(service_body.clone()).unwrap())
        .await
        .unwrap();
    for path in ["/profile-service/check", "/services/profile-service/check"] {
        assert_eq!(request(path, 2, None).send().await.unwrap().status(), 200);
        assert_eq!(request(path, 0, None).send().await.unwrap().status(), 401);
    }
    let mut invalid = service_body.clone();
    invalid["name"] = json!("invalid-profile-service");
    invalid["route_pattern"] = json!("/invalid-profile-service/*");
    invalid["access"]["authentication_profiles"]["bindings"][0]["key_id"] = json!(Uuid::new_v4());
    assert!(store
        .create_service(serde_json::from_value(invalid).unwrap())
        .await
        .is_err());
    let project = store
        .create_project(ProjectCreateRequest {
            name: "profile-ownership".into(),
        })
        .await
        .unwrap();
    let mut cross_project = service_body.clone();
    cross_project["name"] = json!("cross-project-service");
    cross_project["route_pattern"] = json!("/cross-project/*");
    cross_project["project_id"] = json!(project.id);
    assert_eq!(
        store
            .create_service(serde_json::from_value(cross_project).unwrap())
            .await
            .unwrap_err(),
        GatewayError::InvalidServicePayload
    );
    // Changing service ownership must recheck existing bindings, even if access is unchanged.
    assert_eq!(
        store
            .patch_service(
                "profile-service",
                serde_json::from_value(json!({"project_id":project.id})).unwrap()
            )
            .await
            .unwrap_err(),
        GatewayError::InvalidServicePayload
    );
    assert_eq!(
        store
            .get_service("profile-service")
            .await
            .unwrap()
            .unwrap()
            .project_id,
        None
    );
    // A key move must not retain a service profile from a different project.
    store
        .patch_admin_key(
            ids[2],
            serde_json::from_value(json!({"owner_type":"project","project_id":project.id}))
                .unwrap(),
        )
        .await
        .unwrap();
    let mut owned = service_body.clone();
    owned["name"] = json!("owned-profile-service");
    owned["route_pattern"] = json!("/owned-profile/*");
    owned["project_id"] = json!(project.id);
    owned["access"]["authentication_profiles"]["bindings"] =
        json!([{"key_id":ids[2],"profile_id":"automation"}]);
    let owned = store
        .create_service(serde_json::from_value(owned).unwrap())
        .await
        .unwrap();
    let other = store
        .create_project(ProjectCreateRequest {
            name: "other-profile-project".into(),
        })
        .await
        .unwrap();
    for project_id in [Some(other.id), None] {
        let error = store
            .patch_admin_key(
                ids[2],
                AdminKeyPatch {
                    owner_type: Some(if project_id.is_some() {
                        AdminKeyOwnerType::Project
                    } else {
                        AdminKeyOwnerType::Individual
                    }),
                    project_id: Some(project_id),
                    ..Default::default()
                },
            )
            .await
            .unwrap_err();
        assert_eq!(error, GatewayError::KeyProfileProjectConflict);
        assert_eq!(error.status_code(), http::StatusCode::CONFLICT);
        assert!(error
            .public_message()
            .contains("Remove those service profile assignments"));
    }
    // Old writers/direct SQL also cannot bypass the ownership guard.
    assert!(sqlx::query("UPDATE api_keys SET project_id=$2 WHERE id=$1")
        .bind(ids[2])
        .bind(other.id)
        .execute(store.pool())
        .await
        .is_err());
    // Unrelated edits and keeping the same project remain valid.
    store
        .patch_admin_key(
            ids[2],
            serde_json::from_value(json!({"name":"Renamed bound key","project_id":project.id}))
                .unwrap(),
        )
        .await
        .unwrap();
    let mut unbound = serde_json::to_value(owned.access).unwrap();
    unbound["authentication_profiles"]["bindings"] = json!([]);
    store
        .patch_service(
            "owned-profile-service",
            serde_json::from_value(json!({"access":unbound})).unwrap(),
        )
        .await
        .unwrap();
    store
        .patch_admin_key(
            ids[2],
            serde_json::from_value(json!({"project_id":other.id})).unwrap(),
        )
        .await
        .unwrap();
    // Concurrent binding creation holds the key row until commit. A waiting
    // project move must see the newly committed assignment and reject it.
    store
        .patch_admin_key(
            ids[2],
            serde_json::from_value(json!({"project_id":project.id})).unwrap(),
        )
        .await
        .unwrap();
    let current = store
        .get_service("owned-profile-service")
        .await
        .unwrap()
        .unwrap();
    let mut rebound = serde_json::to_value(current.access).unwrap();
    rebound["authentication_profiles"]["bindings"] =
        json!([{"key_id":ids[2],"profile_id":"automation"}]);
    let mut transaction = store.pool().begin().await.unwrap();
    sqlx::query("UPDATE service_registrations SET access=$1 WHERE name='owned-profile-service'")
        .bind(sqlx::types::Json(rebound))
        .execute(&mut *transaction)
        .await
        .unwrap();
    let moving = store.patch_admin_key(
        ids[2],
        serde_json::from_value(json!({"project_id":other.id})).unwrap(),
    );
    tokio::pin!(moving);
    assert!(
        tokio::time::timeout(Duration::from_millis(100), &mut moving)
            .await
            .is_err()
    );
    transaction.commit().await.unwrap();
    assert_eq!(
        moving.await.unwrap_err(),
        GatewayError::KeyProfileProjectConflict
    );
    let mut new_access = serde_json::to_value(service.access).unwrap();
    let stale = new_access.clone();
    new_access["authentication_profiles"]["profiles"][2]["name"] = json!("Renamed automation");
    let saved = store
        .patch_service(
            "profile-service",
            serde_json::from_value(json!({"access":new_access})).unwrap(),
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(saved.access.authentication_profiles.unwrap().revision, 2);
    assert_eq!(
        store
            .patch_service(
                "profile-service",
                serde_json::from_value(json!({"access":stale})).unwrap()
            )
            .await
            .unwrap_err(),
        GatewayError::AuthenticationProfileConflict
    );
    // Profile-specific database errors must not swallow existing uniqueness errors.
    let mut duplicate = service_body.clone();
    duplicate["access"] = json!({});
    assert_eq!(
        store
            .create_service(serde_json::from_value(duplicate).unwrap())
            .await
            .unwrap_err(),
        GatewayError::DuplicateService
    );
    let mut other = service_body.clone();
    other["name"] = json!("other-service");
    other["route_pattern"] = json!("/other-service/*");
    other["access"] = json!({});
    other["studio_service_id"] = json!("shared-studio-id");
    store
        .create_service(serde_json::from_value(other).unwrap())
        .await
        .unwrap();
    assert_eq!(
        store
            .patch_service(
                "profile-service",
                serde_json::from_value(json!({"studio_service_id":"shared-studio-id"})).unwrap()
            )
            .await
            .unwrap_err(),
        GatewayError::DuplicateService
    );
    // A profiles-only key cannot be selected by a forged hint on another route.
    assert_eq!(
        request("/v1/chat/completions", 2, None)
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    let mut bad = setting.clone();
    bad["access"]["authentication_profiles"]["bindings"][1]["key_id"] = json!(ids[0]);
    assert_eq!(
        client
            .put(&endpoint)
            .bearer_auth(&operator.raw_token)
            .json(&bad)
            .send()
            .await
            .unwrap()
            .status(),
        400
    );
    bad = setting.clone();
    bad["access"]["authentication_profiles"]["bindings"][0]["key_id"] = json!(Uuid::new_v4());
    assert_eq!(
        client
            .put(&endpoint)
            .bearer_auth(&operator.raw_token)
            .json(&bad)
            .send()
            .await
            .unwrap()
            .status(),
        400
    );
    bad = setting.clone();
    bad["access"]["authentication_profiles"]["revision"] = json!(0);
    assert_eq!(
        client
            .put(&endpoint)
            .bearer_auth(&operator.raw_token)
            .json(&bad)
            .send()
            .await
            .unwrap()
            .status(),
        409
    );
    // Legacy writers cannot erase opt-in enforcement, including raw old SQL writes.
    assert_eq!(
        client
            .put(&endpoint)
            .bearer_auth(&operator.raw_token)
            .json(&json!({"route":"/v1/responses","access":{}}))
            .send()
            .await
            .unwrap()
            .status(),
        409
    );
    assert!(sqlx::query(
        "UPDATE route_identity_settings SET access='{}' WHERE route='/v1/responses'"
    )
    .execute(store.pool())
    .await
    .is_err());
    setting["access"]["authentication_profiles"]["profiles"][0]["enabled"] = json!(false);
    let response = client
        .put(&endpoint)
        .bearer_auth(&operator.raw_token)
        .json(&setting)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    setting = response.json().await.unwrap();
    assert_eq!(setting["access"]["authentication_profiles"]["revision"], 2);
    assert_eq!(
        request("/responses", 0, Some(&employee))
            .send()
            .await
            .unwrap()
            .status(),
        403
    );
    // Stored corruption still fails closed even if validation is bypassed externally.
    sqlx::query(
        "ALTER TABLE route_identity_settings DISABLE TRIGGER route_authentication_profile_revision",
    )
    .execute(store.pool())
    .await
    .unwrap();
    bad = setting.clone();
    bad["access"]["authentication_profiles"]["bindings"][1]["key_id"] = json!(ids[0]);
    sqlx::query("UPDATE route_identity_settings SET access=$1 WHERE route='/v1/responses'")
        .bind(sqlx::types::Json(&bad["access"]))
        .execute(store.pool())
        .await
        .unwrap();
    assert_ne!(
        request("/responses", 0, Some(&employee))
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
    sqlx::query("UPDATE route_identity_settings SET access=$1 WHERE route='/v1/responses'")
        .bind(sqlx::types::Json(&setting["access"]))
        .execute(store.pool())
        .await
        .unwrap();
    sqlx::query(
        "ALTER TABLE route_identity_settings ENABLE TRIGGER route_authentication_profile_revision",
    )
    .execute(store.pool())
    .await
    .unwrap();
    // History retains the original selection even after a later profile rename.
    let mut recorded = 0i64;
    for _ in 0..40 {
        recorded=sqlx::query_scalar("SELECT count(*) FROM usage_events WHERE diagnostics->'authentication_profile'->>'id'='automation'").fetch_one(store.pool()).await.unwrap();
        if recorded > 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(recorded > 0);
    let old_name:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM request_traffic WHERE record->'diagnostics'->'authentication_profile'->>'name'='Automation')").fetch_one(store.pool()).await.unwrap();
    assert!(old_name);
    let audit: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM audit_events WHERE action='policies:route_identity_update')",
    )
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert!(audit);
    sqlx::query("UPDATE operator_tokens SET scopes=ARRAY['usage:read']")
        .execute(store.pool())
        .await
        .unwrap();
    assert_eq!(
        client
            .put(&endpoint)
            .bearer_auth(&operator.raw_token)
            .json(&setting)
            .send()
            .await
            .unwrap()
            .status(),
        403
    );
    assert_eq!(
        client
            .get(&endpoint)
            .bearer_auth(&operator.raw_token)
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
    let events = gateway_core::traffic::monitor().batch(None).rows;
    assert!(events.iter().any(
        |row| row
            .diagnostics
            .authentication_profile
            .as_ref()
            .is_some_and(
                |p| p.id.as_deref() == Some("automation") && p.outcome == "entra_not_required"
            )
    ));
    assert!(events.iter().any(|row| row
        .diagnostics
        .authentication_profile
        .as_ref()
        .is_some_and(|p| p.outcome == "identity_failed")));
    assert!(events.iter().any(|row| row
        .diagnostics
        .authentication_profile
        .as_ref()
        .is_some_and(|p| p.outcome == "selection_failed")));
    assert!(events.iter().any(|row| row
        .diagnostics
        .authentication_profile
        .as_ref()
        .is_some_and(|p| p.outcome == "credential_failed")));
    let snapshots = serde_json::to_string(&events).unwrap();
    for secret in keys.iter().chain([&employee, &backend]) {
        assert!(!snapshots.contains(secret));
    }
    store.pool().close().await;
    assert_eq!(
        request("/responses", 2, None)
            .send()
            .await
            .unwrap()
            .status(),
        502
    );
    assert_eq!(
        store
            .set_route_identity(serde_json::from_value(setting).unwrap())
            .await
            .unwrap_err(),
        GatewayError::StoreUnavailable
    );
    sqlx::query(&format!("DROP DATABASE {name} WITH (FORCE)"))
        .execute(&parent)
        .await
        .unwrap();
}
