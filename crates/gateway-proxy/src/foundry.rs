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
        let mut entries = self.entries.lock().await;
        if let Some(cached) = entries.get(&config.id) {
            if cached.revision == config.revision && cached.refresh_at > Instant::now() {
                return Ok(cached.token.clone());
            }
        }
        let (token, lifetime) = acquire_token(config).await?;
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
    let error = || GatewayError::FoundryCredentialUnavailable;
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
        let endpoint = token_endpoint(config, Uuid::nil())?;
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
        let endpoint = token_endpoint(config, tenant)?;
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
            let path = std::env::var("AZURE_FEDERATED_TOKEN_FILE").map_err(|_| error())?;
            let assertion = tokio::task::spawn_blocking(move || {
                use std::io::Read;
                let mut text = String::new();
                std::fs::File::open(path)?
                    .take(65_537)
                    .read_to_string(&mut text)?;
                Ok::<_, std::io::Error>(text)
            })
            .await
            .map_err(|_| error())?
            .map_err(|_| error())?;
            if assertion.trim().is_empty() || assertion.len() > 65_536 {
                return Err(error());
            }
            form.push((
                "client_assertion_type",
                "urn:ietf:params:oauth:client-assertion-type:jwt-bearer".into(),
            ));
            form.push(("client_assertion", assertion.trim().into()));
        }
        client.post(endpoint).form(&form)
    };
    let mut response = request.send().await.map_err(|_| error())?;
    if !response.status().is_success() {
        return Err(error());
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
        let path = std::env::temp_dir().join(format!("foundry-assertion-{}", Uuid::new_v4()));
        std::env::set_var("AZURE_FEDERATED_TOKEN_FILE", &path);
        assert!(acquire_token(&c).await.is_err());
        for content in ["".to_owned(), "x".repeat(65_537)] {
            std::fs::write(&path, content).unwrap();
            assert!(acquire_token(&c).await.is_err());
        }
        std::fs::write(&path, "mock-federated-assertion").unwrap();
        std::env::set_var(
            "GATEWAY_FOUNDRY_MOCK_TOKEN_ENDPOINT",
            mock_response(200, valid),
        );
        assert_eq!(acquire_token(&c).await.unwrap().0, "token");
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
}
