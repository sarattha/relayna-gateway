use gateway_core::{
    foundry::{FoundryIdentityMethod, FoundryRuntimeConfig},
    GatewayError, GatewayResult,
};
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};
use tokio::sync::Mutex;
use uuid::Uuid;

struct CachedToken {
    revision: i64,
    token: String,
    refresh_at: Instant,
}

/// Tokens never enter persisted configuration, diagnostics, or Debug output.
#[derive(Default)]
pub(crate) struct FoundryTokenCache {
    entries: Mutex<HashMap<Uuid, CachedToken>>,
}

impl FoundryTokenCache {
    pub async fn token(&self, config: &FoundryRuntimeConfig) -> GatewayResult<String> {
        config
            .identity
            .validate(&config.base_url, config.secret.is_some())?;
        let entries = self.entries.lock().await;
        if let Some(cached) = entries.get(&config.id) {
            if cached.revision == config.revision && cached.refresh_at > Instant::now() {
                return Ok(cached.token.clone());
            }
        }
        drop(entries);
        let (token, lifetime) = acquire_token(config).await?;
        let mut entries = self.entries.lock().await;
        if let Some(cached) = entries.get(&config.id) {
            // An older in-flight request must not overwrite a newer revision.
            if cached.revision > config.revision {
                return Ok(token);
            }
            if cached.revision == config.revision && cached.refresh_at > Instant::now() {
                return Ok(cached.token.clone());
            }
        }
        entries.retain(|_, v| v.refresh_at > Instant::now());
        if entries.len() >= 256 {
            entries.clear();
        }
        let refresh_after = lifetime.saturating_sub((lifetime / 10).clamp(1, 60));
        entries.insert(
            config.id,
            CachedToken {
                revision: config.revision,
                token: token.clone(),
                refresh_at: Instant::now() + Duration::from_secs(refresh_after),
            },
        );
        Ok(token)
    }
}

async fn acquire_token(config: &FoundryRuntimeConfig) -> GatewayResult<(String, u64)> {
    acquire_token_checked(config)
        .await
        .map_err(|_| GatewayError::FoundryCredentialUnavailable)
}

async fn acquire_token_checked(
    config: &FoundryRuntimeConfig,
) -> Result<(String, u64), IdentityFailure> {
    config
        .identity
        .validate(&config.base_url, config.secret.is_some())
        .map_err(|_| IdentityFailure::Configuration)?;
    let error = || IdentityFailure::TokenResponse;
    let builder = reqwest::Client::builder();
    let builder = if config.identity.method == FoundryIdentityMethod::ManagedIdentity {
        builder.no_proxy()
    } else {
        builder
    };
    let client = builder
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|_| error())?;
    let method = config.identity.method;
    let request = if method == FoundryIdentityMethod::ManagedIdentity {
        let endpoint =
            token_endpoint(config, Uuid::nil()).map_err(|_| IdentityFailure::Configuration)?;
        let mut request = client.get(endpoint).header("Metadata", "true").query(&[
            ("api-version", "2018-02-01"),
            ("resource", "https://ai.azure.com"),
        ]);
        if let Some(client_id) = config.identity.client_id {
            request = request.query(&[("client_id", client_id.to_string())]);
        }
        request
    } else {
        let tenant = config.identity.tenant_id.ok_or_else(error)?;
        let endpoint =
            token_endpoint(config, tenant).map_err(|_| IdentityFailure::Configuration)?;
        let mut form = vec![
            ("grant_type", "client_credentials".to_owned()),
            (
                "client_id",
                config.identity.client_id.ok_or_else(error)?.to_string(),
            ),
            ("scope", "https://ai.azure.com/.default".to_owned()),
        ];
        if method == FoundryIdentityMethod::ClientSecret {
            form.push(("client_secret", config.secret.clone().ok_or_else(error)?));
        } else {
            // The deployment controls this path, never the public/admin request.
            let path = std::env::var("AZURE_FEDERATED_TOKEN_FILE")
                .map_err(|_| IdentityFailure::WorkloadMissing)?;
            let assertion = tokio::task::spawn_blocking(move || {
                use std::io::Read;
                let mut text = String::new();
                std::fs::File::open(path)?
                    .take(65_537)
                    .read_to_string(&mut text)?;
                Ok::<_, std::io::Error>(text)
            })
            .await
            .map_err(|_| IdentityFailure::WorkloadUnreadable)?
            .map_err(|_| IdentityFailure::WorkloadUnreadable)?;
            if assertion.trim().is_empty() || assertion.len() > 65_536 {
                return Err(IdentityFailure::WorkloadInvalid);
            }
            form.push((
                "client_assertion_type",
                "urn:ietf:params:oauth:client-assertion-type:jwt-bearer".into(),
            ));
            form.push(("client_assertion", assertion.trim().into()));
        }
        client.post(endpoint).form(&form)
    };
    let mut response = request.send().await.map_err(IdentityFailure::transport)?;
    if !response.status().is_success() {
        return Err(IdentityFailure::Rejected(response.status().as_u16()));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| error())? {
        if bytes.len() + chunk.len() > 65_536 {
            return Err(error());
        }
        bytes.extend_from_slice(&chunk);
    }
    let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| error())?;
    let token = value
        .get("access_token")
        .and_then(|v| v.as_str())
        .ok_or_else(error)?;
    if token.is_empty()
        || token
            .bytes()
            .any(|c| c.is_ascii_whitespace() || c.is_ascii_control())
    {
        return Err(error());
    }
    if !value
        .get("token_type")
        .and_then(|v| v.as_str())
        .is_some_and(|v| v.eq_ignore_ascii_case("Bearer"))
    {
        return Err(error());
    }
    let lifetime = value
        .get("expires_in")
        .and_then(|v| v.as_u64().or_else(|| v.as_str()?.parse().ok()))
        .ok_or_else(error)?;
    if lifetime == 0 || lifetime > 86_400 {
        return Err(error());
    }
    Ok((token.into(), lifetime))
}

fn token_endpoint(config: &FoundryRuntimeConfig, tenant: Uuid) -> GatewayResult<String> {
    // Mock mode is deliberately restricted to a loopback Foundry connection and
    // a loopback token endpoint. It cannot redirect real Azure credentials.
    if config.base_url.starts_with("http://127.0.0.1:") {
        if let Ok(endpoint) = std::env::var("GATEWAY_FOUNDRY_MOCK_TOKEN_ENDPOINT") {
            let url = url::Url::parse(&endpoint)
                .map_err(|_| GatewayError::InvalidFoundryConfiguration)?;
            if url.scheme() != "http"
                || url.host_str() != Some("127.0.0.1")
                || !url.username().is_empty()
                || url.password().is_some()
            {
                return Err(GatewayError::InvalidFoundryConfiguration);
            }
            return Ok(endpoint);
        }
    }
    if config.identity.method == FoundryIdentityMethod::ManagedIdentity {
        return Ok("http://169.254.169.254/metadata/identity/oauth2/token".into());
    }
    Ok(format!(
        "https://login.microsoftonline.com/{tenant}/oauth2/v2.0/token"
    ))
}

/// Safe diagnostic fields only: never serialize Azure response bodies or tokens.
#[derive(Debug, serde::Serialize)]
pub struct FoundryCheckStep {
    pub status: &'static str,
    pub code: &'static str,
    pub message: &'static str,
    pub http_status: Option<u16>,
}
impl FoundryCheckStep {
    fn new(
        status: &'static str,
        code: &'static str,
        message: &'static str,
        http_status: Option<u16>,
    ) -> Self {
        Self {
            status,
            code,
            message,
            http_status,
        }
    }
}

#[derive(Debug, serde::Serialize)]
pub struct FoundryConnectionCheck {
    pub identity: FoundryCheckStep,
    pub project: FoundryCheckStep,
}

#[derive(Debug)]
enum IdentityFailure {
    Configuration,
    WorkloadMissing,
    WorkloadUnreadable,
    WorkloadInvalid,
    Rejected(u16),
    Network,
    Timeout,
    TokenResponse,
}
impl IdentityFailure {
    fn transport(error: reqwest::Error) -> Self {
        if error.is_timeout() {
            Self::Timeout
        } else {
            Self::Network
        }
    }
    fn step(self, method: FoundryIdentityMethod) -> FoundryCheckStep {
        let (code, message, status) = match self {
            Self::Configuration => ("identity_configuration", "The saved Azure identity configuration is incomplete or invalid. Check the project endpoint, tenant ID, client ID and identity method, then save and retry.", None),
            Self::WorkloadMissing => ("workload_token_missing", "AZURE_FEDERATED_TOKEN_FILE is not set on this gateway instance. Enable AKS workload identity, label the pod azure.workload.identity/use=true and configure its service account, then recreate the pod and retry.", None),
            Self::WorkloadUnreadable => ("workload_token_unreadable", "The gateway cannot read the projected workload token. Check the pod's token volume mount and file permissions, then retry.", None),
            Self::WorkloadInvalid => ("workload_token_invalid", "The projected workload token is empty, invalid or too large. Check the service-account token projection and restart the affected pod before retrying.", None),
            Self::Rejected(status) => ("identity_rejected", match method {
                FoundryIdentityMethod::WorkloadIdentity => "Azure rejected the workload token exchange. Check the tenant and client IDs and the federated credential's issuer, service-account subject and api://AzureADTokenExchange audience. If Azure is throttling or unavailable, retry later.",
                FoundryIdentityMethod::ManagedIdentity => "Azure's VM identity endpoint rejected the request. Enable managed identity on the VM and confirm the configured client ID is assigned to it. For AKS, select Workload identity instead. Retry later if Azure is unavailable.",
                FoundryIdentityMethod::ClientSecret => "Azure rejected the client credentials. Check the tenant, client ID and secret value/expiry, save any correction and retry. Retry later if Azure is throttling or unavailable.",
            }, Some(status)),
            Self::Network => ("identity_network", match method {
                FoundryIdentityMethod::ManagedIdentity => "The gateway cannot reach Azure VM IMDS. Run on an Azure VM with managed identity and allow access to 169.254.169.254. For AKS, select Workload identity instead.",
                _ => "The gateway cannot connect securely to Microsoft Entra. Check DNS, TLS trust, proxy and outbound HTTPS access to login.microsoftonline.com, then retry.",
            }, None),
            Self::Timeout => ("identity_timeout", "Azure token acquisition timed out. Check this instance's identity endpoint connectivity and retry.", None),
            Self::TokenResponse => ("identity_response_invalid", "Azure did not return a usable bearer token. Check the identity endpoint/network proxy and retry; inspect Azure identity diagnostics if this persists.", None),
        };
        FoundryCheckStep::new("failed", code, message, status)
    }
}

/// Fresh acquisition deliberately bypasses the forwarding cache so broken mounts
/// and rotated credentials are tested now. This check never invokes an agent.
pub async fn verify_foundry_connection(config: &FoundryRuntimeConfig) -> FoundryConnectionCheck {
    let token = match acquire_token_checked(config).await {
        Ok((token, _)) => token,
        Err(error) => {
            return FoundryConnectionCheck {
                identity: error.step(config.identity.method),
                project: FoundryCheckStep::new(
                    "skipped",
                    "identity_required",
                    "Fix the Azure identity check, then retry to check Foundry project access.",
                    None,
                ),
            }
        }
    };
    FoundryConnectionCheck {
        identity: FoundryCheckStep::new(
            "passed",
            "token_acquired",
            "Azure issued a fresh token for Foundry using the saved identity configuration.",
            None,
        ),
        project: check_project(config, &token).await,
    }
}

async fn check_project(config: &FoundryRuntimeConfig, token: &str) -> FoundryCheckStep {
    let result = async {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(10))
            .build()?;
        client
            .get(format!("{}/agents", config.base_url.trim_end_matches('/')))
            .query(&[("api-version", "v1"), ("limit", "1")])
            .header(reqwest::header::ACCEPT, "application/json")
            .bearer_auth(token)
            .send()
            .await
    }
    .await;
    let mut response = match result {
        Ok(response) => response,
        Err(error) => {
            return if error.is_timeout() {
                FoundryCheckStep::new("failed", "project_timeout", "Foundry did not respond in time. Check this instance's network access and retry.", None)
            } else {
                FoundryCheckStep::new("failed", "project_network", "Cannot connect securely to Foundry. Check the project endpoint, DNS, TLS trust, proxy, firewall and private-endpoint routing from this instance.", None)
            }
        }
    };
    let status = response.status().as_u16();
    if status == 200 {
        // Validate a bounded list envelope, never expose agent data or follow links.
        let mut bytes = Vec::new();
        let complete = loop {
            match response.chunk().await {
                Ok(Some(chunk)) if bytes.len() + chunk.len() <= 65_536 => {
                    bytes.extend_from_slice(&chunk)
                }
                Ok(None) => break true,
                _ => break false,
            }
        };
        if complete
            && serde_json::from_slice::<serde_json::Value>(&bytes)
                .ok()
                .is_some_and(|v| v.get("data").is_some_and(|v| v.is_array()))
        {
            return FoundryCheckStep::new("passed", "project_read_verified", "Foundry accepted the token and allowed the read-only agents check. Agent execution, tools and model access have not been tested.", Some(status));
        }
        return FoundryCheckStep::new("failed", "project_response_invalid", "Foundry returned an unexpected or oversized response. Confirm the project endpoint and any proxy configuration, then retry.", Some(status));
    }
    let (state, code, message) = match status {
        401 => ("failed", "project_unauthorized", "Foundry rejected the fresh Azure token. Confirm this project belongs to the configured tenant and the identity is intended for this Foundry resource."),
        403 => ("inconclusive", "project_read_forbidden", "Azure identity works, but Foundry denied the read-only agents check. Check project RBAC scope and network restrictions. An invocation-only role may legitimately lack read permission; do not broaden its role just to pass this check. Agent invocation remains unverified."),
        404 => ("failed", "project_not_found", "Foundry could not find this project/API path. Copy the project endpoint from Foundry and confirm it supports the v1 agents API, then save and retry."),
        429 => ("inconclusive", "project_throttled", "Foundry throttled the read-only check. Wait and retry; identity token acquisition succeeded."),
        500..=599 => ("inconclusive", "project_unavailable", "Foundry is temporarily unavailable. Retry later; identity token acquisition succeeded."),
        300..=399 => ("failed", "project_redirect", "Foundry redirected the check. Redirects are not followed to protect credentials. Save the correct project endpoint and retry."),
        _ => ("failed", "project_rejected", "Foundry rejected the read-only agents check. Confirm the project endpoint and support for the v1 agents API, then retry."),
    };
    FoundryCheckStep::new(state, code, message, Some(status))
}

/// Observe individual SSE data lines without delaying forwarding. Oversized
/// events are ignored for accounting, never retained as an unbounded response.
#[derive(Debug, Default)]
pub(crate) struct FoundryUsage {
    line: Vec<u8>,
    overflow: bool,
    pub tokens: (Option<i64>, Option<i64>, Option<i64>),
}

impl FoundryUsage {
    pub fn feed(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            if byte == b'\n' {
                if !self.overflow {
                    if let Some(data) = self.line.strip_prefix(b"data:") {
                        if let Ok(value) = serde_json::from_slice::<serde_json::Value>(data) {
                            if let Some(usage) = value.pointer("/response/usage") {
                                let count = |name: &str| {
                                    usage.get(name).and_then(|v| v.as_i64()).filter(|n| *n >= 0)
                                };
                                self.tokens = (
                                    count("input_tokens"),
                                    count("output_tokens"),
                                    count("total_tokens"),
                                );
                            }
                        }
                    }
                }
                self.line.clear();
                self.overflow = false;
            } else if self.line.len() < 65_536 {
                self.line.push(byte);
            } else {
                self.overflow = true;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    #[test]
    fn streaming_usage_handles_chunk_boundaries_and_bounded_observation() {
        let mut usage = FoundryUsage::default();
        let event = b"event: response.completed\r\ndata: {\"response\":{\"usage\":{\"input_tokens\":2,\"output_tokens\":3,\"total_tokens\":5}}}\r\n\r\n";
        for byte in event {
            usage.feed(&[*byte]);
        }
        assert_eq!(usage.tokens, (Some(2), Some(3), Some(5)));
        usage.feed(b"data: [DONE]\ndata: {}\n");
        assert_eq!(usage.tokens.2, Some(5));
        usage.feed(&vec![b'x'; 70_000]);
        assert_eq!(usage.line.len(), 65_536);
        usage.feed(b"\ndata: {\"response\":{\"usage\":{\"input_tokens\":-1}}}\n");
        assert_eq!(usage.tokens, (None, None, None));
        usage.feed(event);
        assert_eq!(usage.tokens.2, Some(5));
    }

    fn mock_response(status: u16, body: String) -> String {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0; 8192];
            let _ = stream.read(&mut buf);
            let response = format!(
                "HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes());
        });
        format!("http://{address}/token")
    }
    fn config() -> FoundryRuntimeConfig {
        FoundryRuntimeConfig {
            id: Uuid::new_v4(),
            base_url: "http://127.0.0.1:1234/api/projects/mock".into(),
            identity: gateway_core::foundry::FoundryIdentity {
                method: FoundryIdentityMethod::ClientSecret,
                tenant_id: Some(Uuid::nil()),
                client_id: Some(Uuid::nil()),
            },
            secret: Some("test-secret".into()),
            revision: 1,
        }
    }
    // One test owns process environment for this module, avoiding parallel env races.
    #[tokio::test]
    async fn token_contract_errors_rotation_and_workload_assertions() {
        let mut c = config();
        for (status,body) in [(401,"private error".into()),(302,"redirect".into()),(200,"not-json".into()),(200,"{}".into()),(200,serde_json::json!({"access_token":"bad token","token_type":"Bearer","expires_in":60}).to_string()),(200,serde_json::json!({"access_token":"","token_type":"Bearer","expires_in":60}).to_string()),(200,serde_json::json!({"access_token":"token","token_type":"Basic","expires_in":60}).to_string()),(200,serde_json::json!({"access_token":"token","token_type":"Bearer","expires_in":0}).to_string()),(200,serde_json::json!({"access_token":"token","token_type":"Bearer","expires_in":86401}).to_string()),(200,serde_json::json!({"access_token":"token","token_type":"Bearer"}).to_string()),(200,"x".repeat(65_537))] {
            std::env::set_var("GATEWAY_FOUNDRY_MOCK_TOKEN_ENDPOINT",mock_response(status,body));
            assert_eq!(acquire_token(&c).await.unwrap_err(), GatewayError::FoundryCredentialUnavailable);
        }
        let valid =
            serde_json::json!({"access_token":"token","token_type":"bearer","expires_in":"60"})
                .to_string();
        std::env::set_var(
            "GATEWAY_FOUNDRY_MOCK_TOKEN_ENDPOINT",
            mock_response(200, valid.clone()),
        );
        let cache = FoundryTokenCache::default();
        assert_eq!(cache.token(&c).await.unwrap(), "token");
        assert_eq!(
            cache.token(&c).await.unwrap(),
            "token",
            "second call does not need a server"
        );
        c.revision += 1;
        std::env::set_var(
            "GATEWAY_FOUNDRY_MOCK_TOKEN_ENDPOINT",
            mock_response(200, valid.clone()),
        );
        cache.token(&c).await.unwrap();
        cache
            .entries
            .lock()
            .await
            .get_mut(&c.id)
            .unwrap()
            .refresh_at = Instant::now();
        std::env::set_var(
            "GATEWAY_FOUNDRY_MOCK_TOKEN_ENDPOINT",
            mock_response(200, valid.clone()),
        );
        cache.token(&c).await.unwrap();
        // A stalled acquisition must not block a valid token for another provider.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        std::env::set_var(
            "GATEWAY_FOUNDRY_MOCK_TOKEN_ENDPOINT",
            format!("http://{}/token", listener.local_addr().unwrap()),
        );
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = tokio::sync::oneshot::channel();
        let body = valid.clone();
        let server = tokio::spawn(async move {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buffer = [0; 4096];
            assert!(socket.read(&mut buffer).await.unwrap() > 0);
            started_tx.send(()).unwrap();
            release_rx.await.unwrap();
            socket
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
        });
        let mut slow_config = c.clone();
        slow_config.id = Uuid::new_v4();
        let slow = cache.token(&slow_config);
        tokio::pin!(slow);
        tokio::select! {
            result = &mut slow => panic!("acquisition unexpectedly completed: {result:?}"),
            _ = started_rx => {}
        }
        assert_eq!(
            tokio::time::timeout(Duration::from_millis(200), cache.token(&c))
                .await
                .unwrap()
                .unwrap(),
            "token"
        );
        // Simulate a newer revision completing before this slow old request.
        cache.entries.lock().await.insert(
            slow_config.id,
            CachedToken {
                revision: slow_config.revision + 1,
                token: "newer".into(),
                refresh_at: Instant::now() + Duration::from_secs(60),
            },
        );
        release_tx.send(()).unwrap();
        assert_eq!(slow.await.unwrap(), "token");
        assert_eq!(
            cache
                .entries
                .lock()
                .await
                .get(&slow_config.id)
                .unwrap()
                .token,
            "newer"
        );
        server.await.unwrap();
        for _ in 0..256 {
            cache.entries.lock().await.insert(
                Uuid::new_v4(),
                CachedToken {
                    revision: 1,
                    token: "old".into(),
                    refresh_at: Instant::now() + Duration::from_secs(60),
                },
            );
        }
        c.revision += 1;
        std::env::set_var(
            "GATEWAY_FOUNDRY_MOCK_TOKEN_ENDPOINT",
            mock_response(200, valid.clone()),
        );
        cache.token(&c).await.unwrap();
        assert_eq!(cache.entries.lock().await.len(), 1);
        c.identity.method = FoundryIdentityMethod::ManagedIdentity;
        for client_id in [Some(Uuid::nil()), None] {
            c.identity.client_id = client_id;
            std::env::set_var(
                "GATEWAY_FOUNDRY_MOCK_TOKEN_ENDPOINT",
                mock_response(200, valid.clone()),
            );
            assert_eq!(acquire_token(&c).await.unwrap().0, "token");
        }
        std::env::remove_var("GATEWAY_FOUNDRY_MOCK_TOKEN_ENDPOINT");
        assert!(token_endpoint(&c, Uuid::nil())
            .unwrap()
            .starts_with("http://169.254.169.254/"));
        c.identity.client_id = Some(Uuid::nil());
        c.identity.method = FoundryIdentityMethod::WorkloadIdentity;
        std::env::remove_var("AZURE_FEDERATED_TOKEN_FILE");
        assert!(acquire_token(&c).await.is_err());
        assert_eq!(
            verify_foundry_connection(&c).await.identity.code,
            "workload_token_missing"
        );
        let path = std::env::temp_dir().join(format!("foundry-assertion-{}", Uuid::new_v4()));
        std::env::set_var("AZURE_FEDERATED_TOKEN_FILE", &path);
        assert!(acquire_token(&c).await.is_err());
        assert_eq!(
            verify_foundry_connection(&c).await.identity.code,
            "workload_token_unreadable"
        );
        for content in ["".to_owned(), "x".repeat(65_537)] {
            std::fs::write(&path, content).unwrap();
            assert!(acquire_token(&c).await.is_err());
            assert_eq!(
                verify_foundry_connection(&c).await.identity.code,
                "workload_token_invalid"
            );
        }
        std::fs::write(&path, "mock-federated-assertion").unwrap();
        std::env::set_var(
            "GATEWAY_FOUNDRY_MOCK_TOKEN_ENDPOINT",
            mock_response(200, valid),
        );
        assert_eq!(acquire_token(&c).await.unwrap().0, "token");
        for method in [
            FoundryIdentityMethod::ClientSecret,
            FoundryIdentityMethod::WorkloadIdentity,
            FoundryIdentityMethod::ManagedIdentity,
        ] {
            c.identity.method = method;
            c.base_url = format!(
                "{}/api/projects/mock",
                mock_response(200, serde_json::json!({"data":[]}).to_string())
                    .trim_end_matches("/token")
            );
            std::env::set_var("GATEWAY_FOUNDRY_MOCK_TOKEN_ENDPOINT", mock_response(200, serde_json::json!({"access_token":"token","token_type":"Bearer","expires_in":60}).to_string()));
            let report = verify_foundry_connection(&c).await;
            assert_eq!(report.identity.status, "passed");
            assert_eq!(report.project.status, "passed");
            std::env::set_var(
                "GATEWAY_FOUNDRY_MOCK_TOKEN_ENDPOINT",
                mock_response(401, "secret Azure error".into()),
            );
            let report = verify_foundry_connection(&c).await;
            assert_eq!(report.identity.code, "identity_rejected");
            assert_eq!(report.project.status, "skipped");
            assert!(!serde_json::to_string(&report)
                .unwrap()
                .contains("secret Azure error"));
        }
        c.identity.method = FoundryIdentityMethod::WorkloadIdentity;
        std::fs::remove_file(path).unwrap();
        std::env::remove_var("AZURE_FEDERATED_TOKEN_FILE");
        for endpoint in [
            "bad",
            "https://evil.example/token",
            "http://user@127.0.0.1/token",
        ] {
            std::env::set_var("GATEWAY_FOUNDRY_MOCK_TOKEN_ENDPOINT", endpoint);
            assert!(token_endpoint(&c, Uuid::nil()).is_err());
        }
        c.base_url = "https://account.services.ai.azure.com/api/projects/demo".into();
        assert_eq!(token_endpoint(&c,Uuid::nil()).unwrap(),"https://login.microsoftonline.com/00000000-0000-0000-0000-000000000000/oauth2/v2.0/token");
        std::env::remove_var("GATEWAY_FOUNDRY_MOCK_TOKEN_ENDPOINT");
        c.base_url = "http://127.0.0.1:1234/api/projects/mock".into();
        assert!(token_endpoint(&c, Uuid::nil())
            .unwrap()
            .starts_with("https://login.microsoftonline.com"));
    }
    #[tokio::test]
    async fn project_probe_rejects_malformed_oversized_and_unreachable_responses() {
        let mut c = config();
        for body in [
            "{}".into(),
            "<html>login</html>".into(),
            "x".repeat(65_537),
            serde_json::json!({"data":[],"padding":"x".repeat(65_536)}).to_string(),
        ] {
            c.base_url = mock_response(200, body);
            assert_eq!(
                check_project(&c, "safe-test-token").await.code,
                "project_response_invalid"
            );
        }
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        c.base_url = format!("http://{}", listener.local_addr().unwrap());
        drop(listener);
        assert_eq!(
            check_project(&c, "safe-test-token").await.code,
            "project_network"
        );
        c.base_url = "invalid URL".into();
        assert_eq!(
            verify_foundry_connection(&c).await.identity.code,
            "identity_configuration"
        );
    }

    #[test]
    fn identity_diagnostics_are_actionable_and_method_specific() {
        for method in [
            FoundryIdentityMethod::ClientSecret,
            FoundryIdentityMethod::WorkloadIdentity,
            FoundryIdentityMethod::ManagedIdentity,
        ] {
            for error in [
                IdentityFailure::Rejected(401),
                IdentityFailure::Network,
                IdentityFailure::Timeout,
                IdentityFailure::TokenResponse,
            ] {
                let step = error.step(method);
                assert_eq!(step.status, "failed");
                assert!(!step.message.is_empty());
                assert!(!step.code.is_empty());
            }
        }
    }

    #[tokio::test]
    async fn transport_failures_distinguish_timeout_from_connection_failure() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let client = reqwest::Client::builder()
            .timeout(Duration::from_millis(20))
            .build()
            .unwrap();
        let error = client.get(&endpoint).send().await.unwrap_err();
        assert!(matches!(
            IdentityFailure::transport(error),
            IdentityFailure::Timeout
        ));
        let mut c = config();
        c.base_url = endpoint.clone();
        // Production's fixed 10-second timeout must also bound the project probe.
        assert_eq!(
            check_project(&c, "safe-test-token").await.code,
            "project_timeout"
        );
        drop(listener);
        let error = client.get(&endpoint).send().await.unwrap_err();
        assert!(matches!(
            IdentityFailure::transport(error),
            IdentityFailure::Network
        ));
    }
}
