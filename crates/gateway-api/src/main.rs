use anyhow::Context;
use gateway_api::portal::PortalOidcRuntime;
use gateway_api::{app, config::Config};
use gateway_core::{
    AdminGatewayAuthSettingsStore, EffectiveGatewayAuthSettings, EntraJwtVerifier,
    OperatorTokenMaterial, OperatorTokenStore, SharedGatewayAuthRuntime,
};
use gateway_proxy::{PingoraLiteLlmConfig, PingoraUpstreamConfig, RelaynaPingoraProxy};
use gateway_store::{PostgresStore, RedisControlState, RedisReadiness};
use pingora_core::server::Server;
use std::{sync::Arc, thread};
use tokio::net::TcpListener;
use tokio::time::{self, Duration};

fn main() -> anyhow::Result<()> {
    let config = Config::from_env().context("load gateway configuration")?;
    gateway_telemetry::init(&config.log_level, config.entra_auth_debug);
    if config.entra_auth_debug {
        gateway_telemetry::authorization_debug(
            "configuration",
            "startup",
            "enabled",
            "operator_enabled_sensitive_diagnostics",
            None,
            serde_json::json!({
                "warning": "decoded Entra claims and authorization decisions may contain sensitive identity data",
                "compact_credentials_logged": false,
            }),
        );
    }

    let setup_runtime = tokio::runtime::Runtime::new().context("create setup runtime")?;
    let store = setup_runtime
        .block_on(PostgresStore::connect(&config.database_url))
        .context("connect postgres")?;
    if setup_runtime
        .block_on(store.has_active_operator_token())
        .context("check active operator token")?
    {
        if config.gateway_admin_token.is_some() {
            tracing::info!(
                "active Relayna Gateway operator token already exists; ignoring GATEWAY_ADMIN_TOKEN because token rotation owns changes after bootstrap"
            );
        }
    } else if let Some(bootstrap) = setup_runtime
        .block_on(bootstrap_operator_token(
            &store,
            config.gateway_admin_token.as_deref(),
        ))
        .context("bootstrap operator token")?
    {
        match bootstrap {
            BootstrapOperatorToken::Configured(_) => {
                tracing::warn!(
                    "stored first Relayna Gateway operator token from GATEWAY_ADMIN_TOKEN; future env changes are ignored after bootstrap"
                );
            }
            BootstrapOperatorToken::Generated(material) => {
                tracing::warn!(
                    "generated first Relayna Gateway operator token; store it securely because it will not be shown again"
                );
                println!("Relayna Gateway operator token: {}", material.raw_token);
            }
        }
    }
    let redis = RedisReadiness::new(&config.redis_url).context("create redis client")?;
    let redis_control =
        RedisControlState::new(&config.redis_url).context("create redis control client")?;
    setup_runtime
        .block_on(redis.ready())
        .context("check redis readiness before budget rehydration")?;
    setup_runtime
        .block_on(rehydrate_budget_counters(&store, &redis_control))
        .context("rehydrate budget counters")?;
    let auth_env = config.gateway_auth_env();
    let effective_auth = setup_runtime
        .block_on(store.gateway_auth_settings())
        .context("load persisted gateway auth settings")
        .and_then(|stored| {
            EffectiveGatewayAuthSettings::from_sources(stored, &auth_env)
                .context("resolve gateway auth settings")
        })?;
    let shared_auth = SharedGatewayAuthRuntime::new(effective_auth.runtime_config())
        .context("create shared gateway auth runtime")?;
    let mut proxy_config =
        PingoraLiteLlmConfig::from_base_url(&config.litellm_base_url, &config.litellm_service_key)
            .context("create pingora LiteLLM proxy config")?
            .with_relayna_key_header(effective_auth.relayna_key_header.clone())
            .context("configure Relayna key header")?;
    if let (Some(base_url), Some(service_key)) = (
        config.direct_openai_base_url.as_deref(),
        config.direct_openai_service_key.as_deref(),
    ) {
        proxy_config = proxy_config.with_direct_openai(Some(
            PingoraUpstreamConfig::from_base_url(base_url, service_key)
                .context("create direct OpenAI-compatible upstream config")?,
        ));
    }
    proxy_config = proxy_config
        .with_worker_token(config.relayna_worker_token.clone())
        .with_entra_auth(effective_auth.entra_auth.clone())
        .with_apigee_trusted_header(effective_auth.apigee_trusted_header.clone())
        .with_auth_runtime(shared_auth.clone())
        .with_body_admission_limits(
            config.gateway_max_buffered_requests,
            config.gateway_max_inflight_buffer_bytes,
        )
        .context("configure body admission limits")?;

    let studio = config.relayna_studio_base_url.clone().map(|base_url| {
        app::StudioCatalogClient::new(base_url, config.relayna_studio_token.clone())
    });
    let portal_oidc = config
        .portal_oidc
        .clone()
        .map(PortalOidcRuntime::new)
        .transpose()
        .context("create portal OIDC runtime")?
        .map(Arc::new);
    let owner_entra_verifier = config
        .owner_entra_auth
        .clone()
        .map(EntraJwtVerifier::new)
        .transpose()
        .context("create owner Entra verifier")?
        .map(Arc::new);
    let app = app::router_with_identity(
        store.clone(),
        redis,
        studio,
        auth_env,
        shared_auth,
        config.litellm_base_url.clone(),
        config.litellm_service_key.clone(),
        portal_oidc,
        owner_entra_verifier,
    );
    let control_bind_addr = config.gateway_control_bind_addr;
    let reconciler_store = store.clone();
    let reconciler_redis = redis_control.clone();
    thread::spawn(move || {
        let runtime = match tokio::runtime::Runtime::new() {
            Ok(runtime) => runtime,
            Err(error) => {
                tracing::error!(%error, "failed to create gateway control runtime");
                return;
            }
        };
        runtime.block_on(async move {
            let listener = match TcpListener::bind(control_bind_addr).await {
                Ok(listener) => listener,
                Err(error) => {
                    tracing::error!(%error, addr = %control_bind_addr, "failed to bind gateway control listener");
                    return;
                }
            };
            tracing::info!(addr = %listener.local_addr().unwrap_or(control_bind_addr), "gateway control API listening");
            let budget_reconciler = tokio::spawn(run_budget_counter_reconciler(
                reconciler_store,
                reconciler_redis,
            ));
            if let Err(error) = axum::serve(listener, app)
                .with_graceful_shutdown(shutdown_signal())
                .await
            {
                tracing::error!(%error, "gateway control API stopped with error");
            }
            budget_reconciler.abort();
        });
    });

    let mut pingora = Server::new(None).context("create pingora server")?;
    pingora.bootstrap();
    let proxy = RelaynaPingoraProxy::new(Arc::new(store), Arc::new(redis_control), proxy_config);
    let mut proxy_service = pingora_proxy::http_proxy_service(&pingora.configuration, proxy);
    proxy_service.add_tcp(&config.gateway_bind_addr.to_string());
    tracing::info!(addr = %config.gateway_bind_addr, "gateway Pingora proxy listening");
    pingora.add_service(proxy_service);
    pingora.run_forever()
}

async fn rehydrate_budget_counters(
    store: &PostgresStore,
    redis: &RedisControlState,
) -> anyhow::Result<usize> {
    let now = chrono::Utc::now();
    let seeds = store
        .budget_counter_seeds(now)
        .await
        .context("load budget counter seeds from postgres")?;
    for seed in &seeds {
        redis
            .seed_budget_counters(
                seed.key_id,
                seed.daily_spend_usd,
                seed.monthly_spend_usd,
                now,
            )
            .await
            .context("seed redis budget counters")?;
    }
    tracing::info!(seeded_keys = seeds.len(), "rehydrated budget counters");
    Ok(seeds.len())
}

async fn run_budget_counter_reconciler(store: PostgresStore, redis: RedisControlState) {
    let mut interval = time::interval(Duration::from_secs(300));
    loop {
        interval.tick().await;
        match rehydrate_budget_counters(&store, &redis).await {
            Ok(seeded_keys) => {
                tracing::debug!(seeded_keys, "reconciled budget counters");
            }
            Err(error) => {
                tracing::warn!(%error, "budget counter reconciliation failed");
            }
        }
    }
}

async fn bootstrap_operator_token(
    store: &PostgresStore,
    configured_token: Option<&str>,
) -> anyhow::Result<Option<BootstrapOperatorToken>> {
    let bootstrap = bootstrap_operator_token_material(configured_token)?;
    match store
        .bootstrap_operator_token(bootstrap.material())
        .await
        .context("store bootstrap operator token")?
    {
        Some(_) => Ok(Some(bootstrap)),
        None => Ok(None),
    }
}

enum BootstrapOperatorToken {
    Configured(OperatorTokenMaterial),
    Generated(OperatorTokenMaterial),
}

impl BootstrapOperatorToken {
    fn material(&self) -> &OperatorTokenMaterial {
        match self {
            Self::Configured(material) | Self::Generated(material) => material,
        }
    }
}

fn bootstrap_operator_token_material(
    configured_token: Option<&str>,
) -> anyhow::Result<BootstrapOperatorToken> {
    match configured_token {
        Some(raw_token) => OperatorTokenMaterial::from_raw(raw_token.to_owned())
            .map(BootstrapOperatorToken::Configured)
            .context("parse GATEWAY_ADMIN_TOKEN"),
        None => OperatorTokenMaterial::generate()
            .map(BootstrapOperatorToken::Generated)
            .context("generate operator token"),
    }
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("install Ctrl-C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("install terminate handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::{TcpListener as StdTcpListener, TcpStream},
        time::Instant,
    };

    fn unused_local_address() -> String {
        let listener = StdTcpListener::bind("127.0.0.1:0").expect("reserve local address");
        listener.local_addr().expect("local address").to_string()
    }

    fn wait_for_http(address: &str, path: &str) -> String {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            if let Ok(mut stream) = TcpStream::connect(address) {
                stream
                    .write_all(
                        format!(
                            "GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"
                        )
                        .as_bytes(),
                    )
                    .expect("write HTTP request");
                let mut response = String::new();
                stream
                    .read_to_string(&mut response)
                    .expect("read HTTP response");
                return response;
            }
            assert!(
                Instant::now() < deadline,
                "gateway did not start at {address}"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    #[test]
    fn bootstrap_material_uses_configured_admin_token() {
        let raw_token = "op_live_1234567890abcdef1234567890abcdef";
        let bootstrap = bootstrap_operator_token_material(Some(raw_token)).unwrap();

        match bootstrap {
            BootstrapOperatorToken::Configured(material) => {
                assert_eq!(material.raw_token, raw_token);
                assert_eq!(material.token_prefix, "op_live_12345678");
            }
            BootstrapOperatorToken::Generated(_) => panic!("expected configured token"),
        }
    }

    #[test]
    fn bootstrap_material_rejects_malformed_configured_admin_token() {
        assert!(bootstrap_operator_token_material(Some("test-admin-token")).is_err());
    }

    #[test]
    fn bootstrap_material_generates_when_no_admin_token_is_configured() {
        let bootstrap = bootstrap_operator_token_material(None).unwrap();

        match bootstrap {
            BootstrapOperatorToken::Generated(material) => {
                assert!(material.raw_token.starts_with("op_live_"));
            }
            BootstrapOperatorToken::Configured(_) => panic!("expected generated token"),
        }
    }

    #[test]
    fn gateway_entrypoint_boots_control_and_proxy_planes_with_live_dependencies() {
        let database_url = match std::env::var("DATABASE_URL") {
            Ok(value) => value,
            Err(_) => {
                eprintln!("skipping gateway entrypoint coverage: DATABASE_URL is not set");
                return;
            }
        };
        let redis_url = match std::env::var("REDIS_URL") {
            Ok(value) => value,
            Err(_) => {
                eprintln!("skipping gateway entrypoint coverage: REDIS_URL is not set");
                return;
            }
        };
        let proxy_address = unused_local_address();
        let control_address = unused_local_address();
        for (name, value) in [
            ("DATABASE_URL", database_url),
            ("REDIS_URL", redis_url),
            ("LITELLM_BASE_URL", "http://127.0.0.1:9".to_owned()),
            ("LITELLM_SERVICE_KEY", "coverage-key".to_owned()),
            ("GATEWAY_BIND_ADDR", proxy_address.clone()),
            ("GATEWAY_CONTROL_BIND_ADDR", control_address.clone()),
            ("LOG_LEVEL", "gateway_api=error".to_owned()),
            ("DIRECT_OPENAI_BASE_URL", "http://127.0.0.1:9".to_owned()),
            (
                "DIRECT_OPENAI_SERVICE_KEY",
                "coverage-openai-key".to_owned(),
            ),
            ("RELAYNA_WORKER_TOKEN", "coverage-worker".to_owned()),
            ("RELAYNA_STUDIO_BASE_URL", "http://127.0.0.1:9".to_owned()),
            ("RELAYNA_STUDIO_TOKEN", "coverage-studio".to_owned()),
        ] {
            std::env::set_var(name, value);
        }
        std::env::set_var("ENTRA_AUTH_ENABLED", "false");
        std::env::set_var("APIGEE_TRUSTED_HEADER_ENABLED", "false");

        std::thread::spawn(|| main().expect("gateway entrypoint"));

        let control = wait_for_http(&control_address, "/admin-ui/readyz");
        assert!(control.starts_with("HTTP/1.1 200"), "{control}");
        let proxy = wait_for_http(&proxy_address, "/v1/models");
        assert!(proxy.starts_with("HTTP/1.1 401"), "{proxy}");
    }
}
