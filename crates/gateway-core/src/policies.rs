use crate::{GatewayError, GatewayResult, GuardrailPolicy, Provider, Route};
use async_trait::async_trait;
use chrono::{DateTime, Timelike, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KeyPolicy {
    #[serde(default)]
    pub deny: bool,
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
    pub max_requests_per_day: Option<i32>,
    pub max_tokens_per_day: Option<i32>,
    pub max_cost_per_request: Option<f64>,
    pub max_input_tokens_per_request: Option<i32>,
    pub max_output_tokens_per_request: Option<i32>,
    pub allowed_hours_utc: Vec<i32>,
    pub unused_key_auto_disable_after_days: Option<i32>,
    pub max_request_body_bytes: Option<i64>,
    pub max_response_body_bytes: Option<i64>,
    pub max_stream_duration_seconds: Option<i32>,
    pub max_sse_event_bytes: Option<i64>,
    pub max_tool_call_count: Option<i32>,
    pub max_tool_schema_bytes: Option<i64>,
    pub policy_version: i64,
}

impl Default for KeyPolicy {
    fn default() -> Self {
        Self {
            deny: false,
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
            max_requests_per_day: None,
            max_tokens_per_day: None,
            max_cost_per_request: None,
            max_input_tokens_per_request: None,
            max_output_tokens_per_request: None,
            allowed_hours_utc: Vec::new(),
            unused_key_auto_disable_after_days: None,
            max_request_body_bytes: None,
            max_response_body_bytes: None,
            max_stream_duration_seconds: None,
            max_sse_event_bytes: None,
            max_tool_call_count: None,
            max_tool_schema_bytes: None,
            policy_version: 1,
        }
    }
}

impl KeyPolicy {
    pub fn neutral_layer(policy_version: i64) -> Self {
        Self {
            deny: false,
            allowed_routes: Vec::new(),
            allowed_models: Vec::new(),
            allowed_providers: Vec::new(),
            allowed_services: Vec::new(),
            rpm_limit: None,
            tpm_limit: None,
            daily_budget_usd: None,
            monthly_budget_usd: None,
            allow_streaming: true,
            allow_tools: true,
            max_requests_per_day: None,
            max_tokens_per_day: None,
            max_cost_per_request: None,
            max_input_tokens_per_request: None,
            max_output_tokens_per_request: None,
            allowed_hours_utc: Vec::new(),
            unused_key_auto_disable_after_days: None,
            max_request_body_bytes: None,
            max_response_body_bytes: None,
            max_stream_duration_seconds: None,
            max_sse_event_bytes: None,
            max_tool_call_count: None,
            max_tool_schema_bytes: None,
            policy_version,
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

    async fn effective_policy_for_context(
        &self,
        key_id: Uuid,
        project_id: Option<Uuid>,
        team_id: Option<String>,
        route: Option<Route>,
        model: Option<String>,
    ) -> GatewayResult<EffectivePolicy> {
        Ok(EffectivePolicy {
            policy: self
                .policy_for_context(key_id, project_id, team_id, route, model)
                .await?,
            guardrail_policy: GuardrailPolicy::default(),
            applied_layers: Vec::new(),
        })
    }

    async fn policy_for_context(
        &self,
        key_id: Uuid,
        _project_id: Option<Uuid>,
        _team_id: Option<String>,
        _route: Option<Route>,
        _model: Option<String>,
    ) -> GatewayResult<KeyPolicy> {
        self.policy_for_key(key_id).await
    }
}

#[async_trait]
impl<T> PolicyLookup for std::sync::Arc<T>
where
    T: PolicyLookup + ?Sized,
{
    async fn policy_for_key(&self, key_id: Uuid) -> GatewayResult<KeyPolicy> {
        (**self).policy_for_key(key_id).await
    }

    async fn policy_for_context(
        &self,
        key_id: Uuid,
        project_id: Option<Uuid>,
        team_id: Option<String>,
        route: Option<Route>,
        model: Option<String>,
    ) -> GatewayResult<KeyPolicy> {
        (**self)
            .policy_for_context(key_id, project_id, team_id, route, model)
            .await
    }

    async fn effective_policy_for_context(
        &self,
        key_id: Uuid,
        project_id: Option<Uuid>,
        team_id: Option<String>,
        route: Option<Route>,
        model: Option<String>,
    ) -> GatewayResult<EffectivePolicy> {
        (**self)
            .effective_policy_for_context(key_id, project_id, team_id, route, model)
            .await
    }
}

pub fn evaluate_policy(
    policy: &KeyPolicy,
    route: Route,
    provider: Provider,
    features: &GenerationFeatures,
) -> GatewayResult<()> {
    if policy.deny {
        return Err(GatewayError::PolicyDenied);
    }

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

pub fn evaluate_policy_limits(
    policy: &KeyPolicy,
    now: DateTime<Utc>,
    request_body_bytes: Option<i64>,
    response_body_bytes: Option<i64>,
    input_tokens: Option<i32>,
    output_tokens: Option<i32>,
    estimated_cost_usd: Option<f64>,
) -> GatewayResult<()> {
    if !policy.allowed_hours_utc.is_empty()
        && !policy
            .allowed_hours_utc
            .iter()
            .any(|hour| *hour == i32::try_from(now.hour()).unwrap_or_default())
    {
        return Err(GatewayError::PolicyDenied);
    }

    if exceeds_i64(request_body_bytes, policy.max_request_body_bytes) {
        return Err(GatewayError::RequestBodyTooLarge);
    }
    if exceeds_i64(response_body_bytes, policy.max_response_body_bytes) {
        return Err(GatewayError::ResponseBodyTooLarge);
    }
    if exceeds_i32(input_tokens, policy.max_input_tokens_per_request)
        || exceeds_i32(output_tokens, policy.max_output_tokens_per_request)
    {
        return Err(GatewayError::PolicyDenied);
    }
    if let (Some(cost), Some(limit)) = (estimated_cost_usd, policy.max_cost_per_request) {
        if cost > limit {
            return Err(GatewayError::PolicyDenied);
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyLayerKind {
    Global,
    Project,
    Team,
    Key,
    Route,
    Model,
}

impl PolicyLayerKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Global => "global",
            Self::Project => "project",
            Self::Team => "team",
            Self::Key => "key",
            Self::Route => "route",
            Self::Model => "model",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PolicyLayer {
    pub kind: PolicyLayerKind,
    pub scope_id: Option<String>,
    pub policy: KeyPolicy,
    pub guardrail_policy: GuardrailPolicy,
    pub policy_version: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PolicyLayerTrace {
    pub kind: PolicyLayerKind,
    pub scope_id: Option<String>,
    pub policy_version: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EffectivePolicy {
    pub policy: KeyPolicy,
    pub guardrail_policy: GuardrailPolicy,
    pub applied_layers: Vec<PolicyLayerTrace>,
}

pub fn resolve_effective_policy(layers: Vec<PolicyLayer>) -> GatewayResult<EffectivePolicy> {
    let mut iter = layers.into_iter();
    let Some(first) = iter.next() else {
        return Ok(EffectivePolicy {
            policy: KeyPolicy::default(),
            guardrail_policy: GuardrailPolicy::default(),
            applied_layers: Vec::new(),
        });
    };

    let mut effective_policy = first.policy;
    let mut effective_guardrails = first.guardrail_policy;
    let mut applied_layers = vec![PolicyLayerTrace {
        kind: first.kind,
        scope_id: first.scope_id,
        policy_version: first.policy_version,
    }];

    for layer in iter {
        merge_policy(&mut effective_policy, &layer.policy);
        merge_guardrail_policy(&mut effective_guardrails, &layer.guardrail_policy)?;
        applied_layers.push(PolicyLayerTrace {
            kind: layer.kind,
            scope_id: layer.scope_id,
            policy_version: layer.policy_version,
        });
    }

    Ok(EffectivePolicy {
        policy: effective_policy,
        guardrail_policy: effective_guardrails,
        applied_layers,
    })
}

fn merge_policy(effective: &mut KeyPolicy, next: &KeyPolicy) {
    effective.deny = effective.deny || next.deny;
    merge_allowlist(
        &mut effective.allowed_routes,
        &next.allowed_routes,
        &mut effective.deny,
    );
    merge_allowlist(
        &mut effective.allowed_models,
        &next.allowed_models,
        &mut effective.deny,
    );
    merge_allowlist(
        &mut effective.allowed_providers,
        &next.allowed_providers,
        &mut effective.deny,
    );
    merge_allowlist(
        &mut effective.allowed_services,
        &next.allowed_services,
        &mut effective.deny,
    );
    effective.rpm_limit = stricter_i32(effective.rpm_limit, next.rpm_limit);
    effective.tpm_limit = stricter_i32(effective.tpm_limit, next.tpm_limit);
    effective.daily_budget_usd = stricter_f64(effective.daily_budget_usd, next.daily_budget_usd);
    effective.monthly_budget_usd =
        stricter_f64(effective.monthly_budget_usd, next.monthly_budget_usd);
    effective.allow_streaming = effective.allow_streaming && next.allow_streaming;
    effective.allow_tools = effective.allow_tools && next.allow_tools;
    effective.max_requests_per_day =
        stricter_i32(effective.max_requests_per_day, next.max_requests_per_day);
    effective.max_tokens_per_day =
        stricter_i32(effective.max_tokens_per_day, next.max_tokens_per_day);
    effective.max_cost_per_request =
        stricter_f64(effective.max_cost_per_request, next.max_cost_per_request);
    effective.max_input_tokens_per_request = stricter_i32(
        effective.max_input_tokens_per_request,
        next.max_input_tokens_per_request,
    );
    effective.max_output_tokens_per_request = stricter_i32(
        effective.max_output_tokens_per_request,
        next.max_output_tokens_per_request,
    );
    merge_allowed_hours(
        &mut effective.allowed_hours_utc,
        &next.allowed_hours_utc,
        &mut effective.deny,
    );
    effective.unused_key_auto_disable_after_days = stricter_i32(
        effective.unused_key_auto_disable_after_days,
        next.unused_key_auto_disable_after_days,
    );
    effective.max_request_body_bytes = stricter_i64(
        effective.max_request_body_bytes,
        next.max_request_body_bytes,
    );
    effective.max_response_body_bytes = stricter_i64(
        effective.max_response_body_bytes,
        next.max_response_body_bytes,
    );
    effective.max_stream_duration_seconds = stricter_i32(
        effective.max_stream_duration_seconds,
        next.max_stream_duration_seconds,
    );
    effective.max_sse_event_bytes =
        stricter_i64(effective.max_sse_event_bytes, next.max_sse_event_bytes);
    effective.max_tool_call_count =
        stricter_i32(effective.max_tool_call_count, next.max_tool_call_count);
    effective.max_tool_schema_bytes =
        stricter_i64(effective.max_tool_schema_bytes, next.max_tool_schema_bytes);
    effective.policy_version = effective.policy_version.max(next.policy_version);
}

fn merge_guardrail_policy(
    effective: &mut GuardrailPolicy,
    next: &GuardrailPolicy,
) -> GatewayResult<()> {
    append_unique(
        &mut effective.mandatory_guardrails,
        &next.mandatory_guardrails,
    );
    append_unique(
        &mut effective.optional_guardrails,
        &next.optional_guardrails,
    );
    append_unique(
        &mut effective.forbidden_guardrails,
        &next.forbidden_guardrails,
    );
    effective.optional_guardrails.retain(|name| {
        !effective
            .forbidden_guardrails
            .iter()
            .any(|forbidden| forbidden == name)
    });
    for (name, value) in &next.guardrail_config_overrides {
        effective
            .guardrail_config_overrides
            .insert(name.clone(), value.clone());
    }
    effective.validate()
}

fn merge_allowlist<T>(effective: &mut Vec<T>, next: &[T], deny: &mut bool)
where
    T: Clone + Eq + Ord,
{
    if next.is_empty() {
        return;
    }
    if effective.is_empty() {
        *effective = next.to_vec();
        return;
    }
    let next_set = next.iter().collect::<BTreeSet<_>>();
    effective.retain(|value| next_set.contains(value));
    if effective.is_empty() {
        *deny = true;
    }
}

fn merge_allowed_hours(effective: &mut Vec<i32>, next: &[i32], deny: &mut bool) {
    let normalized_next = valid_hours(next);
    if normalized_next.is_empty() {
        return;
    }
    if effective.is_empty() {
        *effective = normalized_next;
        return;
    }
    let next_set = normalized_next.iter().collect::<BTreeSet<_>>();
    effective.retain(|value| next_set.contains(value));
    if effective.is_empty() {
        *deny = true;
    }
}

fn valid_hours(hours: &[i32]) -> Vec<i32> {
    let mut values = hours
        .iter()
        .copied()
        .filter(|hour| (0..=23).contains(hour))
        .collect::<Vec<_>>();
    values.sort_unstable();
    values.dedup();
    values
}

fn append_unique(target: &mut Vec<String>, values: &[String]) {
    for value in values {
        if !target.iter().any(|current| current == value) {
            target.push(value.clone());
        }
    }
}

fn stricter_i32(current: Option<i32>, next: Option<i32>) -> Option<i32> {
    match (current, next) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    }
}

fn stricter_i64(current: Option<i64>, next: Option<i64>) -> Option<i64> {
    match (current, next) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    }
}

fn stricter_f64(current: Option<f64>, next: Option<f64>) -> Option<f64> {
    match (current, next) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    }
}

fn exceeds_i64(value: Option<i64>, limit: Option<i64>) -> bool {
    matches!((value, limit), (Some(value), Some(limit)) if value > limit)
}

fn exceeds_i32(value: Option<i32>, limit: Option<i32>) -> bool {
    matches!((value, limit), (Some(value), Some(limit)) if value > limit)
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

    #[test]
    fn effective_policy_intersects_allowlists_and_strictens_limits() {
        let effective = resolve_effective_policy(vec![
            PolicyLayer {
                kind: PolicyLayerKind::Global,
                scope_id: None,
                policy: KeyPolicy {
                    allowed_models: vec!["gpt-4.1-mini".to_owned(), "gpt-4.1".to_owned()],
                    allow_streaming: true,
                    rpm_limit: Some(100),
                    daily_budget_usd: Some(50.0),
                    max_request_body_bytes: Some(4096),
                    allowed_hours_utc: vec![8, 9, 10],
                    policy_version: 3,
                    ..KeyPolicy::default()
                },
                guardrail_policy: GuardrailPolicy {
                    mandatory_guardrails: vec!["pii-redact".to_owned()],
                    optional_guardrails: vec!["toxicity".to_owned()],
                    ..GuardrailPolicy::default()
                },
                policy_version: 3,
            },
            PolicyLayer {
                kind: PolicyLayerKind::Key,
                scope_id: Some("key-1".to_owned()),
                policy: KeyPolicy {
                    allowed_models: vec!["gpt-4.1-mini".to_owned()],
                    allow_streaming: false,
                    rpm_limit: Some(25),
                    daily_budget_usd: Some(10.0),
                    max_request_body_bytes: Some(1024),
                    allowed_hours_utc: vec![9, 10, 11],
                    policy_version: 7,
                    ..KeyPolicy::default()
                },
                guardrail_policy: GuardrailPolicy {
                    forbidden_guardrails: vec!["toxicity".to_owned()],
                    ..GuardrailPolicy::default()
                },
                policy_version: 7,
            },
        ])
        .expect("effective policy");

        assert_eq!(effective.policy.allowed_models, vec!["gpt-4.1-mini"]);
        assert_eq!(effective.policy.rpm_limit, Some(25));
        assert_eq!(effective.policy.daily_budget_usd, Some(10.0));
        assert_eq!(effective.policy.max_request_body_bytes, Some(1024));
        assert_eq!(effective.policy.allowed_hours_utc, vec![9, 10]);
        assert!(!effective.policy.allow_streaming);
        assert_eq!(effective.policy.policy_version, 7);
        assert_eq!(
            effective.guardrail_policy.mandatory_guardrails,
            vec!["pii-redact"]
        );
        assert!(effective.guardrail_policy.optional_guardrails.is_empty());
        assert_eq!(
            effective.guardrail_policy.forbidden_guardrails,
            vec!["toxicity"]
        );
    }

    #[test]
    fn effective_policy_denies_on_disjoint_allowlist_or_explicit_deny() {
        let effective = resolve_effective_policy(vec![
            PolicyLayer {
                kind: PolicyLayerKind::Global,
                scope_id: None,
                policy: KeyPolicy {
                    allowed_models: vec!["global-only".to_owned()],
                    ..KeyPolicy::default()
                },
                guardrail_policy: GuardrailPolicy::default(),
                policy_version: 1,
            },
            PolicyLayer {
                kind: PolicyLayerKind::Model,
                scope_id: Some("local".to_owned()),
                policy: KeyPolicy {
                    allowed_models: vec!["model-only".to_owned()],
                    ..KeyPolicy::default()
                },
                guardrail_policy: GuardrailPolicy::default(),
                policy_version: 2,
            },
        ])
        .expect("effective policy");

        assert!(effective.policy.deny);
        assert_eq!(
            evaluate_policy(
                &effective.policy,
                Route::ChatCompletions,
                Provider::LiteLlm,
                &GenerationFeatures::default(),
            )
            .unwrap_err(),
            GatewayError::PolicyDenied
        );
    }

    #[test]
    fn policy_limits_return_stable_size_errors() {
        let policy = KeyPolicy {
            max_request_body_bytes: Some(10),
            max_response_body_bytes: Some(20),
            allowed_hours_utc: vec![0],
            ..KeyPolicy::default()
        };
        let now = DateTime::parse_from_rfc3339("2026-05-23T01:00:00Z")
            .expect("time")
            .with_timezone(&Utc);

        assert_eq!(
            evaluate_policy_limits(&policy, now, Some(11), None, None, None, None).unwrap_err(),
            GatewayError::PolicyDenied
        );

        let now = DateTime::parse_from_rfc3339("2026-05-23T00:00:00Z")
            .expect("time")
            .with_timezone(&Utc);
        assert_eq!(
            evaluate_policy_limits(&policy, now, Some(11), None, None, None, None).unwrap_err(),
            GatewayError::RequestBodyTooLarge
        );
        assert_eq!(
            evaluate_policy_limits(&policy, now, None, Some(21), None, None, None).unwrap_err(),
            GatewayError::ResponseBodyTooLarge
        );
    }
}
