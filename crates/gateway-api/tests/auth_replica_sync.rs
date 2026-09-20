//! Two real gateway processes share only PostgreSQL/Redis, never their runtime Arcs.
use axum::{
    routing::{get, post},
    Json, Router,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use gateway_core::*;
use gateway_store::PostgresStore;
use serde_json::{json, Value};
use std::{
    process::{Child, Command, Stdio},
    time::Duration,
};
use uuid::Uuid;

struct GatewayProcess(Child);
impl Drop for GatewayProcess {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
struct TestDatabase {
    url: String,
    name: String,
}
impl Drop for TestDatabase {
    fn drop(&mut self) {
        let url = self.url.clone();
        let name = self.name.clone();
        // Cleanup also runs on assertion failure after the gateway child is killed.
        let _ = std::thread::spawn(move || {
            let runtime = tokio::runtime::Runtime::new().unwrap();
            runtime.block_on(async {
                if let Ok(pool) = sqlx::PgPool::connect(&url).await {
                    let _ = sqlx::query(&format!("DROP DATABASE {name} WITH (FORCE)"))
                        .execute(&pool)
                        .await;
                }
            });
        })
        .join();
    }
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

async fn serve(router: Router) -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    (
        format!("http://{address}"),
        tokio::spawn(async move { axum::serve(listener, router).await.unwrap() }),
    )
}
async fn patch(client: &reqwest::Client, control: &str, operator: &str, body: Value) {
    let response = client
        .patch(format!("{control}/admin-ui/admin/auth/front-door"))
        .bearer_auth(operator)
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200, "{}", response.text().await.unwrap());
}
async fn status(client: &reqwest::Client, proxy: &str, bearer: &str, key: Option<&str>) -> u16 {
    let mut request = client
        .post(format!("{proxy}/v1/chat/completions"))
        .bearer_auth(bearer)
        .json(&json!({"model":"test","messages":[]}));
    if let Some(key) = key {
        request = request.header("x-relayna-key", key);
    }
    request.send().await.unwrap().status().as_u16()
}
async fn eventually(
    client: &reqwest::Client,
    proxy: &str,
    bearer: &str,
    key: Option<&str>,
    expected: u16,
) {
    tokio::time::timeout(Duration::from_secs(12), async {
        loop {
            if status(client, proxy, bearer, key).await == expected {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("replica did not converge without restart");
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn admin_changes_reach_other_gateway_processes_without_rollout() {
    let (Ok(database), Ok(redis)) = (std::env::var("DATABASE_URL"), std::env::var("REDIS_URL"))
    else {
        return;
    };
    let parent = sqlx::PgPool::connect(&database).await.unwrap();
    let name = format!("auth_sync_{}", Uuid::new_v4().simple());
    sqlx::query(&format!("CREATE DATABASE {name}"))
        .execute(&parent)
        .await
        .unwrap();
    let _cleanup = TestDatabase {
        url: database.clone(),
        name: name.clone(),
    };
    let mut database_url = url::Url::parse(&database).unwrap();
    database_url.set_path(&name);
    let store = PostgresStore::connect(database_url.as_str()).await.unwrap();
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
    let key = VirtualKeyMaterial::generate().unwrap();
    store.create_admin_key(serde_json::from_value(json!({"owner_type":"individual","policy":{"allowed_routes":["/v1/chat/completions"],"rpm_limit":1000}})).unwrap(), &key).await.unwrap();
    let rsa = openssl::rsa::Rsa::generate(2048).unwrap();
    let signing =
        jsonwebtoken::EncodingKey::from_rsa_pem(&rsa.private_key_to_pem().unwrap()).unwrap();
    let jwks = json!({"keys":[{"kty":"RSA","kid":"sync","alg":"RS256","n":URL_SAFE_NO_PAD.encode(rsa.n().to_vec()),"e":URL_SAFE_NO_PAD.encode(rsa.e().to_vec())}]});
    let (keys_url, keys_task) = serve(Router::new().route(
        "/keys",
        get(move || {
            let keys = jwks.clone();
            async move { Json(keys) }
        }),
    ))
    .await;
    let (discovery, discovery_task) = serve(Router::new().route(
        "/discovery",
        get(move || {
            let keys = keys_url.clone();
            async move {
                Json(json!({"issuer":"https://issuer.test","jwks_uri":format!("{keys}/keys")}))
            }
        }),
    ))
    .await;
    let (upstream, upstream_task) = serve(Router::new().route(
        "/v1/chat/completions",
        post(|| async {
            Json(json!({"choices":[],"usage":{"prompt_tokens":1,"completion_tokens":1}}))
        }),
    ))
    .await;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
        .unwrap();
    let mut processes = Vec::new();
    let mut endpoints = Vec::new();
    for _ in 0..2 {
        let proxy_port = free_port();
        let control_port = free_port();
        let process = GatewayProcess(
            Command::new(env!("CARGO_BIN_EXE_gateway-api"))
                .env("DATABASE_URL", database_url.as_str())
                .env("REDIS_URL", &redis)
                .env("GATEWAY_BIND_ADDR", format!("127.0.0.1:{proxy_port}"))
                .env(
                    "GATEWAY_CONTROL_BIND_ADDR",
                    format!("127.0.0.1:{control_port}"),
                )
                .env("LITELLM_BASE_URL", &upstream)
                .env("LITELLM_SERVICE_KEY", "replica-test-upstream")
                .env("ENTRA_AUTH_ENABLED", "false")
                .env("APIGEE_TRUSTED_HEADER_ENABLED", "false")
                .env("GATEWAY_UNVERIFIED_BEARER_ENABLED", "false")
                .env("LOG_LEVEL", "error")
                .stdout(Stdio::null())
                .stderr(Stdio::inherit())
                .spawn()
                .unwrap(),
        );
        processes.push(process);
        let control = format!("http://127.0.0.1:{control_port}");
        let proxy = format!("http://127.0.0.1:{proxy_port}");
        tokio::time::timeout(Duration::from_secs(15), async {
            loop {
                if client
                    .get(format!("{control}/admin-ui/readyz"))
                    .send()
                    .await
                    .is_ok_and(|r| r.status().is_success())
                    && tokio::net::TcpStream::connect(("127.0.0.1", proxy_port))
                        .await
                        .is_ok()
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(status(&client, &proxy, &key.raw_key, None).await, 200);
        endpoints.push((control, proxy));
    }
    let mut header = jsonwebtoken::Header::new(jsonwebtoken::Algorithm::RS256);
    header.kid = Some("sync".into());
    let token = |aud: &str| {
        jsonwebtoken::encode(&header,&json!({"iss":"https://issuer.test","tid":"tenant","aud":aud,"exp":chrono::Utc::now().timestamp()+3600,"iat":chrono::Utc::now().timestamp(),"ver":"2.0","oid":"sync-user"}),&signing).unwrap()
    };
    let first = token("api://first");
    let second = token("api://second");
    patch(&client,&endpoints[0].0,&operator.raw_token,json!({"entra_enabled":true,"tenant_id":"tenant","audience":"api://first","issuer":"https://issuer.test","oidc_discovery_url":format!("{discovery}/discovery"),"relayna_key_header":"x-relayna-key"})).await;
    assert_eq!(
        status(&client, &endpoints[0].1, &first, Some(&key.raw_key)).await,
        200
    );
    eventually(&client, &endpoints[1].1, &first, Some(&key.raw_key), 200).await;
    for (_, proxy) in &endpoints {
        assert_eq!(
            status(&client, proxy, "not-a-jwt", Some(&key.raw_key)).await,
            401
        );
    }
    // Reverse the writer to prove every replica reconciles, not only a leader.
    patch(
        &client,
        &endpoints[1].0,
        &operator.raw_token,
        json!({"audience":"api://second"}),
    )
    .await;
    eventually(&client, &endpoints[0].1, &second, Some(&key.raw_key), 200).await;
    for (_, proxy) in &endpoints {
        assert_eq!(
            status(&client, proxy, &first, Some(&key.raw_key)).await,
            401
        );
    }
    patch(
        &client,
        &endpoints[0].0,
        &operator.raw_token,
        json!({"entra_enabled":false}),
    )
    .await;
    eventually(&client, &endpoints[1].1, &key.raw_key, None, 200).await;
    for process in &mut processes {
        assert!(
            process.0.try_wait().unwrap().is_none(),
            "gateway restarted or exited"
        );
    }
    // Exercise signal shutdown and let instrumented child processes flush their
    // coverage, rather than using the failure-cleanup SIGKILL path on success.
    #[cfg(unix)]
    for process in &mut processes {
        assert!(Command::new("kill")
            .args(["-INT", &process.0.id().to_string()])
            .status()
            .unwrap()
            .success());
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                if let Some(status) = process.0.try_wait().unwrap() {
                    assert!(status.success());
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
    }
    drop(processes);
    keys_task.abort();
    discovery_task.abort();
    upstream_task.abort();
}
