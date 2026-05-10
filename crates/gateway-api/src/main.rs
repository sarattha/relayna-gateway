use anyhow::Context;
use gateway_api::{app, config::Config};
use gateway_core::{OperatorTokenMaterial, OperatorTokenStore};
use gateway_proxy::{PingoraLiteLlmConfig, PingoraUpstreamConfig, RelaynaPingoraProxy};
use gateway_store::{PostgresStore, RedisControlState, RedisReadiness};
use pingora_core::server::Server;
use std::{sync::Arc, thread};
use tokio::net::TcpListener;

fn main() -> anyhow::Result<()> {
    let config = Config::from_env().context("load gateway configuration")?;
    gateway_telemetry::init(&config.log_level);

    let setup_runtime = tokio::runtime::Runtime::new().context("create setup runtime")?;
    let store = setup_runtime
        .block_on(PostgresStore::connect(&config.database_url))
        .context("connect postgres")?;
    if let Some(material) = setup_runtime
        .block_on(bootstrap_operator_token(&store))
        .context("bootstrap operator token")?
    {
        tracing::warn!(
            "generated first Relayna Gateway operator token; store it securely because it will not be shown again"
        );
        println!("Relayna Gateway operator token: {}", material.raw_token);
    }
    let redis = RedisReadiness::new(&config.redis_url).context("create redis client")?;
    let redis_control =
        RedisControlState::new(&config.redis_url).context("create redis control client")?;
    let mut proxy_config =
        PingoraLiteLlmConfig::from_base_url(&config.litellm_base_url, &config.litellm_service_key)
            .context("create pingora LiteLLM proxy config")?;
    if let (Some(base_url), Some(service_key)) = (
        config.direct_openai_base_url.as_deref(),
        config.direct_openai_service_key.as_deref(),
    ) {
        proxy_config = proxy_config.with_direct_openai(Some(
            PingoraUpstreamConfig::from_base_url(base_url, service_key)
                .context("create direct OpenAI-compatible upstream config")?,
        ));
    }
    proxy_config = proxy_config.with_worker_token(config.relayna_worker_token.clone());

    let app = app::router(store.clone(), redis);
    let control_bind_addr = config.gateway_control_bind_addr;
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
            if let Err(error) = axum::serve(listener, app)
                .with_graceful_shutdown(shutdown_signal())
                .await
            {
                tracing::error!(%error, "gateway control API stopped with error");
            }
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

async fn bootstrap_operator_token(
    store: &PostgresStore,
) -> anyhow::Result<Option<OperatorTokenMaterial>> {
    let material = OperatorTokenMaterial::generate().context("generate operator token")?;
    match store
        .bootstrap_operator_token(&material)
        .await
        .context("store bootstrap operator token")?
    {
        Some(_) => Ok(Some(material)),
        None => Ok(None),
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
