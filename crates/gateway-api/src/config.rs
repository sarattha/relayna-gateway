use gateway_core::{
    validate_relayna_key_header_name, ApigeeTrustedHeaderConfig, EntraAuthConfig, GatewayError,
    GatewayResult, ENTRA_DEFAULT_RELAYNA_KEY_HEADER,
};
use std::{env, net::SocketAddr};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub database_url: String,
    pub redis_url: String,
    pub litellm_base_url: String,
    pub litellm_service_key: String,
    pub gateway_admin_token: Option<String>,
    pub direct_openai_base_url: Option<String>,
    pub direct_openai_service_key: Option<String>,
    pub relayna_worker_token: Option<String>,
    pub relayna_studio_base_url: Option<String>,
    pub relayna_studio_token: Option<String>,
    pub guardrail_pii_mapping_ttl_seconds: u64,
    pub guardrail_mapping_encryption_key: Option<String>,
    pub relayna_key_header: String,
    pub entra_auth: Option<EntraAuthConfig>,
    pub apigee_trusted_header: Option<ApigeeTrustedHeaderConfig>,
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
        let gateway_admin_token = optional("GATEWAY_ADMIN_TOKEN");
        let direct_openai_base_url = optional("DIRECT_OPENAI_BASE_URL");
        let direct_openai_service_key = optional("DIRECT_OPENAI_SERVICE_KEY");
        let relayna_worker_token = optional("RELAYNA_WORKER_TOKEN");
        let relayna_studio_base_url = optional("RELAYNA_STUDIO_BASE_URL");
        let relayna_studio_token = optional("RELAYNA_STUDIO_TOKEN");
        let guardrail_pii_mapping_ttl_seconds = optional("GUARDRAIL_PII_MAPPING_TTL_SECONDS")
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(3600);
        let guardrail_mapping_encryption_key = optional("GUARDRAIL_MAPPING_ENCRYPTION_KEY");
        let relayna_key_header = optional("ENTRA_RELAYNA_KEY_HEADER")
            .unwrap_or_else(|| ENTRA_DEFAULT_RELAYNA_KEY_HEADER.to_owned());
        validate_relayna_key_header_name(&relayna_key_header)?;
        let entra_auth = if optional_bool("ENTRA_AUTH_ENABLED")?.unwrap_or(false) {
            let config = EntraAuthConfig {
                tenant_id: required("ENTRA_TENANT_ID")?,
                audience: required("ENTRA_AUDIENCE")?,
                issuer: required("ENTRA_ISSUER")?,
                oidc_discovery_url: required("ENTRA_OIDC_DISCOVERY_URL")?,
                required_scope: optional("ENTRA_REQUIRED_SCOPE"),
                required_role: optional("ENTRA_REQUIRED_ROLE"),
                allowed_groups: optional_csv("ENTRA_ALLOWED_GROUPS"),
                accepted_algorithms: with_default(
                    optional_csv("ENTRA_ACCEPTED_ALGORITHMS"),
                    vec!["RS256".to_owned()],
                ),
                relayna_key_header: relayna_key_header.clone(),
                jwks_cache_ttl_seconds: optional_u64("ENTRA_JWKS_CACHE_TTL_SECONDS").unwrap_or(300),
                clock_skew_seconds: optional_i64("ENTRA_CLOCK_SKEW_SECONDS").unwrap_or(60),
            };
            config.validate()?;
            Some(config)
        } else {
            None
        };
        let apigee_trusted_header =
            if optional_bool("APIGEE_TRUSTED_HEADER_ENABLED")?.unwrap_or(false) {
                let config = ApigeeTrustedHeaderConfig {
                    secret: required("APIGEE_TRUSTED_HEADER_SECRET")?,
                };
                config.validate()?;
                Some(config)
            } else {
                None
            };
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
            gateway_admin_token,
            direct_openai_base_url,
            direct_openai_service_key,
            relayna_worker_token,
            relayna_studio_base_url,
            relayna_studio_token,
            guardrail_pii_mapping_ttl_seconds,
            guardrail_mapping_encryption_key,
            relayna_key_header,
            entra_auth,
            apigee_trusted_header,
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

fn optional_csv(name: &str) -> Vec<String> {
    optional(name)
        .map(|value| {
            value
                .split(',')
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn with_default(values: Vec<String>, default: Vec<String>) -> Vec<String> {
    if values.is_empty() {
        default
    } else {
        values
    }
}

fn optional_bool(name: &str) -> GatewayResult<Option<bool>> {
    optional(name)
        .map(|value| match value.to_ascii_lowercase().as_str() {
            "true" | "1" | "yes" => Ok(true),
            "false" | "0" | "no" => Ok(false),
            _ => Err(GatewayError::InvalidConfiguration),
        })
        .transpose()
}

fn optional_u64(name: &str) -> Option<u64> {
    optional(name).and_then(|value| value.parse::<u64>().ok())
}

fn optional_i64(name: &str) -> Option<i64> {
    optional(name).and_then(|value| value.parse::<i64>().ok())
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
