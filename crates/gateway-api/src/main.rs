use anyhow::Context;
use gateway_api::{app, config::Config};
use gateway_proxy::{LiteLlmConfig, LiteLlmProxy};
use gateway_store::{PostgresStore, RedisReadiness};
use std::time::Duration;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Config::from_env().context("load gateway configuration")?;
    gateway_telemetry::init(&config.log_level);

    let store = PostgresStore::connect(&config.database_url)
        .await
        .context("connect postgres")?;
    let redis = RedisReadiness::new(&config.redis_url).context("create redis client")?;
    let proxy = LiteLlmProxy::new(LiteLlmConfig::new(
        &config.litellm_base_url,
        &config.litellm_service_key,
        Duration::from_secs(30),
    )?)
    .context("create litellm proxy")?;

    let app = app::router(store, redis, proxy);
    let listener = TcpListener::bind(config.gateway_bind_addr)
        .await
        .context("bind gateway listener")?;

    tracing::info!(addr = %listener.local_addr()?, "gateway listening");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("serve gateway")?;

    Ok(())
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
