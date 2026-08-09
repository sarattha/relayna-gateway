use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{DateTime, Utc};
use gateway_core::{
    EntraAuthConfig, EntraIdentityContext, EntraJwtVerifier, GatewayError, GatewayResult,
    PortalAdminBootstrapPolicy, ENTRA_DEFAULT_RELAYNA_KEY_HEADER,
};
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use openssl::{pkey::PKey, x509::X509};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{fmt, fs, sync::Arc, time::Duration};
use url::Url;
use uuid::Uuid;

const OIDC_HTTP_TIMEOUT: Duration = Duration::from_secs(8);
const CLIENT_ASSERTION_LIFETIME_SECONDS: i64 = 300;
const CLIENT_ASSERTION_CLOCK_SKEW_SECONDS: i64 = 5;
const CLIENT_ASSERTION_TYPE: &str = "urn:ietf:params:oauth:client-assertion-type:jwt-bearer";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortalOidcConfig {
    pub tenant_id: String,
    pub client_id: String,
    pub private_key_path: String,
    pub certificate_path: String,
    pub admin_emails: Vec<String>,
    pub admin_object_ids: Vec<String>,
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
            &self.private_key_path,
            &self.certificate_path,
            &self.issuer,
            &self.discovery_url,
            &self.redirect_uri,
            &self.post_logout_redirect_uri,
        ]
        .iter()
        .any(|value| value.trim().is_empty())
            || self.session_ttl_seconds <= 0
            || self.login_ttl_seconds <= 0
            || Url::parse(&self.issuer).is_err()
            || Url::parse(&self.discovery_url).is_err()
            || !has_exact_path(&self.redirect_uri, "/admin-ui/auth/callback")
            || !has_exact_path(&self.post_logout_redirect_uri, "/admin-ui")
        {
            return Err(GatewayError::InvalidConfiguration);
        }
        PortalAdminBootstrapPolicy::new(
            self.tenant_id.clone(),
            self.admin_emails.clone(),
            self.admin_object_ids.clone(),
        )?;
        Ok(())
    }
}

fn has_exact_path(value: &str, expected_path: &str) -> bool {
    Url::parse(value).is_ok_and(|url| {
        url.path() == expected_path && url.query().is_none() && url.fragment().is_none()
    })
}

pub struct PortalOidcRuntime {
    pub config: PortalOidcConfig,
    verifier: Arc<EntraJwtVerifier>,
    client: reqwest::Client,
    client_assertion_key: EncodingKey,
    certificate_thumbprint: String,
    pub admin_bootstrap_policy: PortalAdminBootstrapPolicy,
}

impl fmt::Debug for PortalOidcRuntime {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PortalOidcRuntime")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

impl PortalOidcRuntime {
    pub fn new(config: PortalOidcConfig) -> GatewayResult<Self> {
        config.validate()?;
        let private_key =
            fs::read(&config.private_key_path).map_err(|_| GatewayError::InvalidConfiguration)?;
        let client_assertion_key = EncodingKey::from_rsa_pem(&private_key)
            .map_err(|_| GatewayError::InvalidConfiguration)?;
        let certificate =
            fs::read(&config.certificate_path).map_err(|_| GatewayError::InvalidConfiguration)?;
        let certificate =
            pem::parse(certificate).map_err(|_| GatewayError::InvalidConfiguration)?;
        if certificate.tag() != "CERTIFICATE" || certificate.contents().is_empty() {
            return Err(GatewayError::InvalidConfiguration);
        }
        validate_certificate_pair(&private_key, certificate.contents())?;
        let certificate_thumbprint = URL_SAFE_NO_PAD.encode(Sha256::digest(certificate.contents()));
        let admin_bootstrap_policy = PortalAdminBootstrapPolicy::new(
            config.tenant_id.clone(),
            config.admin_emails.clone(),
            config.admin_object_ids.clone(),
        )?;
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
            client_assertion_key,
            certificate_thumbprint,
            admin_bootstrap_policy,
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
        let client_assertion = self.client_assertion(&metadata.token_endpoint, now)?;
        let response = self
            .client
            .post(&metadata.token_endpoint)
            .form(&[
                ("grant_type", "authorization_code".to_owned()),
                ("client_id", self.config.client_id.clone()),
                ("client_assertion_type", CLIENT_ASSERTION_TYPE.to_owned()),
                ("client_assertion", client_assertion),
                ("redirect_uri", self.config.redirect_uri.clone()),
                ("code", code.to_owned()),
                ("code_verifier", pkce_verifier.to_owned()),
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

    fn client_assertion(&self, token_endpoint: &str, now: DateTime<Utc>) -> GatewayResult<String> {
        let mut header = Header::new(Algorithm::PS256);
        header.x5t_s256 = Some(self.certificate_thumbprint.clone());
        let issued_at = now.timestamp();
        encode(
            &header,
            &ClientAssertionClaims {
                issuer: &self.config.client_id,
                subject: &self.config.client_id,
                audience: token_endpoint,
                issued_at,
                not_before: issued_at - CLIENT_ASSERTION_CLOCK_SKEW_SECONDS,
                expires_at: issued_at + CLIENT_ASSERTION_LIFETIME_SECONDS,
                jwt_id: Uuid::new_v4().to_string(),
            },
            &self.client_assertion_key,
        )
        .map_err(|_| GatewayError::OidcUnavailable)
    }

    pub async fn end_session_url(&self) -> GatewayResult<String> {
        let metadata = self.metadata().await?;
        if metadata.issuer != self.config.issuer {
            return Err(GatewayError::OidcUnavailable);
        }
        let mut url = Url::parse(
            metadata
                .end_session_endpoint
                .as_deref()
                .ok_or(GatewayError::OidcUnavailable)?,
        )
        .map_err(|_| GatewayError::OidcUnavailable)?;
        url.query_pairs_mut().append_pair(
            "post_logout_redirect_uri",
            &self.config.post_logout_redirect_uri,
        );
        Ok(url.into())
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

fn validate_certificate_pair(private_key_pem: &[u8], certificate_der: &[u8]) -> GatewayResult<()> {
    let private_key = PKey::private_key_from_pem(private_key_pem)
        .map_err(|_| GatewayError::InvalidConfiguration)?;
    private_key
        .rsa()
        .map_err(|_| GatewayError::InvalidConfiguration)?;
    let certificate =
        X509::from_der(certificate_der).map_err(|_| GatewayError::InvalidConfiguration)?;
    let certificate_public_key = certificate
        .public_key()
        .map_err(|_| GatewayError::InvalidConfiguration)?;
    certificate_public_key
        .rsa()
        .map_err(|_| GatewayError::InvalidConfiguration)?;
    if !private_key.public_eq(&certificate_public_key) {
        return Err(GatewayError::InvalidConfiguration);
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
struct OidcMetadata {
    issuer: String,
    authorization_endpoint: String,
    token_endpoint: String,
    end_session_endpoint: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OidcTokenResponse {
    id_token: String,
}

#[derive(Debug, Serialize)]
struct ClientAssertionClaims<'a> {
    #[serde(rename = "iss")]
    issuer: &'a str,
    #[serde(rename = "sub")]
    subject: &'a str,
    #[serde(rename = "aud")]
    audience: &'a str,
    #[serde(rename = "iat")]
    issued_at: i64,
    #[serde(rename = "nbf")]
    not_before: i64,
    #[serde(rename = "exp")]
    expires_at: i64,
    #[serde(rename = "jti")]
    jwt_id: String,
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
        collections::HashMap,
        io::{Read, Write},
        net::TcpListener,
        path::PathBuf,
        process::{Command, Stdio},
        sync::mpsc,
        thread,
    };

    struct TestCredentials {
        directory: PathBuf,
        private_key_path: String,
        certificate_path: String,
        public_key_path: String,
    }

    impl Drop for TestCredentials {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.directory);
        }
    }

    fn test_credentials() -> TestCredentials {
        let directory = std::env::temp_dir().join(format!(
            "relayna-portal-oidc-test-{}",
            Uuid::new_v4().simple()
        ));
        std::fs::create_dir(&directory).expect("create certificate fixture directory");
        let private_key = directory.join("private-key.pem");
        let certificate = directory.join("certificate.pem");
        let public_key = directory.join("public-key.pem");
        let status = Command::new("openssl")
            .args([
                "req", "-x509", "-newkey", "rsa:2048", "-sha256", "-nodes", "-keyout",
            ])
            .arg(&private_key)
            .args(["-out"])
            .arg(&certificate)
            .args(["-days", "1", "-subj", "/CN=relayna-portal-test"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("run openssl certificate generation");
        assert!(status.success(), "generate certificate fixture");
        let status = Command::new("openssl")
            .args(["pkey", "-in"])
            .arg(&private_key)
            .args(["-pubout", "-out"])
            .arg(&public_key)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("run openssl public-key extraction");
        assert!(status.success(), "extract public key fixture");
        TestCredentials {
            directory,
            private_key_path: private_key.to_string_lossy().into_owned(),
            certificate_path: certificate.to_string_lossy().into_owned(),
            public_key_path: public_key.to_string_lossy().into_owned(),
        }
    }

    fn mock_metadata_server(issuer_override: Option<&str>, request_count: usize) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock metadata server");
        let address = listener.local_addr().expect("mock metadata address");
        let base_url = format!("http://{address}");
        let issuer = issuer_override.unwrap_or(&base_url).to_owned();
        let body = serde_json::json!({
            "issuer": issuer,
            "authorization_endpoint": format!("{base_url}/authorize"),
            "token_endpoint": format!("{base_url}/token"),
            "end_session_endpoint": format!("{base_url}/logout")
        })
        .to_string();
        thread::spawn(move || {
            for _ in 0..request_count {
                let (mut stream, _) = listener.accept().expect("accept metadata request");
                let mut request = [0_u8; 4096];
                let _ = stream.read(&mut request).expect("read metadata request");
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                )
                .expect("write metadata response");
            }
        });
        base_url
    }

    fn mock_exchange_failure_server() -> (String, mpsc::Receiver<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock token server");
        let address = listener.local_addr().expect("mock token address");
        let base_url = format!("http://{address}");
        let body = serde_json::json!({
            "issuer": base_url.clone(),
            "authorization_endpoint": format!("{base_url}/authorize"),
            "token_endpoint": format!("{base_url}/token")
        })
        .to_string();
        let (request_tx, request_rx) = mpsc::channel();
        thread::spawn(move || {
            for (index, (status, response_body)) in
                [("200 OK", body.as_str()), ("401 Unauthorized", "{}")]
                    .into_iter()
                    .enumerate()
            {
                let (mut stream, _) = listener.accept().expect("accept OIDC request");
                let mut request = [0_u8; 4096];
                let length = stream.read(&mut request).expect("read OIDC request");
                if index == 1 {
                    request_tx
                        .send(String::from_utf8_lossy(&request[..length]).into_owned())
                        .expect("capture token request");
                }
                write!(
                    stream,
                    "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{response_body}",
                    response_body.len()
                )
                .expect("write OIDC response");
            }
        });
        (base_url, request_rx)
    }

    fn test_config(base_url: &str, credentials: &TestCredentials) -> PortalOidcConfig {
        PortalOidcConfig {
            tenant_id: "tenant-test".into(),
            client_id: "browser-client".into(),
            private_key_path: credentials.private_key_path.clone(),
            certificate_path: credentials.certificate_path.clone(),
            admin_emails: vec!["first.admin@example.test".into()],
            admin_object_ids: vec!["object-admin".into()],
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
        let credentials = test_credentials();
        let base_url = "http://127.0.0.1:9".to_owned();
        let mut invalid = test_config(&base_url, &credentials);
        invalid.private_key_path.clear();
        assert_eq!(
            invalid.validate().unwrap_err(),
            GatewayError::InvalidConfiguration
        );
        invalid.private_key_path = credentials.private_key_path.clone();
        invalid.session_ttl_seconds = 0;
        assert_eq!(
            PortalOidcRuntime::new(invalid).unwrap_err(),
            GatewayError::InvalidConfiguration
        );

        let invalid_certificate_path = credentials.directory.join("invalid-certificate.pem");
        std::fs::write(
            &invalid_certificate_path,
            "-----BEGIN CERTIFICATE-----\nAQID\n-----END CERTIFICATE-----\n",
        )
        .expect("write invalid certificate");
        let mut invalid_certificate = test_config(&base_url, &credentials);
        invalid_certificate.certificate_path =
            invalid_certificate_path.to_string_lossy().into_owned();
        assert_eq!(
            PortalOidcRuntime::new(invalid_certificate).unwrap_err(),
            GatewayError::InvalidConfiguration
        );

        let other_credentials = test_credentials();
        let mut mismatched_certificate = test_config(&base_url, &credentials);
        mismatched_certificate.certificate_path = other_credentials.certificate_path.clone();
        assert_eq!(
            PortalOidcRuntime::new(mismatched_certificate).unwrap_err(),
            GatewayError::InvalidConfiguration
        );

        let authorization_base = mock_metadata_server(None, 2);
        let runtime = PortalOidcRuntime::new(test_config(&authorization_base, &credentials))
            .expect("OIDC runtime");
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
        assert_eq!(
            runtime.end_session_url().await.expect("end-session URL"),
            format!(
                "{authorization_base}/logout?post_logout_redirect_uri=http%3A%2F%2F127.0.0.1%3A18381%2Fadmin-ui"
            )
        );

        let failing_base = mock_metadata_server(Some("http://different-issuer.example"), 1);
        let mismatched = PortalOidcRuntime::new(test_config(&failing_base, &credentials)).unwrap();
        assert_eq!(
            mismatched
                .authorization_url("state", "nonce", "challenge")
                .await
                .unwrap_err(),
            GatewayError::OidcUnavailable
        );

        let (exchange_base, request_rx) = mock_exchange_failure_server();
        let exchange_runtime =
            PortalOidcRuntime::new(test_config(&exchange_base, &credentials)).unwrap();
        let assertion_time = Utc::now();
        assert_eq!(
            exchange_runtime
                .exchange_code("invalid-code", "verifier", assertion_time)
                .await
                .unwrap_err(),
            GatewayError::InvalidOidcTransaction
        );
        let request = request_rx.recv().expect("captured token request");
        let body = request.split("\r\n\r\n").nth(1).expect("request body");
        let form = url::form_urlencoded::parse(body.as_bytes())
            .into_owned()
            .collect::<HashMap<_, _>>();
        assert!(!form.contains_key("client_secret"));
        assert_eq!(
            form.get("client_assertion_type").map(String::as_str),
            Some(CLIENT_ASSERTION_TYPE)
        );
        let assertion = form.get("client_assertion").expect("client assertion");
        let header = jsonwebtoken::decode_header(assertion).expect("assertion header");
        assert_eq!(header.alg, Algorithm::PS256);
        let certificate = pem::parse(
            std::fs::read(&credentials.certificate_path).expect("read test certificate"),
        )
        .expect("parse test certificate");
        assert_eq!(
            header.x5t_s256,
            Some(URL_SAFE_NO_PAD.encode(Sha256::digest(certificate.contents())))
        );
        let public_key = std::fs::read(&credentials.public_key_path).expect("public key");
        let mut validation = jsonwebtoken::Validation::new(Algorithm::PS256);
        validation.set_audience(&[format!("{exchange_base}/token")]);
        validation.set_issuer(&["browser-client"]);
        validation.sub = Some("browser-client".into());
        let verified = jsonwebtoken::decode::<serde_json::Value>(
            assertion,
            &jsonwebtoken::DecodingKey::from_rsa_pem(&public_key).expect("decoding key"),
            &validation,
        )
        .expect("valid client assertion");
        assert_eq!(verified.claims["iss"], "browser-client");
        assert_eq!(verified.claims["sub"], "browser-client");
        assert_eq!(verified.claims["aud"], format!("{exchange_base}/token"));
        assert_eq!(
            verified.claims["exp"].as_i64(),
            Some(assertion_time.timestamp() + 300)
        );
        assert_eq!(
            verified.claims["nbf"].as_i64(),
            Some(assertion_time.timestamp() - 5)
        );
        assert!(verified.claims["jti"]
            .as_str()
            .is_some_and(|jti| !jti.is_empty()));
    }
}
