//! Foundry connections and stateless Responses service bindings.
use crate::{GatewayError, GatewayResult};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FoundryIdentityMethod {
    ClientSecret,
    WorkloadIdentity,
    ManagedIdentity,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FoundryIdentity {
    pub method: FoundryIdentityMethod,
    pub tenant_id: Option<Uuid>,
    pub client_id: Option<Uuid>,
}

impl FoundryIdentity {
    pub fn validate(&self, endpoint: &str, secret_present: bool) -> GatewayResult<()> {
        project_url(endpoint)?;
        if self.method != FoundryIdentityMethod::ManagedIdentity
            && (self.tenant_id.is_none() || self.client_id.is_none())
        {
            return Err(GatewayError::InvalidFoundryConfiguration);
        }
        if self.method == FoundryIdentityMethod::ClientSecret && !secret_present {
            return Err(GatewayError::InvalidFoundryConfiguration);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum FoundryBinding {
    RegisteredAgent {
        provider_id: Uuid,
        agent_name: String,
        #[serde(default)]
        agent_version: Option<String>,
    },
    EndpointPassthrough {
        provider_id: Uuid,
    },
}

impl FoundryBinding {
    pub fn provider_id(&self) -> Uuid {
        match self {
            Self::RegisteredAgent { provider_id, .. }
            | Self::EndpointPassthrough { provider_id } => *provider_id,
        }
    }

    pub fn validate(&self) -> GatewayResult<()> {
        if let Self::RegisteredAgent {
            agent_name,
            agent_version,
            ..
        } = self
        {
            if !valid_segment(agent_name)
                || agent_version.as_deref().is_some_and(|v| !valid_segment(v))
            {
                return Err(GatewayError::InvalidFoundryConfiguration);
            }
        }
        Ok(())
    }

    /// Expose only POST responses under the configured service route.
    pub fn validate_path(
        &self,
        method: &http::Method,
        uri: &http::Uri,
        route: &str,
    ) -> GatewayResult<()> {
        let expected = format!(
            "{}/responses",
            route.trim_end_matches('*').trim_end_matches('/')
        );
        if method != http::Method::POST || uri.path() != expected || uri.query().is_some() {
            return Err(GatewayError::InvalidFoundryRequest);
        }
        Ok(())
    }

    pub fn prepare_request(&self, body: &[u8]) -> GatewayResult<Vec<u8>> {
        let mut value: serde_json::Value =
            serde_json::from_slice(body).map_err(|_| GatewayError::InvalidFoundryRequest)?;
        let object = value
            .as_object_mut()
            .ok_or(GatewayError::InvalidFoundryRequest)?;
        // Responses and conversations are shared under the provider identity. Do not
        // permit callers to access persisted state or run work after admission ends.
        for name in ["conversation", "previous_response_id", "background"] {
            if object
                .get(name)
                .is_some_and(|v| !v.is_null() && v != &serde_json::Value::Bool(false))
            {
                return Err(GatewayError::InvalidFoundryRequest);
            }
            object.remove(name);
        }
        if object
            .get("store")
            .is_some_and(|v| v != &serde_json::Value::Bool(false))
        {
            return Err(GatewayError::InvalidFoundryRequest);
        }
        if !object.contains_key("input") {
            return Err(GatewayError::InvalidFoundryRequest);
        }
        // Only inline message history is accepted. Item IDs, file IDs and tool
        // results could refer to resources owned by another gateway caller.
        if let Some(input) = object.get("input") {
            validate_input(input)?;
        }
        if let Self::RegisteredAgent {
            agent_name,
            agent_version,
            ..
        } = self
        {
            // An allowlist prevents future API fields from overriding the definition.
            if object.keys().any(|key| {
                ![
                    "input",
                    "stream",
                    "store",
                    "metadata",
                    "max_output_tokens",
                    "guardrails",
                ]
                .contains(&key.as_str())
            }) {
                return Err(GatewayError::InvalidFoundryRequest);
            }
            let mut reference = serde_json::json!({"type":"agent_reference", "name":agent_name});
            if let Some(version) = agent_version {
                reference["version"] = version.clone().into();
            }
            object.insert("agent_reference".into(), reference);
        }
        object.insert("store".into(), false.into());
        serde_json::to_vec(&value).map_err(|_| GatewayError::InvalidFoundryRequest)
    }
}

fn validate_input(value: &serde_json::Value) -> GatewayResult<()> {
    if value.is_string() {
        return Ok(());
    }
    let messages = value
        .as_array()
        .ok_or(GatewayError::InvalidFoundryRequest)?;
    for message in messages {
        let item = message
            .as_object()
            .ok_or(GatewayError::InvalidFoundryRequest)?;
        if item
            .keys()
            .any(|key| !["type", "role", "content"].contains(&key.as_str()))
            || item.get("type").is_some_and(|v| v != "message")
            || !item
                .get("role")
                .is_some_and(|v| v == "user" || v == "assistant")
        {
            return Err(GatewayError::InvalidFoundryRequest);
        }
        let content = item
            .get("content")
            .ok_or(GatewayError::InvalidFoundryRequest)?;
        if content.is_string() {
            continue;
        }
        let parts = content
            .as_array()
            .ok_or(GatewayError::InvalidFoundryRequest)?;
        for part in parts {
            let p = part
                .as_object()
                .ok_or(GatewayError::InvalidFoundryRequest)?;
            if p.keys()
                .any(|key| !["type", "text"].contains(&key.as_str()))
                || !p
                    .get("type")
                    .is_some_and(|v| v == "input_text" || v == "output_text")
                || !p.get("text").is_some_and(serde_json::Value::is_string)
            {
                return Err(GatewayError::InvalidFoundryRequest);
            }
        }
    }
    Ok(())
}

pub fn project_url(endpoint: &str) -> GatewayResult<url::Url> {
    let url = url::Url::parse(endpoint).map_err(|_| GatewayError::InvalidFoundryConfiguration)?;
    let host = url.host_str().unwrap_or_default();
    let azure = host.ends_with(".services.ai.azure.com") && url.scheme() == "https";
    let loopback = (host == "127.0.0.1" || host == "[::1]") && url.scheme() == "http";
    let path: Vec<_> = url.path().trim_end_matches('/').split('/').collect();
    if (!azure && !loopback)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || path.len() != 4
        || path[1] != "api"
        || path[2] != "projects"
        || !valid_segment(path[3])
    {
        return Err(GatewayError::InvalidFoundryConfiguration);
    }
    Ok(url)
}

fn valid_segment(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
}

#[derive(Clone, PartialEq, Eq)]
pub struct FoundryRuntimeConfig {
    pub id: Uuid,
    pub base_url: String,
    pub identity: FoundryIdentity,
    pub secret: Option<String>,
    pub revision: i64,
}

impl std::fmt::Debug for FoundryRuntimeConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FoundryRuntimeConfig")
            .field("id", &self.id)
            .field("identity", &self.identity)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    fn binding() -> FoundryBinding {
        FoundryBinding::RegisteredAgent {
            provider_id: Uuid::nil(),
            agent_name: "research".into(),
            agent_version: Some("3".into()),
        }
    }
    #[test]
    fn configuration_validates_endpoints_and_identity() {
        let url = "https://account.services.ai.azure.com/api/projects/demo";
        let mut identity = FoundryIdentity {
            method: FoundryIdentityMethod::ClientSecret,
            tenant_id: Some(Uuid::nil()),
            client_id: Some(Uuid::nil()),
        };
        identity.validate(url, true).unwrap();
        assert!(identity.validate(url, false).is_err());
        identity.method = FoundryIdentityMethod::WorkloadIdentity;
        identity.validate(url, false).unwrap();
        identity.tenant_id = None;
        assert!(identity.validate(url, false).is_err());
        identity.method = FoundryIdentityMethod::ManagedIdentity;
        identity.client_id = None;
        identity.validate(url, false).unwrap();
        for url in [
            "bad",
            "https://evil.example/api/projects/demo",
            "http://account.services.ai.azure.com/api/projects/demo",
            "https://user@a.services.ai.azure.com/api/projects/demo",
            "https://a.services.ai.azure.com/api/projects/demo?q=x",
            "https://a.services.ai.azure.com/api/projects/demo#f",
            "https://a.services.ai.azure.com/api/projects/a%2Fb",
            "https://a.services.ai.azure.com/api/projects/demo/agents",
            "https://a.services.ai.azure.com/api/projects/",
            "https://a.services.ai.azure.com/bad/projects/demo",
            "https://a.services.ai.azure.com/api/other/demo",
        ] {
            assert!(project_url(url).is_err(), "{url}");
        }
        project_url("http://127.0.0.1:1234/api/projects/mock").unwrap();
        for name in ["", "a/b", "a b", &"a".repeat(129)] {
            let b = FoundryBinding::RegisteredAgent {
                provider_id: Uuid::nil(),
                agent_name: name.into(),
                agent_version: None,
            };
            assert!(b.validate().is_err());
        }
        let b = FoundryBinding::RegisteredAgent {
            provider_id: Uuid::nil(),
            agent_name: "a".into(),
            agent_version: Some("../1".into()),
        };
        assert!(b.validate().is_err());
        binding().validate().unwrap();
        assert_eq!(binding().provider_id(), Uuid::nil());
    }
    #[test]
    fn registered_agent_is_fixed_and_stateless_with_inline_history() {
        let b = binding();
        let output: Value = serde_json::from_slice(
            &b.prepare_request(br#"{"input":"hello","stream":true}"#)
                .unwrap(),
        )
        .unwrap();
        assert_eq!(
            output["agent_reference"],
            json!({"type":"agent_reference","name":"research","version":"3"})
        );
        assert_eq!(output["store"], false);
        for input in [
            json!("hi"),
            json!([]),
            json!([{"role":"user","content":"hi"},{"role":"assistant","type":"message","content":[{"type":"output_text","text":"hello"}]}]),
        ] {
            b.prepare_request(
                &serde_json::to_vec(
                    &json!({"input":input,"store":false,"background":false,"conversation":null}),
                )
                .unwrap(),
            )
            .unwrap();
        }
        for payload in [
            json!(null),
            json!([]),
            json!({}),
            json!({"input":1}),
            json!({"input":["x"]}),
            json!({"input":[{"role":"system","content":"override"}]}),
            json!({"input":[{"role":"user"}]}),
            json!({"input":[{"role":"user","content":42}]}),
            json!({"input":[{"role":"user","content":[1]}]}),
            json!({"input":[{"role":"user","content":[{"type":"input_image","image_url":"https://example.com"}]}]}),
            json!({"input":[{"type":"item_reference","id":"other"}]}),
            json!({"input":[{"role":"user","content":[{"type":"input_text","text":42}]}]}),
            json!({"input":[{"role":"user","type":"other","content":"x"}]}),
            json!({"input":[{"role":"user","content":[{"type":"other","text":"x"}]}]}),
        ] {
            assert!(
                b.prepare_request(&serde_json::to_vec(&payload).unwrap())
                    .is_err(),
                "{payload}"
            );
        }
        assert!(b.prepare_request(b"bad").is_err());
        for (name, value) in [
            ("store", json!(true)),
            ("conversation", json!("other")),
            ("previous_response_id", json!("other")),
            ("background", json!(true)),
            ("model", json!("other")),
            ("agent_reference", json!({})),
            ("instructions", json!("override")),
            ("tools", json!([])),
        ] {
            let mut v = json!({"input":"hi"});
            v[name] = value;
            assert!(b.prepare_request(&serde_json::to_vec(&v).unwrap()).is_err());
        }
        let unpinned = FoundryBinding::RegisteredAgent {
            provider_id: Uuid::nil(),
            agent_name: "agent".into(),
            agent_version: None,
        };
        let output: Value =
            serde_json::from_slice(&unpinned.prepare_request(br#"{"input":"hi"}"#).unwrap())
                .unwrap();
        assert!(output["agent_reference"].get("version").is_none());
    }
    #[test]
    fn passthrough_retains_definition_and_restricts_paths() {
        let b = FoundryBinding::EndpointPassthrough {
            provider_id: Uuid::nil(),
        };
        b.validate().unwrap();
        assert_eq!(b.provider_id(), Uuid::nil());
        let v: Value = serde_json::from_slice(
            &b.prepare_request(
                br#"{"model":"gpt-deployment","input":"hi","instructions":"help","tools":[]}"#,
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(v["model"], "gpt-deployment");
        assert_eq!(v["instructions"], "help");
        assert_eq!(v["store"], false);
        b.validate_path(
            &http::Method::POST,
            &"/services/demo/responses".parse().unwrap(),
            "/services/demo/*",
        )
        .unwrap();
        for (method, path) in [
            (http::Method::GET, "/services/demo/responses"),
            (http::Method::POST, "/services/demo/responses?id=other"),
            (http::Method::POST, "/services/demo/conversations"),
            (http::Method::POST, "/services/demo/responses/other"),
        ] {
            assert!(b
                .validate_path(&method, &path.parse().unwrap(), "/services/demo/*")
                .is_err());
        }
        let config = FoundryRuntimeConfig {
            id: Uuid::nil(),
            base_url: "x".into(),
            identity: FoundryIdentity {
                method: FoundryIdentityMethod::ManagedIdentity,
                tenant_id: None,
                client_id: None,
            },
            secret: Some("never-print-secret".into()),
            revision: 1,
        };
        assert!(!format!("{config:?}").contains("never-print-secret"));
    }
}
