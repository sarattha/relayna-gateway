use gateway_core::{GatewayError, GatewayResult};
use std::{env, net::SocketAddr};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub database_url: String,
    pub redis_url: String,
    pub litellm_base_url: String,
    pub litellm_service_key: String,
    pub direct_openai_base_url: Option<String>,
    pub direct_openai_service_key: Option<String>,
    pub relayna_worker_token: Option<String>,
    pub relayna_studio_base_url: Option<String>,
    pub relayna_studio_token: Option<String>,
    pub gateway_bind_addr: SocketAddr,
    pub gateway_control_bind_addr: SocketAddr,
    pub log_level: String,
}

impl Config {
    pub fn from_env() -> GatewayResult<Self> {
        let database_url = required("DATABASE_URL")?;
        let redis_url = required("REDIS_URL")?;
        let litellm_base_url = required("LITELLM_BASE_URL")?;
        let litellm_service_key = required("LITELLM_SERVICE_KEY")?;
        let direct_openai_base_url = optional("DIRECT_OPENAI_BASE_URL");
        let direct_openai_service_key = optional("DIRECT_OPENAI_SERVICE_KEY");
        let relayna_worker_token = optional("RELAYNA_WORKER_TOKEN");
        let relayna_studio_base_url = optional("RELAYNA_STUDIO_BASE_URL");
        let relayna_studio_token = optional("RELAYNA_STUDIO_TOKEN");
        let gateway_bind_addr = required("GATEWAY_BIND_ADDR")?
            .parse()
            .map_err(|_| GatewayError::InvalidConfiguration)?;
        let gateway_control_bind_addr = required("GATEWAY_CONTROL_BIND_ADDR")?
            .parse()
            .map_err(|_| GatewayError::InvalidConfiguration)?;
        let log_level = required("LOG_LEVEL")?;

        Ok(Self {
            database_url,
            redis_url,
            litellm_base_url,
            litellm_service_key,
            direct_openai_base_url,
            direct_openai_service_key,
            relayna_worker_token,
            relayna_studio_base_url,
            relayna_studio_token,
            gateway_bind_addr,
            gateway_control_bind_addr,
            log_level,
        })
    }
}

fn required(name: &str) -> GatewayResult<String> {
    env::var(name)
        .map(|value| value.trim().to_owned())
        .ok()
        .filter(|value| !value.is_empty())
        .ok_or(GatewayError::InvalidConfiguration)
}

fn optional(name: &str) -> Option<String> {
    env::var(name)
        .map(|value| value.trim().to_owned())
        .ok()
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_missing_config() {
        assert_eq!(
            required("REL_AYNA_GATEWAY_DOES_NOT_EXIST").unwrap_err(),
            GatewayError::InvalidConfiguration
        );
    }
}
