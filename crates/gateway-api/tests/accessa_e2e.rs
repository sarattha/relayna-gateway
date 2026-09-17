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
type AgentHistory = Arc<Mutex<HashMap<String, Vec<String>>>>;

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
                    let mut events=vec![json!({"type":"accepted","turn_id":turn,"conversation_id":command["conversation_id"],"admission_id":result["admission_id"]})];
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
        "admission_present":headers.contains_key("x-relayna-admission-context"),"key_leaked":headers.contains_key("x-relayna-key"),"user_token_leaked":headers.contains_key("x-access-token")}));
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

// Assert the complete event boundary for each turn, including correlation and
// context from previous turns. The agent is deterministic; no live LLM is used.
async fn chat_turn(
    socket: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    conversation: &str,
    messages: &[&str],
) -> Vec<Value> {
    let turn = Uuid::new_v4().to_string();
    socket
        .send(Message::Text(
            json!({"command":"run","turn_id":turn,
        "conversation_id":conversation,"message":messages.last().unwrap()})
            .to_string()
            .into(),
        ))
        .await
        .unwrap();
    let mut events = Vec::new();
    for kind in ["accepted", "token", "token", "completed"] {
        let event = next_json(socket).await;
        assert_eq!(event["type"], kind);
        assert_eq!(event["turn_id"], turn);
        assert_eq!(event["conversation_id"], conversation);
        events.push(event);
    }
    for (sequence, event) in events[1..].iter().enumerate() {
        assert_eq!(event["sequence"], sequence + 1);
    }
    assert_eq!(
        events[1]["prior_messages"],
        json!(&messages[..messages.len() - 1])
    );
    assert_eq!(
        events[1]["text"],
        messages[..messages.len() - 1].join(" | ")
    );
    assert_eq!(events[2]["text"], *messages.last().unwrap());
    events
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
    // Conversation memory belongs to the agent fixture, not the gateway. Every
    // dispatch opens a new Router -> agent socket, so state must survive sockets.
    let agent_history = Arc::new(Mutex::new(HashMap::<String, Vec<String>>::new()));
    let agent = serve(
        Router::new()
            .route(
                "/chat",
                get(
                    |State(history): State<AgentHistory>, upgrade: WebSocketUpgrade| async move {
                        upgrade.on_upgrade(move |mut socket| async move {
                    if let Some(Ok(AxumMessage::Text(text))) = socket.recv().await {
                        let command: Value = serde_json::from_str(&text).unwrap();
                        let conversation = command["conversation_id"].as_str().unwrap();
                        let message = command["message"].as_str().unwrap();
                        let previous = {
                            let mut history = history.lock().unwrap();
                            let messages = history.entry(conversation.to_owned()).or_default();
                            let previous = messages.clone();
                            messages.push(message.to_owned());
                            previous
                        };
                        for event in [
                            json!({"type":"token","sequence":1,"text":previous.join(" | "),
                                "prior_messages":previous,"turn_id":command["turn_id"],
                                "conversation_id":conversation}),
                            json!({"type":"token","sequence":2,"text":message,
                                "turn_id":command["turn_id"],"conversation_id":conversation}),
                            json!({"type":"completed","sequence":3,"turn_id":command["turn_id"],
                                "conversation_id":conversation}),
                        ] {
                            socket.send(AxumMessage::Text(event.to_string().into())).await.unwrap();
                        }
                    }
                })
                    },
                ),
            )
            .with_state(agent_history.clone()),
    )
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
            "access":{"entra":{"audience":"api://accessa","required_scopes":["run"],"required_roles":["invoke"],"allowed_groups":["staff"]},"accessa":{"app":"tara","channel":channel,"idle_timeout_ms":10000,"max_connections":2,"max_connections_per_key":1,"max_frame_bytes":4096}}})).unwrap();
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
    let failed_identities: Vec<_> = gateway_core::traffic::monitor()
        .batch(None)
        .rows
        .into_iter()
        .filter_map(|row| row.diagnostics.entra)
        .filter(|identity| identity.verification == "failed")
        .collect();
    assert!(!failed_identities.is_empty());
    for identity in failed_identities {
        assert_eq!(identity.expected_audience.as_deref(), Some("api://accessa"));
        assert!(
            identity.audiences.is_empty()
                && identity.scopes.is_empty()
                && identity.roles.is_empty()
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
    let conversation = Uuid::new_v4().to_string();
    let messages = [
        "My name is Mali. Help me plan a trip to Chiang Mai.",
        "Make it three days, please.",
        "I prefer vegetarian food. Include places to eat.",
        "Change the second day to an indoor activity if it rains.",
        "Summarize my destination, duration, food preference and revised plan.",
        "I am back. Remind me of the preferences I gave you before reconnecting.",
    ];
    let mut transcript = Vec::new();
    let mut admission_ids = std::collections::HashSet::new();
    // Allow slow CI admission/Argon2 work within the 10-second socket idle limit.
    // Five successful conversational turns on exactly the same BFF socket.
    for index in 0..5 {
        let events = chat_turn(&mut socket, &conversation, &messages[..=index]).await;
        assert!(admission_ids.insert(events[0]["admission_id"].as_str().unwrap().to_owned()));
        transcript.push(events);
        assert_eq!(state.dispatches.load(Ordering::SeqCst), index + 1);
        assert_eq!(
            agent_history.lock().unwrap()[&conversation],
            messages[..=index]
        );
    }
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
    let live = gateway_core::traffic::monitor()
        .batch(None)
        .rows
        .into_iter()
        .find(|row| {
            row.key_id == Some(key_ids[0])
                && !row.completed
                && row
                    .diagnostics
                    .websocket
                    .as_ref()
                    .is_some_and(|socket| socket.state == "open")
        })
        .expect("live websocket diagnostic sample");
    let socket_sample = live.diagnostics.websocket.as_ref().unwrap();
    assert!(socket_sample.client_bytes > 0 && socket_sample.upstream_bytes > 0);
    assert!(socket_sample.client_frames >= 9 && socket_sample.upstream_frames >= 9);
    assert!(socket_sample.duration_ms >= 1500);
    assert_eq!(socket_sample.idle_timeout_ms, 10000);
    assert!(socket_sample.session_expires_at.is_some());
    let verified = live.diagnostics.entra.as_ref().unwrap();
    assert_eq!(verified.verification, "verified");
    assert_eq!(verified.expected_audience.as_deref(), Some("api://accessa"));
    assert_eq!(verified.audiences, ["api://accessa"]);
    assert_eq!(verified.scopes, ["run"]);
    assert_eq!(verified.roles, ["invoke"]);
    assert_eq!(verified.groups, ["staff"]);
    assert_eq!(verified.required_scopes, ["run"]);
    let safe = serde_json::to_string(&live).unwrap();
    for secret in [&token, &keys[0], messages[0]] {
        assert!(!safe.contains(secret));
    }
    sqlx::query("UPDATE api_keys SET disabled=true WHERE id=$1")
        .bind(key_ids[0])
        .execute(store.pool())
        .await
        .unwrap();
    socket
        .send(Message::Text(
            json!({"command":"run","turn_id":Uuid::new_v4(),
                "conversation_id":conversation,"message":"Please continue my itinerary."})
            .to_string()
            .into(),
        ))
        .await
        .unwrap();
    assert_eq!(next_json(&mut socket).await["type"], "error");
    assert_eq!(state.dispatches.load(Ordering::SeqCst), 5);
    assert_eq!(agent_history.lock().unwrap()[&conversation], messages[..5]);
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
            json!({"command":"run","turn_id":Uuid::new_v4(),
                "conversation_id":conversation,"message":"Please continue my itinerary."})
            .to_string()
            .into(),
        ))
        .await
        .unwrap();
    assert_eq!(next_json(&mut socket).await["status"], 402);
    assert_eq!(state.dispatches.load(Ordering::SeqCst), 5);
    assert_eq!(agent_history.lock().unwrap()[&conversation], messages[..5]);
    control
        .seed_budget_counters(key_ids[0], 0.0, 0.0, Utc::now())
        .await
        .unwrap();
    socket.close(None).await.unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    let mut terminal = None;
    for _ in 0..100 {
        terminal = sqlx::query_scalar::<_, Value>(
            "SELECT diagnostics FROM usage_events WHERE diagnostics->>'traffic_id' = $1",
        )
        .bind(live.id.to_string())
        .fetch_optional(store.pool())
        .await
        .unwrap();
        if terminal.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let terminal: gateway_core::traffic::RequestDiagnostics =
        serde_json::from_value(terminal.expect("persisted socket usage")).unwrap();
    let closed = terminal.websocket.unwrap();
    assert_eq!(closed.state, "closed");
    assert!(closed.closed_at.is_some());
    assert!(closed.client_bytes >= socket_sample.client_bytes);
    assert!(closed.upstream_bytes >= socket_sample.upstream_bytes);
    assert!(closed.duration_ms >= socket_sample.duration_ms);
    assert_eq!(terminal.entra.unwrap(), *verified);
    let (mut socket, _) = connect_async(format!("{}/socket", ws(&bff_url)))
        .await
        .unwrap();
    // Replay every completed turn after reconnect, preserving exact event IDs
    // and ordering without dispatching to the agent or modifying its memory.
    for events in &transcript {
        socket
            .send(Message::Text(
                json!({"command":"resume",
            "turn_id":events[0]["turn_id"],"conversation_id":conversation})
                .to_string()
                .into(),
            ))
            .await
            .unwrap();
        for expected in events {
            assert_eq!(next_json(&mut socket).await, *expected);
        }
    }
    assert_eq!(
        state.dispatches.load(Ordering::SeqCst),
        5,
        "resume never re-executes"
    );
    assert_eq!(agent_history.lock().unwrap()[&conversation], messages[..5]);
    // Continue the same conversation on the new socket with all prior context.
    let events = chat_turn(&mut socket, &conversation, &messages).await;
    assert!(admission_ids.insert(events[0]["admission_id"].as_str().unwrap().to_owned()));
    assert_eq!(state.dispatches.load(Ordering::SeqCst), 6);
    assert_eq!(agent_history.lock().unwrap()[&conversation], messages);
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
        assert_eq!(request["admission_present"], true);
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
                .header("x-relayna-admission-context", "spoofed-context")
                .send()
                .await
                .unwrap()
                .status()
                .as_u16(),
            expected
        );
    }

    assert_eq!(
        observed.lock().unwrap().last().unwrap()["admission_present"],
        false
    );

    let signed_identity = gateway_core::traffic::monitor()
        .batch(None)
        .rows
        .into_iter()
        .filter_map(|row| row.diagnostics.entra)
        .find(|identity| {
            identity.source.as_deref() == Some("signed_apigee")
                && identity.verification == "verified"
        })
        .expect("signed Apigee identity diagnostic");
    assert_eq!(
        signed_identity.expected_audience.as_deref(),
        Some("api://internal-service")
    );
    assert_eq!(signed_identity.audiences, ["api://internal-service"]);
    assert_eq!(signed_identity.scopes, ["internal.invoke"]);
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
