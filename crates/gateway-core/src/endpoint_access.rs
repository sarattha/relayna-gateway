//! Route-selected identity requirements and Accessa transport contracts.
use crate::{EntraIdentityContext, GatewayError, GatewayResult};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct EndpointAccess {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authentication_profiles: Option<crate::auth_profiles::AuthenticationProfiles>,
    /// Explicit key-only mode; absent overrides retain released authentication.
    pub skip_entra: bool,
    pub entra: Option<EndpointEntraPolicy>,
    pub accessa: Option<AccessaBinding>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EndpointEntraPolicy {
    pub audience: String,
    #[serde(default)]
    pub required_scopes: Vec<String>,
    #[serde(default)]
    pub required_roles: Vec<String>,
    #[serde(default)]
    pub allowed_groups: Vec<String>,
    #[serde(default)]
    pub allow_apigee: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AccessaBinding {
    /// None is the channel discovery endpoint, which cannot open run sockets.
    pub app: Option<String>,
    pub channel: String,
    pub idle_timeout_ms: u64,
    /// Per gateway process. The instance ceiling is shared by all bindings.
    pub max_connections: usize,
    pub max_connections_per_key: usize,
    pub max_frame_bytes: usize,
}

impl EndpointAccess {
    pub fn identity_for_key(
        &self,
        key_id: uuid::Uuid,
    ) -> GatewayResult<Option<&EndpointEntraPolicy>> {
        match &self.authentication_profiles {
            Some(profiles) => Ok(profiles.select(key_id)?.entra()),
            None => Ok(self.entra.as_ref()),
        }
    }

    pub fn validate(&self, route_pattern: &str) -> GatewayResult<()> {
        if let Some(profiles) = &self.authentication_profiles {
            profiles.validate(self.accessa.is_some())?;
            if self.skip_entra || self.entra.is_some() {
                return Err(GatewayError::InvalidServicePayload);
            }
        }
        if self.skip_entra && (self.entra.is_some() || self.accessa.is_some()) {
            return Err(GatewayError::InvalidServicePayload);
        }
        if let Some(policy) = &self.entra {
            if !valid_claim(&policy.audience)
                || policy
                    .required_scopes
                    .iter()
                    .chain(&policy.required_roles)
                    .chain(&policy.allowed_groups)
                    .any(|value| !valid_claim(value))
            {
                return Err(GatewayError::InvalidServicePayload);
            }
        }
        if let Some(binding) = &self.accessa {
            if (self.entra.is_none() && self.authentication_profiles.is_none())
                || !valid_segment(&binding.channel)
                || binding
                    .app
                    .as_deref()
                    .is_some_and(|app| !valid_segment(app))
                || !(100..=600_000).contains(&binding.idle_timeout_ms)
                || !(1..=10_000).contains(&binding.max_connections)
                || !(1..=binding.max_connections).contains(&binding.max_connections_per_key)
                || !(125..=16_777_216).contains(&binding.max_frame_bytes)
                || route_pattern != binding.route_pattern()
            {
                return Err(GatewayError::InvalidServicePayload);
            }
        }
        Ok(())
    }
}

impl AccessaBinding {
    pub fn route_pattern(&self) -> String {
        match &self.app {
            Some(app) => format!("/app/{app}/channel/{}/v1/*", self.channel),
            None => format!("/channel/{}/v1/me", self.channel),
        }
    }

    pub fn run_path(&self) -> Option<String> {
        self.app
            .as_ref()
            .map(|app| format!("/app/{app}/channel/{}/v1/run", self.channel))
    }
}

impl EndpointEntraPolicy {
    /// Check claims only after JWT signature or Apigee HMAC verification.
    pub fn authorize(&self, identity: &EntraIdentityContext, now: i64) -> GatewayResult<()> {
        if !identity.audiences.iter().any(|aud| aud == &self.audience) {
            return Err(GatewayError::InvalidEntraAudience);
        }
        if identity.expires_at.is_none_or(|exp| exp <= now) {
            return Err(GatewayError::ExpiredEntraToken);
        }
        if self
            .required_scopes
            .iter()
            .any(|scope| !identity.scopes.contains(scope))
            || self
                .required_roles
                .iter()
                .any(|role| !identity.roles.contains(role))
            || (!self.allowed_groups.is_empty()
                && !self
                    .allowed_groups
                    .iter()
                    .any(|group| identity.groups.contains(group)))
        {
            return Err(GatewayError::InsufficientEntraAuthorization);
        }
        Ok(())
    }
}

fn valid_claim(value: &str) -> bool {
    !value.is_empty() && value.len() <= 512 && !value.chars().any(char::is_whitespace)
}

fn valid_segment(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn access() -> EndpointAccess {
        serde_json::from_value(json!({"entra":{"audience":"api://accessa","required_scopes":["run"],"required_roles":["invoke"],"allowed_groups":["staff"]},
            "accessa":{"app":"tara","channel":"web","idle_timeout_ms":60000,"max_connections":10,"max_connections_per_key":2,"max_frame_bytes":1024}})).unwrap()
    }
    fn identity() -> EntraIdentityContext {
        serde_json::from_value(json!({"tenant_id":"tenant","subject":null,"object_id":"user","app_id":null,"authorized_party":null,
            "email":null,"display_name":null,"nonce":null,"scopes":["run"],"roles":["invoke"],"groups":["staff"],"token_version":"2.0","source":"jwt",
            "audiences":["api://accessa"],"expires_at":200})).unwrap()
    }
    #[test]
    fn validate_bindings_and_claims() {
        assert!(EndpointAccess::default().validate("/legacy/*").is_ok());
        let good = access();
        assert!(good.validate("/app/tara/channel/web/v1/*").is_ok());
        assert!(good.validate("/app/docgen/channel/web/v1/*").is_err());
        let binding = good.accessa.as_ref().unwrap();
        assert_eq!(binding.run_path().unwrap(), "/app/tara/channel/web/v1/run");
        let mut discovery = good.clone();
        discovery.accessa.as_mut().unwrap().app = None;
        assert!(discovery.validate("/channel/web/v1/me").is_ok());
        assert_eq!(discovery.accessa.unwrap().run_path(), None);
        let mutations: Vec<fn(&mut EndpointAccess)> = vec![
            |v| v.entra = None,
            |v| v.entra.as_mut().unwrap().audience.clear(),
            |v| v.entra.as_mut().unwrap().audience = "x".repeat(513),
            |v| {
                v.entra
                    .as_mut()
                    .unwrap()
                    .required_scopes
                    .push("bad scope".into())
            },
            |v| v.accessa.as_mut().unwrap().channel = "../web".into(),
            |v| v.accessa.as_mut().unwrap().app = Some(String::new()),
            |v| v.accessa.as_mut().unwrap().idle_timeout_ms = 0,
            |v| v.accessa.as_mut().unwrap().max_connections = 0,
            |v| v.accessa.as_mut().unwrap().max_connections_per_key = 11,
            |v| v.accessa.as_mut().unwrap().max_frame_bytes = 124,
        ];
        for mutate in mutations {
            let mut value = good.clone();
            mutate(&mut value);
            assert!(value.validate("/app/tara/channel/web/v1/*").is_err());
        }
        assert!(!valid_segment(&"x".repeat(65)));
        assert!(valid_segment("valid_1-2"));
        let policy = good.entra.unwrap();
        assert!(policy.authorize(&identity(), 100).is_ok());
        for (field, value, expected) in [
            (
                "audiences",
                json!(["other"]),
                GatewayError::InvalidEntraAudience,
            ),
            ("expires_at", json!(100), GatewayError::ExpiredEntraToken),
            ("expires_at", json!(null), GatewayError::ExpiredEntraToken),
            (
                "scopes",
                json!([]),
                GatewayError::InsufficientEntraAuthorization,
            ),
            (
                "roles",
                json!([]),
                GatewayError::InsufficientEntraAuthorization,
            ),
            (
                "groups",
                json!([]),
                GatewayError::InsufficientEntraAuthorization,
            ),
        ] {
            let mut claims = serde_json::to_value(identity()).unwrap();
            claims[field] = value;
            assert_eq!(
                policy.authorize(&serde_json::from_value(claims).unwrap(), 100),
                Err(expected)
            );
        }
        let policy = EndpointEntraPolicy {
            required_scopes: vec![],
            required_roles: vec![],
            allowed_groups: vec![],
            ..policy
        };
        assert!(policy.authorize(&identity(), 100).is_ok());
        assert!(serde_json::from_value::<EndpointAccess>(json!({"unknown":true})).is_err());
    }
}

/// Canonical request-plane routes. Aliases resolve to the same entry.
pub const IDENTITY_ROUTES: &[crate::Route] = &[
    crate::Route::ChatCompletions,
    crate::Route::Responses,
    crate::Route::LiteLlmEmbeddings,
    crate::Route::LiteLlmRerank,
    crate::Route::AnthropicMessages,
    crate::Route::AnthropicMessagesCountTokens,
    crate::Route::AnthropicMessageBatches,
    crate::Route::AnthropicMessageBatch,
    crate::Route::AnthropicMessageBatchResults,
    crate::Route::AnthropicMessageBatchCancel,
    crate::Route::AnthropicModels,
    crate::Route::DirectOpenAi,
    crate::Route::LiteLlmPassthrough,
    crate::Route::Summary,
    crate::Route::Translation,
    crate::Route::Ocr,
    crate::Route::ServiceWildcard,
];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouteIdentitySetting {
    pub route: String,
    pub access: EndpointAccess,
}
impl RouteIdentitySetting {
    pub fn validate(&self) -> GatewayResult<()> {
        if !IDENTITY_ROUTES
            .iter()
            .any(|route| route.as_str() == self.route)
            || self.access.accessa.is_some()
        {
            return Err(GatewayError::InvalidServicePayload);
        }
        self.access.validate(&self.route)
    }
}

#[cfg(test)]
mod identity_tests {
    use super::*;
    #[test]
    fn explicit_identity_modes_validate_and_round_trip() {
        for route in IDENTITY_ROUTES {
            for access in [
                serde_json::json!({}),
                serde_json::json!({"skip_entra":true}),
                serde_json::json!({"entra":{"audience":"api://service"}}),
            ] {
                let setting: RouteIdentitySetting = serde_json::from_value(
                    serde_json::json!({"route":route.as_str(),"access":access}),
                )
                .unwrap();
                setting.validate().unwrap();
                assert_eq!(
                    serde_json::from_value::<RouteIdentitySetting>(
                        serde_json::to_value(&setting).unwrap()
                    )
                    .unwrap()
                    .access,
                    setting.access
                );
            }
        }
        for value in [
            serde_json::json!({"route":"/unknown","access":{}}),
            serde_json::json!({"route":"/v1/chat/completions","access":{"entra":{"audience":""}}}),
            serde_json::json!({"route":"/v1/chat/completions","access":{"skip_entra":true,"entra":{"audience":"api://service"}}}),
            serde_json::json!({"route":"/v1/chat/completions","access":{"accessa":{"app":"tara","channel":"web","idle_timeout_ms":1000,"max_connections":1,"max_connections_per_key":1,"max_frame_bytes":125}}}),
        ] {
            assert!(serde_json::from_value::<RouteIdentitySetting>(value)
                .unwrap()
                .validate()
                .is_err());
        }
        assert!(serde_json::from_value::<EndpointAccess>(serde_json::json!({"skip_entra":true,"accessa":{"app":"tara","channel":"web","idle_timeout_ms":1000,"max_connections":1,"max_connections_per_key":1,"max_frame_bytes":125}})).unwrap().validate("/app/tara/channel/web/v1/*").is_err());
    }
}
