use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{DateTime, Utc};
use gateway_core::{
    EntraAuthConfig, EntraIdentityContext, EntraJwtVerifier, GatewayError, GatewayResult,
    ENTRA_DEFAULT_RELAYNA_KEY_HEADER,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{sync::Arc, time::Duration};
use url::Url;
use uuid::Uuid;

const OIDC_HTTP_TIMEOUT: Duration = Duration::from_secs(8);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortalOidcConfig {
    pub tenant_id: String,
    pub client_id: String,
    pub client_secret: String,
    pub issuer: String,
    pub discovery_url: String,
    pub redirect_uri: String,
    pub post_logout_redirect_uri: String,
    pub session_ttl_seconds: i64,
    pub login_ttl_seconds: i64,
    pub cookie_secure: bool,
}

impl PortalOidcConfig {
    pub fn validate(&self) -> GatewayResult<()> {
        if [
            &self.tenant_id,
            &self.client_id,
            &self.client_secret,
            &self.issuer,
            &self.discovery_url,
            &self.redirect_uri,
            &self.post_logout_redirect_uri,
        ]
        .iter()
        .any(|value| value.trim().is_empty())
            || self.session_ttl_seconds <= 0
            || self.login_ttl_seconds <= 0
            || Url::parse(&self.discovery_url).is_err()
            || Url::parse(&self.redirect_uri).is_err()
            || Url::parse(&self.post_logout_redirect_uri).is_err()
        {
            return Err(GatewayError::InvalidConfiguration);
        }
        Ok(())
    }
}

#[derive(Debug)]
pub struct PortalOidcRuntime {
    pub config: PortalOidcConfig,
    verifier: Arc<EntraJwtVerifier>,
    client: reqwest::Client,
}

impl PortalOidcRuntime {
    pub fn new(config: PortalOidcConfig) -> GatewayResult<Self> {
        config.validate()?;
        let verifier = EntraJwtVerifier::new(EntraAuthConfig {
            tenant_id: config.tenant_id.clone(),
            audience: config.client_id.clone(),
            issuer: config.issuer.clone(),
            oidc_discovery_url: config.discovery_url.clone(),
            required_scope: None,
            required_role: None,
            allowed_groups: Vec::new(),
            accepted_algorithms: vec!["RS256".to_owned()],
            relayna_key_header: ENTRA_DEFAULT_RELAYNA_KEY_HEADER.to_owned(),
            jwks_cache_ttl_seconds: 300,
            clock_skew_seconds: 60,
        })?;
        let client = reqwest::Client::builder()
            .timeout(OIDC_HTTP_TIMEOUT)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| GatewayError::InvalidConfiguration)?;
        Ok(Self {
            config,
            verifier: Arc::new(verifier),
            client,
        })
    }

    pub async fn authorization_url(
        &self,
        state: &str,
        nonce: &str,
        pkce_challenge: &str,
    ) -> GatewayResult<String> {
        let metadata = self.metadata().await?;
        if metadata.issuer != self.config.issuer {
            return Err(GatewayError::OidcUnavailable);
        }
        let mut url = Url::parse(&metadata.authorization_endpoint)
            .map_err(|_| GatewayError::OidcUnavailable)?;
        url.query_pairs_mut()
            .append_pair("client_id", &self.config.client_id)
            .append_pair("response_type", "code")
            .append_pair("redirect_uri", &self.config.redirect_uri)
            .append_pair("response_mode", "query")
            .append_pair("scope", "openid profile email")
            .append_pair("state", state)
            .append_pair("nonce", nonce)
            .append_pair("code_challenge", pkce_challenge)
            .append_pair("code_challenge_method", "S256");
        Ok(url.into())
    }

    pub async fn exchange_code(
        &self,
        code: &str,
        pkce_verifier: &str,
        now: DateTime<Utc>,
    ) -> GatewayResult<EntraIdentityContext> {
        let metadata = self.metadata().await?;
        if metadata.issuer != self.config.issuer {
            return Err(GatewayError::OidcUnavailable);
        }
        let response = self
            .client
            .post(metadata.token_endpoint)
            .form(&[
                ("grant_type", "authorization_code"),
                ("client_id", self.config.client_id.as_str()),
                ("client_secret", self.config.client_secret.as_str()),
                ("redirect_uri", self.config.redirect_uri.as_str()),
                ("code", code),
                ("code_verifier", pkce_verifier),
            ])
            .send()
            .await
            .map_err(|_| GatewayError::OidcUnavailable)?
            .error_for_status()
            .map_err(|_| GatewayError::InvalidOidcTransaction)?
            .json::<OidcTokenResponse>()
            .await
            .map_err(|_| GatewayError::OidcUnavailable)?;
        self.verifier.verify_token(&response.id_token, now).await
    }

    async fn metadata(&self) -> GatewayResult<OidcMetadata> {
        self.client
            .get(&self.config.discovery_url)
            .send()
            .await
            .map_err(|_| GatewayError::OidcUnavailable)?
            .error_for_status()
            .map_err(|_| GatewayError::OidcUnavailable)?
            .json::<OidcMetadata>()
            .await
            .map_err(|_| GatewayError::OidcUnavailable)
    }
}

#[derive(Debug, Deserialize)]
struct OidcMetadata {
    issuer: String,
    authorization_endpoint: String,
    token_endpoint: String,
}

#[derive(Debug, Deserialize)]
struct OidcTokenResponse {
    id_token: String,
}

pub fn random_opaque_token() -> String {
    URL_SAFE_NO_PAD.encode(format!(
        "{}{}",
        Uuid::new_v4().simple(),
        Uuid::new_v4().simple()
    ))
}

pub fn token_hash(token: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(token.as_bytes()))
}

pub fn pkce_challenge(verifier: &str) -> String {
    token_hash(verifier)
}

pub fn safe_return_to(value: Option<&str>) -> String {
    value
        .filter(|value| value.starts_with("/admin-ui") && !value.starts_with("//"))
        .unwrap_or("/admin-ui")
        .to_owned()
}

pub fn constant_time_eq(left: &str, right: &str) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.as_bytes()
        .iter()
        .zip(right.as_bytes())
        .fold(0_u8, |difference, (left, right)| {
            difference | (left ^ right)
        })
        == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread,
    };

    fn mock_metadata_server(issuer_override: Option<&str>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock metadata server");
        let address = listener.local_addr().expect("mock metadata address");
        let base_url = format!("http://{address}");
        let issuer = issuer_override.unwrap_or(&base_url).to_owned();
        let body = serde_json::json!({
            "issuer": issuer,
            "authorization_endpoint": format!("{base_url}/authorize"),
            "token_endpoint": format!("{base_url}/token")
        })
        .to_string();
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept metadata request");
            let mut request = [0_u8; 4096];
            let _ = stream.read(&mut request).expect("read metadata request");
            write!(
                stream,
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            )
            .expect("write metadata response");
        });
        base_url
    }

    fn mock_exchange_failure_server() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock token server");
        let address = listener.local_addr().expect("mock token address");
        let base_url = format!("http://{address}");
        let body = serde_json::json!({
            "issuer": base_url.clone(),
            "authorization_endpoint": format!("{base_url}/authorize"),
            "token_endpoint": format!("{base_url}/token")
        })
        .to_string();
        thread::spawn(move || {
            for (status, response_body) in [("200 OK", body.as_str()), ("401 Unauthorized", "{}")] {
                let (mut stream, _) = listener.accept().expect("accept OIDC request");
                let mut request = [0_u8; 4096];
                let _ = stream.read(&mut request).expect("read OIDC request");
                write!(
                    stream,
                    "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{response_body}",
                    response_body.len()
                )
                .expect("write OIDC response");
            }
        });
        base_url
    }

    fn test_config(base_url: &str) -> PortalOidcConfig {
        PortalOidcConfig {
            tenant_id: "tenant-test".into(),
            client_id: "browser-client".into(),
            client_secret: "fixture-value".into(),
            issuer: base_url.into(),
            discovery_url: format!("{base_url}/.well-known/openid-configuration"),
            redirect_uri: "http://127.0.0.1:18381/admin-ui/auth/callback".into(),
            post_logout_redirect_uri: "http://127.0.0.1:18381/admin-ui".into(),
            session_ttl_seconds: 3600,
            login_ttl_seconds: 300,
            cookie_secure: false,
        }
    }

    #[test]
    fn opaque_tokens_hash_and_pkce_without_revealing_source() {
        let token = random_opaque_token();
        assert!(token.len() >= 80);
        assert_ne!(token_hash(&token), token);
        assert_eq!(pkce_challenge(&token), token_hash(&token));
        assert!(constant_time_eq("same", "same"));
        assert!(!constant_time_eq("same", "diff"));
        assert!(!constant_time_eq("short", "longer"));
    }

    #[test]
    fn return_path_cannot_escape_admin_ui() {
        assert_eq!(safe_return_to(Some("/admin-ui#/usage")), "/admin-ui#/usage");
        assert_eq!(safe_return_to(Some("https://evil.example")), "/admin-ui");
        assert_eq!(safe_return_to(Some("//evil.example/admin-ui")), "/admin-ui");
    }

    #[tokio::test]
    async fn runtime_builds_authorization_request_and_maps_provider_failures() {
        let base_url = "http://127.0.0.1:9".to_owned();
        let mut invalid = test_config(&base_url);
        invalid.client_secret.clear();
        assert_eq!(
            invalid.validate().unwrap_err(),
            GatewayError::InvalidConfiguration
        );
        invalid.client_secret = "fixture-value".into();
        invalid.session_ttl_seconds = 0;
        assert_eq!(
            PortalOidcRuntime::new(invalid).unwrap_err(),
            GatewayError::InvalidConfiguration
        );

        let authorization_base = mock_metadata_server(None);
        let runtime =
            PortalOidcRuntime::new(test_config(&authorization_base)).expect("OIDC runtime");
        let url = runtime
            .authorization_url("state-value", "nonce-value", "pkce-value")
            .await
            .expect("authorization URL");
        let url = Url::parse(&url).expect("valid authorization URL");
        let query = url
            .query_pairs()
            .collect::<std::collections::HashMap<_, _>>();
        assert_eq!(
            query.get("state").map(|value| value.as_ref()),
            Some("state-value")
        );
        assert_eq!(
            query.get("nonce").map(|value| value.as_ref()),
            Some("nonce-value")
        );
        assert_eq!(
            query
                .get("code_challenge_method")
                .map(|value| value.as_ref()),
            Some("S256")
        );

        let failing_base = mock_metadata_server(Some("http://different-issuer.example"));
        let mismatched = PortalOidcRuntime::new(test_config(&failing_base)).unwrap();
        assert_eq!(
            mismatched
                .authorization_url("state", "nonce", "challenge")
                .await
                .unwrap_err(),
            GatewayError::OidcUnavailable
        );

        let exchange_base = mock_exchange_failure_server();
        let exchange_runtime = PortalOidcRuntime::new(test_config(&exchange_base)).unwrap();
        assert_eq!(
            exchange_runtime
                .exchange_code("invalid-code", "verifier", Utc::now())
                .await
                .unwrap_err(),
            GatewayError::InvalidOidcTransaction
        );
    }
}
