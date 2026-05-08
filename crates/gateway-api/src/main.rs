use anyhow::Context;
use gateway_api::{app, config::Config};
use gateway_proxy::{PingoraLiteLlmConfig, RelaynaPingoraProxy};
use gateway_store::{PostgresStore, RedisControlState, RedisReadiness};
use pingora_core::server::Server;
use std::sync::Arc;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Config::from_env().context("load gateway configuration")?;
    gateway_telemetry::init(&config.log_level);

    let store = PostgresStore::connect(&config.database_url)
        .await
        .context("connect postgres")?;
    let redis = RedisReadiness::new(&config.redis_url).context("create redis client")?;
    let redis_control =
        RedisControlState::new(&config.redis_url).context("create redis control client")?;
    let proxy_config =
        PingoraLiteLlmConfig::from_base_url(&config.litellm_base_url, &config.litellm_service_key)
            .context("create pingora LiteLLM proxy config")?;

    let app = app::router(store.clone(), redis, config.gateway_admin_token.clone());
    let listener = TcpListener::bind(config.gateway_control_bind_addr)
        .await
        .context("bind gateway control listener")?;

    tracing::info!(addr = %listener.local_addr()?, "gateway control API listening");
    tokio::spawn(async move {
        if let Err(error) = axum::serve(listener, app)
            .with_graceful_shutdown(shutdown_signal())
            .await
        {
            tracing::error!(%error, "gateway control API stopped with error");
        }
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
