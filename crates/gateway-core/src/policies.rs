use crate::{GatewayError, GatewayResult, Provider, Route};
use async_trait::async_trait;
use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq)]
pub struct KeyPolicy {
    pub allowed_routes: Vec<Route>,
    pub allowed_models: Vec<String>,
    pub allowed_providers: Vec<Provider>,
    pub allowed_services: Vec<String>,
    pub rpm_limit: Option<i32>,
    pub tpm_limit: Option<i32>,
    pub daily_budget_usd: Option<f64>,
    pub monthly_budget_usd: Option<f64>,
    pub allow_streaming: bool,
    pub allow_tools: bool,
}

impl Default for KeyPolicy {
    fn default() -> Self {
        Self {
            allowed_routes: vec![Route::ChatCompletions, Route::Responses],
            allowed_models: Vec::new(),
            allowed_providers: vec![Provider::LiteLlm],
            allowed_services: Vec::new(),
            rpm_limit: None,
            tpm_limit: None,
            daily_budget_usd: None,
            monthly_budget_usd: None,
            allow_streaming: false,
            allow_tools: false,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GenerationFeatures {
    pub model: Option<String>,
    pub stream: bool,
    pub tools: bool,
    pub service_name: Option<String>,
}

#[async_trait]
pub trait PolicyLookup: Send + Sync {
    async fn policy_for_key(&self, key_id: Uuid) -> GatewayResult<KeyPolicy>;
}

#[async_trait]
impl<T> PolicyLookup for std::sync::Arc<T>
where
    T: PolicyLookup + ?Sized,
{
    async fn policy_for_key(&self, key_id: Uuid) -> GatewayResult<KeyPolicy> {
        (**self).policy_for_key(key_id).await
    }
}

pub fn evaluate_policy(
    policy: &KeyPolicy,
    route: Route,
    provider: Provider,
    features: &GenerationFeatures,
) -> GatewayResult<()> {
    if !policy.allowed_routes.is_empty() && !policy.allowed_routes.contains(&route) {
        return Err(GatewayError::PolicyDenied);
    }

    if !policy.allowed_providers.is_empty() && !policy.allowed_providers.contains(&provider) {
        return Err(GatewayError::PolicyDenied);
    }

    if let Some(service_name) = features.service_name.as_deref() {
        if !policy.allowed_services.is_empty()
            && !policy
                .allowed_services
                .iter()
                .any(|allowed_service| allowed_service == service_name)
        {
            return Err(GatewayError::PolicyDenied);
        }
    }

    if let Some(model) = features.model.as_deref() {
        if !policy.allowed_models.is_empty()
            && !policy
                .allowed_models
                .iter()
                .any(|allowed_model| allowed_model == model)
        {
            return Err(GatewayError::PolicyDenied);
        }
    }

    if features.stream && !policy.allow_streaming {
        return Err(GatewayError::PolicyDenied);
    }

    if features.tools && !policy.allow_tools {
        return Err(GatewayError::PolicyDenied);
    }

    Ok(())
}

pub fn extract_generation_features(body: &[u8]) -> GenerationFeatures {
    let Ok(value) = serde_json::from_slice::<Value>(body) else {
        return GenerationFeatures::default();
    };

    let model = value
        .get("model")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    let stream = value
        .get("stream")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let tools = value
        .get("tools")
        .and_then(Value::as_array)
        .is_some_and(|tools| !tools.is_empty());
    let service_name = value
        .get("service")
        .or_else(|| value.get("service_name"))
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);

    GenerationFeatures {
        model,
        stream,
        tools,
        service_name,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_default_phase_2_generation_policy() {
        let features = GenerationFeatures {
            model: Some("gpt-4.1-mini".to_owned()),
            ..GenerationFeatures::default()
        };

        evaluate_policy(
            &KeyPolicy::default(),
            Route::ChatCompletions,
            Provider::LiteLlm,
            &features,
        )
        .expect("allowed");
    }

    #[test]
    fn denies_disallowed_route_model_streaming_and_tools() {
        let policy = KeyPolicy {
            allowed_routes: vec![Route::Responses],
            allowed_models: vec!["allowed".to_owned()],
            allow_streaming: false,
            allow_tools: false,
            ..KeyPolicy::default()
        };

        assert_eq!(
            evaluate_policy(
                &policy,
                Route::ChatCompletions,
                Provider::LiteLlm,
                &GenerationFeatures::default(),
            )
            .unwrap_err(),
            GatewayError::PolicyDenied
        );

        assert_eq!(
            evaluate_policy(
                &KeyPolicy {
                    allowed_models: vec!["allowed".to_owned()],
                    ..KeyPolicy::default()
                },
                Route::Responses,
                Provider::LiteLlm,
                &GenerationFeatures {
                    model: Some("blocked".to_owned()),
                    ..GenerationFeatures::default()
                },
            )
            .unwrap_err(),
            GatewayError::PolicyDenied
        );

        assert_eq!(
            evaluate_policy(
                &KeyPolicy::default(),
                Route::Responses,
                Provider::LiteLlm,
                &GenerationFeatures {
                    stream: true,
                    ..GenerationFeatures::default()
                },
            )
            .unwrap_err(),
            GatewayError::PolicyDenied
        );

        assert_eq!(
            evaluate_policy(
                &KeyPolicy::default(),
                Route::Responses,
                Provider::LiteLlm,
                &GenerationFeatures {
                    tools: true,
                    ..GenerationFeatures::default()
                },
            )
            .unwrap_err(),
            GatewayError::PolicyDenied
        );

        assert_eq!(
            evaluate_policy(
                &KeyPolicy {
                    allowed_routes: vec![Route::Summary],
                    allowed_providers: vec![Provider::InternalService],
                    allowed_services: vec!["summary".to_owned()],
                    ..KeyPolicy::default()
                },
                Route::Summary,
                Provider::InternalService,
                &GenerationFeatures {
                    service_name: Some("translation".to_owned()),
                    ..GenerationFeatures::default()
                },
            )
            .unwrap_err(),
            GatewayError::PolicyDenied
        );
    }

    #[test]
    fn extracts_generation_features_without_logging_body() {
        let features = extract_generation_features(br#"{"model":"gpt-4.1-mini","stream":true,"tools":[{"type":"function"}],"service":"summary"}"#);

        assert_eq!(features.model.as_deref(), Some("gpt-4.1-mini"));
        assert!(features.stream);
        assert!(features.tools);
        assert_eq!(features.service_name.as_deref(), Some("summary"));
    }
}
