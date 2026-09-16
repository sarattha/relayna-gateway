//! Real gateway + isolated PostgreSQL/Redis + mock BFF, adapters, Router and agent.
//! DATABASE_URL/REDIS_URL must point to disposable test services.
use axum::{
    extract::{
        ws::{Message as AxumMessage, WebSocket, WebSocketUpgrade},
        OriginalUri, State,
    },
    http::HeaderMap,
    response::IntoResponse,
    routing::{any, get},
    Json, Router,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use chrono::Utc;
use futures_util::{SinkExt, StreamExt};
use gateway_core::*;
use gateway_proxy::{PingoraLiteLlmConfig, RelaynaPingoraProxy};
use gateway_store::{PostgresStore, RedisControlState};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tokio::net::TcpListener;
use tokio_tungstenite::{
    connect_async,
    tungstenite::{client::IntoClientRequest, Message},
};
use uuid::Uuid;

async fn serve(app: Router) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{address}")
}
fn ws(url: &str) -> String {
    url.replacen("http:", "ws:", 1)
}
async fn bridge(mut downstream: WebSocket, request: http::Request<()>) {
    let (upstream, _) = match connect_async(request).await {
        Ok(value) => value,
        Err(error) => {
            eprintln!("mock bridge upstream failed: {error:?}");
            let _ = downstream.close().await;
            return;
        }
    };
    let (mut write, mut read) = upstream.split();
    loop {
        tokio::select! {
            message=downstream.recv()=>{match message {
                Some(Ok(AxumMessage::Text(text)))=>{if write.send(Message::Text(text.to_string().into())).await.is_err(){break}},
                Some(Ok(AxumMessage::Binary(bytes)))=>{if write.send(Message::Binary(bytes)).await.is_err(){break}},
                Some(Ok(AxumMessage::Ping(bytes)))=>{let _=write.send(Message::Ping(bytes)).await;},
                Some(Ok(AxumMessage::Pong(bytes)))=>{let _=write.send(Message::Pong(bytes)).await;},
                _=>break,
            }},
            message=read.next()=>{match message {
                Some(Ok(Message::Text(text)))=>{if downstream.send(AxumMessage::Text(text.to_string().into())).await.is_err(){break}},
                Some(Ok(Message::Binary(bytes)))=>{if downstream.send(AxumMessage::Binary(bytes)).await.is_err(){break}},
                Some(Ok(Message::Ping(bytes)))=>{let _=downstream.send(AxumMessage::Ping(bytes)).await;},
                Some(Ok(Message::Pong(bytes)))=>{let _=downstream.send(AxumMessage::Pong(bytes)).await;},
                _=>break,
            }}
        }
    }
    let _ = write.close().await;
    let _ = downstream.close().await;
}
#[derive(Clone)]
struct MockRouter {
    gateway: Arc<Mutex<String>>,
    agent: String,
    dispatches: Arc<AtomicUsize>,
    history: Arc<Mutex<HashMap<String, Vec<Value>>>>,
}
async fn router_socket(
    State(state): State<MockRouter>,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> impl IntoResponse {
    let context = headers["x-relayna-admission-context"]
        .to_str()
        .unwrap()
        .to_owned();
    upgrade.on_upgrade(move |mut socket|async move {
        while let Some(Ok(AxumMessage::Text(text)))=socket.recv().await {
            let command:Value=serde_json::from_str(&text).unwrap();
            let turn=command["turn_id"].as_str().unwrap_or("none").to_owned();
            if command["command"]=="ping" {socket.send(AxumMessage::Text(json!({"type":"pong"}).to_string().into())).await.unwrap();continue;}
            let events=if command["command"]=="resume" {
                state.history.lock().unwrap().get(&turn).cloned().unwrap_or_default()
            }else{
                let gateway=state.gateway.lock().unwrap().clone();
                let response=reqwest::Client::new().post(format!("{gateway}/internal/accessa/admissions/{turn}"))
                    .header("x-relayna-admission-context",&context).send().await.unwrap();
                let status=response.status();let result:Value=response.json().await.unwrap();
                if !status.is_success(){vec![json!({"type":"error","status":status.as_u16(),"detail":result})]}
                else{
                    state.dispatches.fetch_add(1,Ordering::SeqCst);
                    let (mut agent,_)=connect_async(format!("{}/chat",ws(&state.agent))).await.unwrap();
                    agent.send(Message::Text(command.to_string().into())).await.unwrap();
                    let mut events=vec![json!({"type":"accepted","turn_id":turn,"admission_id":result["admission_id"]})];
                    while let Some(Ok(Message::Text(text)))=agent.next().await{
                        let event:Value=serde_json::from_str(&text).unwrap();let done=event["type"]=="completed";
                        events.push(event);if done{break;}
                    }
                    state.history.lock().unwrap().insert(turn.clone(),events.clone());events
                }
            };
            for event in events {if socket.send(AxumMessage::Text(event.to_string().into())).await.is_err(){return;}}
        }
    })
}
#[derive(Clone)]
struct Adapter {
    router: String,
    observed: Arc<Mutex<Vec<Value>>>,
}
async fn adapter(
    State(state): State<Adapter>,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
    upgrade: Result<WebSocketUpgrade, axum::extract::ws::rejection::WebSocketUpgradeRejection>,
) -> axum::response::Response {
    state.observed.lock().unwrap().push(json!({"path":uri.path(),"oid":headers.get("x-caller-oid").and_then(|v|v.to_str().ok()),
        "channel":headers.get("x-route-channel").and_then(|v|v.to_str().ok()),"app":headers.get("x-route-app").and_then(|v|v.to_str().ok()),
        "key_leaked":headers.contains_key("x-relayna-key"),"user_token_leaked":headers.contains_key("x-access-token")}));
    if let Ok(upgrade) = upgrade {
        let mut request = format!("{}/run", ws(&state.router))
            .into_client_request()
            .unwrap();
        request.headers_mut().insert(
            "x-relayna-admission-context",
            headers["x-relayna-admission-context"].clone(),
        );
        upgrade
            .on_upgrade(move |socket| bridge(socket, request))
            .into_response()
    } else {
        Json(json!({"ok":true,"path":uri.path()})).into_response()
    }
}
#[derive(Clone)]
struct Bff {
    url: String,
    jwt: String,
    key: String,
}
async fn bff(State(state): State<Bff>, upgrade: WebSocketUpgrade) -> impl IntoResponse {
    let mut request = state.url.into_client_request().unwrap();
    request.headers_mut().insert(
        "authorization",
        format!("Bearer {}", state.jwt).parse().unwrap(),
    );
    request
        .headers_mut()
        .insert("x-relayna-key", state.key.parse().unwrap());
    request
        .headers_mut()
        .insert("x-caller-oid", "forged-user".parse().unwrap());
    upgrade.on_upgrade(move |socket| bridge(socket, request))
}
async fn next_json(
    socket: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
) -> Value {
    let message = tokio::time::timeout(Duration::from_secs(5), socket.next())
        .await
        .expect("frame timeout")
        .expect("socket ended")
        .expect("frame");
    serde_json::from_str(message.to_text().unwrap())
        .unwrap_or_else(|error| panic!("expected JSON frame, received {message:?}: {error}"))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn accessa_mock_chain_and_endpoint_regressions() {
    gateway_telemetry::init("error", false);
    let Ok(database) = std::env::var("DATABASE_URL") else {
        eprintln!("set DATABASE_URL and REDIS_URL for Accessa E2E");
        return;
    };
    let redis = std::env::var("REDIS_URL").expect("REDIS_URL");
    let store = PostgresStore::connect(&database).await.unwrap();
    let mut lock = store.pool().acquire().await.unwrap();
    sqlx::query("SELECT pg_advisory_lock(82120260808)")
        .execute(&mut *lock)
        .await
        .unwrap();
    let suffix = Uuid::new_v4().simple().to_string();
    let project = store
        .create_project(ProjectCreateRequest {
            name: format!("accessa-{suffix}"),
        })
        .await
        .unwrap();
    store
        .upsert_policy_layer(AdminPolicyLayerUpsert {
            kind: PolicyLayerKind::Project,
            scope_id: Some(project.id.to_string()),
            policy: gateway_core::admin::KeyPolicyPatch::default(),
            guardrail_policy: Default::default(),
        })
        .await
        .unwrap();
    let rsa = openssl::rsa::Rsa::generate(2048).unwrap();
    let signing =
        jsonwebtoken::EncodingKey::from_rsa_pem(&rsa.private_key_to_pem().unwrap()).unwrap();
    let jwks = json!({"keys":[{"kty":"RSA","kid":"accessa-test","alg":"RS256","n":URL_SAFE_NO_PAD.encode(rsa.n().to_vec()),"e":URL_SAFE_NO_PAD.encode(rsa.e().to_vec())}]});
    let oidc = serve(Router::new().route(
        "/discovery",
        get(|| async { Json(json!({"issuer":"https://issuer.test","jwks_uri":"placeholder"})) }),
    ))
    .await;
    // A separate discovery endpoint closes over its JWKS server URL.
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
            let url = format!("{key_server}/keys");
            async move { Json(json!({"issuer":"https://issuer.test","jwks_uri":url})) }
        }),
    ))
    .await;
    let mut header = jsonwebtoken::Header::new(jsonwebtoken::Algorithm::RS256);
    header.kid = Some("accessa-test".into());
    let sign = |aud: &str, scope: &str, exp: i64| {
        jsonwebtoken::encode(&header,&json!({"iss":"https://issuer.test","tid":"tenant","aud":aud,"exp":exp,"iat":Utc::now().timestamp(),"ver":"2.0","oid":"employee-1","scp":scope,"roles":["invoke"],"groups":["staff"]}),&signing).unwrap()
    };
    let expiry = Utc::now().timestamp() + 3600;
    let token = sign("api://accessa", "run", expiry);
    let agent = serve(Router::new().route(
        "/chat",
        get(|upgrade: WebSocketUpgrade| async {
            upgrade.on_upgrade(|mut socket| async move {
                if socket.recv().await.is_some() {
                    for event in [
                        json!({"type":"token","sequence":1,"text":"hello"}),
                        json!({"type":"completed","sequence":2}),
                    ] {
                        socket
                            .send(AxumMessage::Text(event.to_string().into()))
                            .await
                            .unwrap();
                    }
                }
            })
        }),
    ))
    .await;
    let state = MockRouter {
        gateway: Arc::new(Mutex::new(String::new())),
        agent,
        dispatches: Arc::new(AtomicUsize::new(0)),
        history: Arc::new(Mutex::new(HashMap::new())),
    };
    let router = serve(
        Router::new()
            .route("/run", get(router_socket))
            .with_state(state.clone()),
    )
    .await;
    let observed = Arc::new(Mutex::new(vec![]));
    let adapters = [
        serve(Router::new().fallback(any(adapter)).with_state(Adapter {
            router: router.clone(),
            observed: observed.clone(),
        }))
        .await,
        serve(Router::new().fallback(any(adapter)).with_state(Adapter {
            router,
            observed: observed.clone(),
        }))
        .await,
    ];
    let mut service_names = vec![];
    let mut keys = vec![];
    let mut key_ids = vec![];
    let mut channels = vec![];
    for (index, upstream) in adapters.iter().enumerate() {
        let channel = format!("web{index}-{suffix}");
        let name = format!("accessa-{index}-{suffix}");
        let request:ServiceCreateRequest=serde_json::from_value(json!({"name":name,"project_id":project.id,"route_pattern":format!("/app/tara/channel/{channel}/v1/*"),"upstream_base_url":upstream,"credential":"mock-adapter-only","allowed_methods":["GET","POST"],"timeout_ms":1000,"max_body_bytes":512,
            "access":{"entra":{"audience":"api://accessa","required_scopes":["run"],"required_roles":["invoke"],"allowed_groups":["staff"]},"accessa":{"app":"tara","channel":channel,"idle_timeout_ms":1500,"max_connections":2,"max_connections_per_key":1,"max_frame_bytes":4096}}})).unwrap();
        let response = store.create_service(request).await.unwrap();
        assert_eq!(response.access.entra.unwrap().audience, "api://accessa");
        let material = VirtualKeyMaterial::generate().unwrap();
        let created=store.create_admin_key(serde_json::from_value(json!({"owner_type":"project","project_id":project.id,"service_names":[name],"policy":{"allowed_routes":["/services/*"],"allowed_providers":["internal-service"],"allowed_services":[name],"allow_streaming":true,"rpm_limit":100,"monthly_budget_usd":5}})).unwrap(),&material).await.unwrap();
        key_ids.push(created.id);
        keys.push(material.raw_key);
        service_names.push(name);
        channels.push(channel);
    }
    let entra = EntraAuthConfig {
        tenant_id: "tenant".into(),
        audience: "api://internal".into(),
        issuer: "https://issuer.test".into(),
        oidc_discovery_url: format!("{discovery}/discovery"),
        required_scope: Some("internal".into()),
        required_role: None,
        allowed_groups: vec![],
        accepted_algorithms: vec!["RS256".into()],
        relayna_key_header: "x-relayna-key".into(),
        jwks_cache_ttl_seconds: 300,
        clock_skew_seconds: 0,
    };
    let auth = SharedGatewayAuthRuntime::new(GatewayAuthRuntimeConfig {
        unverified_bearer_enabled: false,
        relayna_key_header: "x-relayna-key".into(),
        entra_auth: Some(entra),
        apigee_trusted_header: Some(ApigeeTrustedHeaderConfig {
            secret: "apigee-test-only".into(),
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
        PingoraLiteLlmConfig::from_base_url(&oidc, "unused")
            .unwrap()
            .with_auth_runtime(auth),
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
    tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .expect("gateway listener ready");
    let gateway = format!("http://127.0.0.1:{port}");
    *state.gateway.lock().unwrap() = gateway.clone();
    let client = reqwest::Client::new();
    let run_path = format!("/app/tara/channel/{}/v1/run", channels[0]);
    let direct_request = |jwt: &str, key: &str, path: &str| {
        let mut r = format!("{}{path}", ws(&gateway))
            .into_client_request()
            .unwrap();
        r.headers_mut()
            .insert("authorization", format!("Bearer {jwt}").parse().unwrap());
        r.headers_mut()
            .insert("x-relayna-key", key.parse().unwrap());
        r
    };
    for (jwt, key, path) in [
        (
            sign("api://internal", "internal", expiry),
            keys[0].clone(),
            run_path.clone(),
        ),
        (
            sign("api://accessa", "wrong", expiry),
            keys[0].clone(),
            run_path.clone(),
        ),
        (
            sign("api://accessa", "run", Utc::now().timestamp() - 1),
            keys[0].clone(),
            run_path.clone(),
        ),
        (token.clone(), keys[1].clone(), run_path.clone()),
        (
            token.clone(),
            keys[0].clone(),
            run_path.replace("/run", "/documents"),
        ),
    ] {
        let error = connect_async(direct_request(&jwt, &key, &path))
            .await
            .unwrap_err();
        assert!(
            matches!(error, tokio_tungstenite::tungstenite::Error::Http(ref response) if response.status().is_client_error()),
            "expected gateway rejection, got {error:?}"
        );
    }
    let bff_url = serve(Router::new().route("/socket", get(bff)).with_state(Bff {
        url: format!("{}{run_path}", ws(&gateway)),
        jwt: token.clone(),
        key: keys[0].clone(),
    }))
    .await;
    let (mut socket, _) = connect_async(format!("{}/socket", ws(&bff_url)))
        .await
        .unwrap();
    let turn = Uuid::new_v4();
    socket
        .send(Message::Text(
            json!({"command":"run","turn_id":turn}).to_string().into(),
        ))
        .await
        .unwrap();
    assert_eq!(next_json(&mut socket).await["type"], "accepted");
    assert_eq!(next_json(&mut socket).await["text"], "hello");
    assert_eq!(next_json(&mut socket).await["type"], "completed");
    assert_eq!(state.dispatches.load(Ordering::SeqCst), 1);
    assert!(
        connect_async(direct_request(&token, &keys[0], &run_path))
            .await
            .is_err(),
        "per-key socket cap"
    );
    let (mut second, _) = connect_async(direct_request(
        &token,
        &keys[1],
        &format!("/app/tara/channel/{}/v1/run", channels[1]),
    ))
    .await
    .unwrap();
    second.close(None).await.unwrap();
    // Keep a run channel alive beyond the HTTP timeout with application heartbeat traffic.
    for _ in 0..4 {
        tokio::time::sleep(Duration::from_millis(400)).await;
        socket
            .send(Message::Text(json!({"command":"ping"}).to_string().into()))
            .await
            .unwrap();
        assert_eq!(next_json(&mut socket).await["type"], "pong");
    }
    sqlx::query("UPDATE api_keys SET disabled=true WHERE id=$1")
        .bind(key_ids[0])
        .execute(store.pool())
        .await
        .unwrap();
    socket
        .send(Message::Text(
            json!({"command":"run","turn_id":Uuid::new_v4()})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
    assert_eq!(next_json(&mut socket).await["type"], "error");
    assert_eq!(state.dispatches.load(Ordering::SeqCst), 1);
    sqlx::query("UPDATE api_keys SET disabled=false WHERE id=$1")
        .bind(key_ids[0])
        .execute(store.pool())
        .await
        .unwrap();
    let control = RedisControlState::new(&redis).unwrap();
    control
        .seed_budget_counters(key_ids[0], 0.0, 6.0, Utc::now())
        .await
        .unwrap();
    socket
        .send(Message::Text(
            json!({"command":"run","turn_id":Uuid::new_v4()})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
    assert_eq!(next_json(&mut socket).await["status"], 402);
    assert_eq!(state.dispatches.load(Ordering::SeqCst), 1);
    control
        .seed_budget_counters(key_ids[0], 0.0, 0.0, Utc::now())
        .await
        .unwrap();
    socket.close(None).await.unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    let (mut socket, _) = connect_async(format!("{}/socket", ws(&bff_url)))
        .await
        .unwrap();
    socket
        .send(Message::Text(
            json!({"command":"resume","turn_id":turn})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
    for expected in ["accepted", "token", "completed"] {
        assert_eq!(next_json(&mut socket).await["type"], expected);
    }
    assert_eq!(
        state.dispatches.load(Ordering::SeqCst),
        1,
        "resume never re-executes"
    );
    socket.close(None).await.unwrap();
    let response = client
        .get(format!(
            "{gateway}/app/tara/channel/{}/v1/conversations",
            channels[0]
        ))
        .bearer_auth(&token)
        .header("x-relayna-key", &keys[0])
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(
        response.json::<Value>().await.unwrap()["path"],
        format!("/app/tara/channel/{}/v1/conversations", channels[0])
    );
    for request in observed.lock().unwrap().iter() {
        assert_eq!(request["oid"], "employee-1");
        assert_eq!(request["key_leaked"], false);
        assert_eq!(request["user_token_leaked"], false);
    }
    assert_eq!(
        client
            .post(format!(
                "{gateway}/internal/accessa/admissions/{}",
                Uuid::new_v4()
            ))
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    // The same gateway selects a different audience for an ordinary Apigee service.
    store.patch_service(&service_names[0], serde_json::from_value(json!({
        "route_pattern":format!("/internal-test/{suffix}/*"),
        "access":{"entra":{"audience":"api://internal-service","required_scopes":["internal.invoke"],"allow_apigee":true}}
    })).unwrap()).await.unwrap();
    let internal_url = format!("{gateway}/internal-test/{suffix}/check");
    assert_eq!(
        client
            .get(&internal_url)
            .bearer_auth(&token)
            .header("x-relayna-key", &keys[0])
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    let apigee = ApigeeTrustedHeaderConfig {
        secret: "apigee-test-only".into(),
        required_scope: None,
        required_role: None,
        allowed_groups: vec![],
    };
    let identity = json!({"audiences":["api://internal-service"],"expires_at":expiry,"tenant_id":"tenant","object_id":"employee-1","scopes":["internal.invoke"],"roles":[],"groups":[],"token_version":"2.0","source":"apigee_trusted_header"});
    for (field, value, expected) in [
        ("audiences", json!(["api://accessa"]), 401),
        ("expires_at", Value::Null, 401),
        ("scopes", json!(["run"]), 403),
        ("audiences", json!(["api://internal-service"]), 200),
    ] {
        let mut claims = identity.clone();
        claims[field] = value;
        let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).unwrap());
        let signature = sign_apigee_trusted_identity(&payload, &apigee).unwrap();
        assert_eq!(
            client
                .get(&internal_url)
                .header("x-relayna-key", &keys[0])
                .header("x-apigee-entra-identity", payload)
                .header("x-apigee-entra-signature", signature)
                .send()
                .await
                .unwrap()
                .status()
                .as_u16(),
            expected
        );
    }

    // A service explicitly skips Entra while the same process still protects Accessa.
    store
        .patch_service(
            &service_names[0],
            serde_json::from_value(json!({"access":{"skip_entra":true}})).unwrap(),
        )
        .await
        .unwrap();
    for url in [
        &internal_url,
        &format!("{gateway}/services/{}/check", service_names[0]),
    ] {
        for dedicated in [true, false] {
            let request = client.get(url);
            let request = if dedicated {
                request.header("x-relayna-key", &keys[0])
            } else {
                request.bearer_auth(&keys[0])
            };
            assert_eq!(request.send().await.unwrap().status(), 200);
        }
        assert_eq!(client.get(url).send().await.unwrap().status(), 401);
        assert_eq!(
            client
                .get(url)
                .bearer_auth("rk_live_test_key")
                .send()
                .await
                .unwrap()
                .status(),
            401
        );
    }
    assert_eq!(
        client
            .get(format!(
                "{gateway}/app/tara/channel/{}/v1/documents",
                channels[1]
            ))
            .header("x-relayna-key", &keys[1])
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    store
        .patch_service(
            &service_names[0],
            serde_json::from_value(json!({"access":{}})).unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        client
            .get(&internal_url)
            .header("x-relayna-key", &keys[0])
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    println!("Accessa E2E: two adapters, BFF, Router, agent, Entra isolation, heartbeat, limits, revocation, budget and replay passed");
}
