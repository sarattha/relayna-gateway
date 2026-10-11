use crate::{GatewayError, GatewayResult, GuardrailPolicy, Provider, Route};
use async_trait::async_trait;
use chrono::{DateTime, Timelike, Utc};
use serde::{de::IgnoredAny, Deserialize, Serialize};
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

#[derive(Debug, Clone, Default, PartialEq)]
pub struct GenerationRequestAnalysis {
    pub features: GenerationFeatures,
    pub client_guardrails: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct GenerationRequestProbe {
    model: Option<String>,
    #[serde(default)]
    stream: bool,
    tools: Option<Vec<IgnoredAny>>,
    functions: Option<Vec<IgnoredAny>>,
    function_call: Option<Value>,
    tool_choice: Option<Value>,
    service: Option<String>,
    service_name: Option<String>,
    guardrails: Option<Value>,
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

#[derive(Debug, Default, Clone, Copy)]
enum OutputMaximum {
    #[default]
    Missing,
    Tokens(u64),
}

impl<'de> Deserialize<'de> for OutputMaximum {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        u64::deserialize(deserializer).map(Self::Tokens)
    }
}

#[derive(Deserialize)]
struct OutputMaximumProbe {
    #[serde(default)]
    max_tokens: OutputMaximum,
    #[serde(default)]
    max_completion_tokens: OutputMaximum,
    #[serde(default)]
    max_output_tokens: OutputMaximum,
    #[serde(default)]
    n: OutputMaximum,
}

/// Enforce a provider-recognized generation bound before any paid work. A
/// response-time usage check cannot undo generation or delivered stream chunks.
pub fn enforce_output_token_limit(
    policy: &KeyPolicy,
    route: Route,
    body: &[u8],
) -> GatewayResult<()> {
    reject_provider_parameter_overrides(policy, route, body)?;
    let Some(limit) = policy.max_output_tokens_per_request else {
        return Ok(());
    };
    if matches!(
        route,
        Route::LiteLlmEmbeddings
            | Route::LiteLlmRerank
            | Route::AnthropicMessagesCountTokens
            | Route::AnthropicModels
    ) {
        return Ok(());
    }
    let limit = u64::try_from(limit).map_err(|_| GatewayError::PolicyDenied)?;
    let maximum = if route == Route::AnthropicMessageBatches {
        #[derive(Deserialize)]
        struct Batch {
            requests: Vec<Entry>,
        }
        #[derive(Deserialize)]
        struct Entry {
            params: Box<serde_json::value::RawValue>,
        }
        let batch: Batch = serde_json::from_slice(body).map_err(|_| GatewayError::PolicyDenied)?;
        if batch.requests.is_empty() {
            return Err(GatewayError::PolicyDenied);
        }
        batch.requests.iter().try_fold(0u64, |total, entry| {
            total
                .checked_add(provider_output_maximum(
                    Route::AnthropicMessages,
                    entry.params.get().as_bytes(),
                )?)
                .ok_or(GatewayError::PolicyDenied)
        })?
    } else {
        provider_output_maximum(route, body)?
    };
    if maximum > limit {
        return Err(GatewayError::PolicyDenied);
    }
    Ok(())
}

fn reject_provider_parameter_overrides(
    policy: &KeyPolicy,
    route: Route,
    body: &[u8],
) -> GatewayResult<()> {
    if route == Route::AnthropicMessageBatches {
        #[derive(Deserialize)]
        struct Batch {
            requests: Vec<Entry>,
        }
        #[derive(Deserialize)]
        struct Entry {
            params: Box<serde_json::value::RawValue>,
        }
        let batch: Batch = serde_json::from_slice(body).map_err(|_| GatewayError::PolicyDenied)?;
        for entry in batch.requests {
            reject_provider_parameter_overrides(
                policy,
                Route::AnthropicMessages,
                entry.params.get().as_bytes(),
            )?;
        }
        return Ok(());
    }
    let wildcard = route == Route::LiteLlmPassthrough;
    if wildcard && !serde_json::from_slice::<Value>(body).is_ok_and(|value| value.is_object()) {
        return Ok(());
    }
    if !wildcard
        && !matches!(
            route,
            Route::ChatCompletions
                | Route::Responses
                | Route::AnthropicMessages
                | Route::LiteLlmEmbeddings
                | Route::LiteLlmRerank
        )
    {
        return Ok(());
    }
    #[derive(Deserialize)]
    struct Probe {
        extra_body: Option<serde_json::Map<String, Value>>,
    }
    let probe: Probe = serde_json::from_slice(body).map_err(|_| GatewayError::PolicyDenied)?;
    let Some(extra) = probe.extra_body else {
        return Ok(());
    };
    // A model override bypasses selecting that model's policy layers, even
    // when the validated top-level model has no restrictions.
    if extra.contains_key("model") {
        return Err(GatewayError::PolicyDenied);
    }
    if wildcard {
        // Other wildcard request semantics remain delegated. The model must
        // still agree with the selector used to load model-specific caps.
        return Ok(());
    }
    // LiteLLM may merge provider-specific extra_body after validated top-level
    // parameters. Unknown extensions cannot prove an output bound; feature
    // selectors must not override a denial after gateway validation either.
    if (policy.max_output_tokens_per_request.is_some()
        && matches!(
            route,
            Route::ChatCompletions | Route::Responses | Route::AnthropicMessages
        )
        && !extra.is_empty())
        || (!policy.allow_streaming && extra.contains_key("stream"))
        || (!policy.allow_tools
            && ["tools", "functions", "tool_choice", "function_call"]
                .iter()
                .any(|field| extra.contains_key(*field)))
    {
        return Err(GatewayError::PolicyDenied);
    }
    Ok(())
}

fn provider_output_maximum(route: Route, body: &[u8]) -> GatewayResult<u64> {
    // LiteLLM also promotes unknown top-level provider parameters into
    // extra_body. Under a cap, permit only the supported protocol fields so
    // aliases such as max_new_tokens cannot override a recognized maximum.
    let allowed: &[&str] = match route {
        Route::ChatCompletions => &[
            "model",
            "messages",
            "stream",
            "stream_options",
            "max_tokens",
            "max_completion_tokens",
            "max_output_tokens",
            "n",
            "temperature",
            "top_p",
            "stop",
            "frequency_penalty",
            "presence_penalty",
            "logit_bias",
            "logprobs",
            "top_logprobs",
            "seed",
            "user",
            "tools",
            "tool_choice",
            "functions",
            "function_call",
            "parallel_tool_calls",
            "response_format",
            "metadata",
            "store",
            "reasoning_effort",
            "service_tier",
            "modalities",
            "audio",
            "prediction",
            "web_search_options",
            "verbosity",
            "prompt_cache_key",
            "safety_identifier",
            "guardrails",
            "extra_body",
        ],
        Route::Responses => &[
            "model",
            "input",
            "instructions",
            "stream",
            "stream_options",
            "max_output_tokens",
            "max_tool_calls",
            "max_tokens",
            "max_completion_tokens",
            "n",
            "temperature",
            "top_p",
            "top_logprobs",
            "tools",
            "tool_choice",
            "parallel_tool_calls",
            "metadata",
            "store",
            "reasoning",
            "service_tier",
            "truncation",
            "include",
            "text",
            "previous_response_id",
            "conversation",
            "background",
            "prompt",
            "user",
            "prompt_cache_key",
            "safety_identifier",
            "guardrails",
            "extra_body",
        ],
        Route::AnthropicMessages => &[
            "model",
            "messages",
            "system",
            "stream",
            "max_tokens",
            "max_completion_tokens",
            "max_output_tokens",
            "n",
            "temperature",
            "top_p",
            "top_k",
            "stop_sequences",
            "metadata",
            "tools",
            "tool_choice",
            "thinking",
            "output_config",
            "service_tier",
            "cache_control",
            "container",
            "context_management",
            "guardrails",
            "extra_body",
        ],
        _ => return Err(GatewayError::PolicyDenied),
    };
    let value: Value = serde_json::from_slice(body).map_err(|_| GatewayError::PolicyDenied)?;
    if value.as_object().is_none_or(|object| {
        object
            .keys()
            .any(|field| !allowed.contains(&field.as_str()))
    }) {
        return Err(GatewayError::PolicyDenied);
    }
    // Deriving this probe rejects duplicate limit/multiplicity fields; parsing
    // into Value first would silently collapse them before validation.
    let probe: OutputMaximumProbe =
        serde_json::from_slice(body).map_err(|_| GatewayError::PolicyDenied)?;
    let required = match route {
        Route::ChatCompletions => match (&probe.max_completion_tokens, &probe.max_tokens) {
            (OutputMaximum::Tokens(value), _) | (_, OutputMaximum::Tokens(value)) => *value,
            _ => return Err(GatewayError::PolicyDenied),
        },
        Route::Responses => match probe.max_output_tokens {
            OutputMaximum::Tokens(value) => value,
            _ => return Err(GatewayError::PolicyDenied),
        },
        Route::AnthropicMessages => match probe.max_tokens {
            OutputMaximum::Tokens(value) => value,
            _ => return Err(GatewayError::PolicyDenied),
        },
        // Generic services/wildcards have no proven output-limit wire contract.
        _ => return Err(GatewayError::PolicyDenied),
    };
    if required == 0 {
        return Err(GatewayError::PolicyDenied);
    }
    for field in [
        probe.max_tokens,
        probe.max_completion_tokens,
        probe.max_output_tokens,
    ] {
        if let OutputMaximum::Tokens(value) = field {
            if value != required {
                return Err(GatewayError::PolicyDenied);
            }
        }
    }
    let count = match probe.n {
        OutputMaximum::Missing => 1,
        OutputMaximum::Tokens(value) => value,
    };
    if count == 0 || (route != Route::ChatCompletions && count != 1) {
        return Err(GatewayError::PolicyDenied);
    }
    required
        .checked_mul(count)
        .ok_or(GatewayError::PolicyDenied)
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
    analyze_generation_request(body)
        .map(|analysis| analysis.features)
        .unwrap_or_default()
}

pub fn analyze_generation_request(body: &[u8]) -> Option<GenerationRequestAnalysis> {
    let probe = serde_json::from_slice::<GenerationRequestProbe>(body).ok()?;
    let model = probe.model.filter(|value| !value.is_empty());
    let service_name = probe
        .service
        .filter(|value| !value.is_empty())
        .or_else(|| probe.service_name.filter(|value| !value.is_empty()));
    Some(GenerationRequestAnalysis {
        features: GenerationFeatures {
            model,
            stream: probe.stream,
            tools: probe.tools.is_some_and(|tools| !tools.is_empty())
                || probe
                    .functions
                    .is_some_and(|functions| !functions.is_empty())
                || probe
                    .function_call
                    .as_ref()
                    .is_some_and(explicit_tool_selection)
                || probe
                    .tool_choice
                    .as_ref()
                    .is_some_and(explicit_tool_selection),
            service_name,
        },
        client_guardrails: probe.guardrails,
    })
}

fn explicit_tool_selection(value: &Value) -> bool {
    value.is_object()
        || value
            .as_str()
            .is_some_and(|value| value != "none" && value != "auto")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_caps_reject_ambiguous_or_unbounded_requests_before_generation() {
        let policy = KeyPolicy {
            max_output_tokens_per_request: Some(4),
            ..KeyPolicy::default()
        };
        for body in [
            r#"{}"#,
            r#"{"max_tokens":null}"#,
            r#"{"max_tokens":"1"}"#,
            r#"{"max_tokens":-1}"#,
            r#"{"max_tokens":0}"#,
            r#"{"max_tokens":5}"#,
            r#"{"max_tokens":1,"max_completion_tokens":100}"#,
            r#"{"max_tokens":1,"max_tokens":2}"#,
            r#"{"max_tokens":3,"n":2}"#,
            r#"{"max_tokens":1,"n":0}"#,
            r#"{"max_tokens":1,"n":null}"#,
            r#"{"max_tokens":18446744073709551615,"n":2}"#,
            r#"{"max_tokens":1,"extra_body":{"max_tokens":100}}"#,
            r#"{"max_tokens":1,"max_new_tokens":100}"#,
            r#"{"max_tokens":1,"extra_body":{"top_k":10}}"#,
            r#"{"max_tokens":1,"extra_body":{},"extra_body":{}}"#,
        ] {
            assert_eq!(
                enforce_output_token_limit(&policy, Route::ChatCompletions, body.as_bytes()),
                Err(GatewayError::PolicyDenied),
                "{body}"
            );
        }
        for (route, body) in [
            (
                Route::ChatCompletions,
                r#"{"max_tokens":2,"n":2,"stream":true}"#,
            ),
            (Route::ChatCompletions, r#"{"max_completion_tokens":4}"#),
            (
                Route::ChatCompletions,
                r#"{"max_tokens":1,"extra_body":{}}"#,
            ),
            (Route::Responses, r#"{"max_output_tokens":4,"stream":true}"#),
            (Route::AnthropicMessages, r#"{"max_tokens":4}"#),
        ] {
            assert!(enforce_output_token_limit(&policy, route, body.as_bytes()).is_ok());
        }
        assert!(
            enforce_output_token_limit(&policy, Route::Responses, br#"{"max_tokens":4}"#).is_err()
        );
        assert!(enforce_output_token_limit(
            &policy,
            Route::ServiceWildcard,
            br#"{"max_tokens":1}"#
        )
        .is_err());
        assert!(enforce_output_token_limit(&policy, Route::LiteLlmEmbeddings, br#"{}"#).is_ok());
        let zero = KeyPolicy {
            max_output_tokens_per_request: Some(0),
            ..policy
        };
        assert!(
            enforce_output_token_limit(&zero, Route::ChatCompletions, br#"{"max_tokens":1}"#)
                .is_err()
        );
    }

    #[test]
    fn capped_responses_allow_standard_tool_and_logprob_controls() {
        let policy = KeyPolicy {
            max_output_tokens_per_request: Some(2),
            ..KeyPolicy::default()
        };
        for field in ["max_tool_calls", "top_logprobs"] {
            for (maximum, allowed) in [(2, true), (3, false)] {
                let body = serde_json::to_vec(
                    &serde_json::json!({"model":"allowed", "max_output_tokens":maximum, field:1}),
                )
                .unwrap();
                assert_eq!(
                    enforce_output_token_limit(&policy, Route::Responses, &body).is_ok(),
                    allowed
                );
            }
            let body =
                serde_json::to_vec(&serde_json::json!({"model":"allowed", field:1})).unwrap();
            assert_eq!(
                enforce_output_token_limit(&policy, Route::Responses, &body),
                Err(GatewayError::PolicyDenied)
            );
        }
    }

    #[test]
    fn uncapped_provider_extensions_cannot_override_denied_features() {
        let policy = KeyPolicy::default();
        for body in [
            br#"{"extra_body":{"stream":true}}"#.as_slice(),
            br#"{"extra_body":{"tools":[{}]}}"#.as_slice(),
            br#"{"extra_body":{"functions":[{}]}}"#.as_slice(),
        ] {
            assert!(enforce_output_token_limit(&policy, Route::ChatCompletions, body).is_err());
        }
        assert!(enforce_output_token_limit(
            &policy,
            Route::ChatCompletions,
            br#"{"extra_body":{"top_k":10}}"#
        )
        .is_ok());
    }

    #[test]
    fn provider_extensions_cannot_change_the_policy_selected_model() {
        let policy = KeyPolicy {
            allow_streaming: true,
            allow_tools: true,
            ..KeyPolicy::default()
        };
        for route in [
            Route::ChatCompletions,
            Route::Responses,
            Route::AnthropicMessages,
            Route::LiteLlmEmbeddings,
            Route::LiteLlmRerank,
            Route::LiteLlmPassthrough,
        ] {
            assert_eq!(
                enforce_output_token_limit(
                    &policy,
                    route,
                    br#"{"model":"uncapped-model","extra_body":{"model":"capped-model"}}"#
                ),
                Err(GatewayError::PolicyDenied)
            );
        }
        assert_eq!(enforce_output_token_limit(&policy, Route::AnthropicMessageBatches,
            br#"{"requests":[{"params":{"model":"uncapped-model","extra_body":{"model":"capped-model"}}}]}"#),
            Err(GatewayError::PolicyDenied));
        assert!(enforce_output_token_limit(
            &policy,
            Route::LiteLlmPassthrough,
            b"opaque delegated request"
        )
        .is_ok());
    }

    #[test]
    fn batch_output_cap_bounds_the_sum_and_preserves_nested_duplicate_detection() {
        let policy = KeyPolicy {
            max_output_tokens_per_request: Some(4),
            ..KeyPolicy::default()
        };
        for (body, allowed) in [
            (
                r#"{"requests":[{"params":{"max_tokens":2}},{"params":{"max_tokens":2}}]}"#,
                true,
            ),
            (
                r#"{"requests":[{"params":{"max_tokens":3}},{"params":{"max_tokens":2}}]}"#,
                false,
            ),
            (
                r#"{"requests":[{"params":{"max_tokens":1,"max_tokens":2}}]}"#,
                false,
            ),
            (r#"{"requests":[{"params":{}}]}"#, false),
        ] {
            assert_eq!(
                enforce_output_token_limit(
                    &policy,
                    Route::AnthropicMessageBatches,
                    body.as_bytes()
                )
                .is_ok(),
                allowed,
                "{body}"
            );
        }
    }

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
    fn request_analysis_ignores_large_payload_fields_and_keeps_control_metadata() {
        let body = format!(
            r#"{{"model":"gpt-test","messages":[{{"content":"{}"}}],"stream":true,"tools":[{{"type":"function"}}],"guardrails":["pii-redact"]}}"#,
            "x".repeat(1024 * 1024)
        );

        let analysis = analyze_generation_request(body.as_bytes()).expect("analysis");

        assert_eq!(analysis.features.model.as_deref(), Some("gpt-test"));
        assert!(analysis.features.stream);
        assert!(analysis.features.tools);
        assert_eq!(
            analysis.client_guardrails,
            Some(serde_json::json!(["pii-redact"]))
        );
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

    #[test]
    fn policy_layers_and_request_limits_cover_empty_and_single_sided_cases() {
        let empty = resolve_effective_policy(Vec::new()).expect("empty effective policy");
        assert_eq!(empty.policy, KeyPolicy::default());
        assert!(empty.applied_layers.is_empty());

        for (kind, name) in [
            (PolicyLayerKind::Global, "global"),
            (PolicyLayerKind::Project, "project"),
            (PolicyLayerKind::Team, "team"),
            (PolicyLayerKind::Key, "key"),
            (PolicyLayerKind::Route, "route"),
            (PolicyLayerKind::Model, "model"),
        ] {
            assert_eq!(kind.as_str(), name);
        }

        let now = DateTime::parse_from_rfc3339("2026-05-23T12:00:00Z")
            .expect("time")
            .with_timezone(&Utc);
        let policy = KeyPolicy {
            max_input_tokens_per_request: Some(10),
            max_output_tokens_per_request: Some(20),
            max_cost_per_request: Some(0.25),
            ..KeyPolicy::default()
        };
        assert_eq!(
            evaluate_policy_limits(&policy, now, None, None, Some(11), None, None).unwrap_err(),
            GatewayError::PolicyDenied
        );
        assert_eq!(
            evaluate_policy_limits(&policy, now, None, None, None, Some(21), None).unwrap_err(),
            GatewayError::PolicyDenied
        );
        assert_eq!(
            evaluate_policy_limits(&policy, now, None, None, None, None, Some(0.3)).unwrap_err(),
            GatewayError::PolicyDenied
        );
        evaluate_policy_limits(&policy, now, None, None, Some(10), Some(20), Some(0.25))
            .expect("limits at boundary");

        let effective = resolve_effective_policy(vec![
            PolicyLayer {
                kind: PolicyLayerKind::Global,
                scope_id: None,
                policy: KeyPolicy {
                    rpm_limit: Some(10),
                    max_request_body_bytes: Some(100),
                    daily_budget_usd: Some(1.0),
                    ..KeyPolicy::default()
                },
                guardrail_policy: GuardrailPolicy::default(),
                policy_version: 1,
            },
            PolicyLayer {
                kind: PolicyLayerKind::Key,
                scope_id: Some("key".to_owned()),
                policy: KeyPolicy::default(),
                guardrail_policy: GuardrailPolicy::default(),
                policy_version: 2,
            },
        ])
        .expect("single-sided limits");
        assert_eq!(effective.policy.rpm_limit, Some(10));
        assert_eq!(effective.policy.max_request_body_bytes, Some(100));
        assert_eq!(effective.policy.daily_budget_usd, Some(1.0));
    }
}
