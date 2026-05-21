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

    let studio = config.relayna_studio_base_url.clone().map(|base_url| {
        app::StudioCatalogClient::new(base_url, config.relayna_studio_token.clone())
    });
    let app = app::router_with_studio(store.clone(), redis, studio);
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
}
