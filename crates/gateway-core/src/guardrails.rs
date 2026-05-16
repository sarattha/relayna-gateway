use crate::{GatewayError, GatewayResult, Provider, Route};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::BTreeMap, fmt, time::Duration, time::Instant};
use uuid::Uuid;

pub const PII_REDACT_GUARDRAIL: &str = "pii-redact";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GuardrailMode {
    PreCall,
    PostCall,
    DuringCall,
}

impl GuardrailMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PreCall => "pre_call",
            Self::PostCall => "post_call",
            Self::DuringCall => "during_call",
        }
    }
}

impl std::str::FromStr for GuardrailMode {
    type Err = GatewayError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "pre_call" => Ok(Self::PreCall),
            "post_call" => Ok(Self::PostCall),
            "during_call" => Ok(Self::DuringCall),
            _ => Err(GatewayError::InvalidGuardrailRequest),
        }
    }
}

impl fmt::Display for GuardrailMode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GuardrailAction {
    Allow,
    Block,
    Modify,
    Warn,
}

impl GuardrailAction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::Block => "block",
            Self::Modify => "modify",
            Self::Warn => "warn",
        }
    }
}

impl std::str::FromStr for GuardrailAction {
    type Err = GatewayError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "allow" => Ok(Self::Allow),
            "block" => Ok(Self::Block),
            "modify" => Ok(Self::Modify),
            "warn" => Ok(Self::Warn),
            _ => Err(GatewayError::InvalidGuardrailRequest),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GuardrailFailurePolicy {
    FailClosed,
    FailOpen,
    DryRun,
}

impl GuardrailFailurePolicy {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::FailClosed => "fail_closed",
            Self::FailOpen => "fail_open",
            Self::DryRun => "dry_run",
        }
    }
}

impl std::str::FromStr for GuardrailFailurePolicy {
    type Err = GatewayError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "fail_closed" => Ok(Self::FailClosed),
            "fail_open" => Ok(Self::FailOpen),
            "dry_run" => Ok(Self::DryRun),
            _ => Err(GatewayError::InvalidGuardrailRequest),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GuardrailDefinition {
    pub name: String,
    pub description: String,
    pub modes: Vec<GuardrailMode>,
    pub default_on: bool,
    pub failure_policy: GuardrailFailurePolicy,
    #[serde(default)]
    pub config_schema: Value,
    #[serde(default)]
    pub config: Value,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GuardrailProviderKind {
    BuiltIn,
    Http,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GuardrailDefinitionResponse {
    pub name: String,
    pub description: String,
    pub modes: Vec<GuardrailMode>,
    pub default_on: bool,
    pub failure_policy: GuardrailFailurePolicy,
    pub config_schema: Value,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GuardrailPolicy {
    #[serde(default)]
    pub mandatory_guardrails: Vec<String>,
    #[serde(default)]
    pub optional_guardrails: Vec<String>,
    #[serde(default)]
    pub forbidden_guardrails: Vec<String>,
}

impl GuardrailPolicy {
    pub fn validate(&self) -> GatewayResult<()> {
        for name in self
            .mandatory_guardrails
            .iter()
            .chain(self.optional_guardrails.iter())
            .chain(self.forbidden_guardrails.iter())
        {
            validate_guardrail_name(name)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct GuardrailPolicyPatch {
    pub mandatory_guardrails: Option<Vec<String>>,
    pub optional_guardrails: Option<Vec<String>>,
    pub forbidden_guardrails: Option<Vec<String>>,
}

impl GuardrailPolicyPatch {
    pub fn apply(self, mut policy: GuardrailPolicy) -> GatewayResult<GuardrailPolicy> {
        if let Some(value) = self.mandatory_guardrails {
            policy.mandatory_guardrails = value;
        }
        if let Some(value) = self.optional_guardrails {
            policy.optional_guardrails = value;
        }
        if let Some(value) = self.forbidden_guardrails {
            policy.forbidden_guardrails = value;
        }
        policy.validate()?;
        Ok(policy)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct GuardrailPlanEntry {
    pub definition: GuardrailDefinition,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct GuardrailPlan {
    pub entries: Vec<GuardrailPlanEntry>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GuardrailPolicySet {
    pub key_policy: GuardrailPolicy,
    pub team_policy: Option<GuardrailPolicy>,
    pub route_policy: Option<GuardrailPolicy>,
    pub model_policy: Option<GuardrailPolicy>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GuardrailPlanRequest {
    pub mode: GuardrailMode,
    pub definitions: Vec<GuardrailDefinition>,
    pub policies: GuardrailPolicySet,
    pub client_requested_guardrails: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GuardrailContext {
    pub request_id: String,
    pub key_id: Option<Uuid>,
    pub team_id: Option<Uuid>,
    pub user_id: Option<String>,
    pub project_id: Option<Uuid>,
    pub model: Option<String>,
    pub route: Option<Route>,
    pub provider: Option<Provider>,
    pub pii_mappings: Vec<(String, String)>,
    pub applied_guardrails: Vec<String>,
    pub guardrail_results: Vec<GuardrailExecutionRecord>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GuardrailInput {
    pub mode: GuardrailMode,
    pub request: Option<Value>,
    pub response: Option<Value>,
    pub context: GuardrailContext,
    pub config: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GuardrailResult {
    pub action: GuardrailAction,
    pub request: Option<Value>,
    pub response: Option<Value>,
    pub reason: Option<String>,
    pub metadata: Value,
    pub pii_mappings: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GuardrailExecutionRecord {
    pub guardrail_name: String,
    pub mode: GuardrailMode,
    pub action: GuardrailAction,
    pub failure_policy: GuardrailFailurePolicy,
    pub latency_ms: u128,
    pub reason: Option<String>,
    pub metadata: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GuardrailExecutionEvent {
    pub request_id: String,
    pub key_id: Option<Uuid>,
    pub project_id: Option<Uuid>,
    pub route: Option<Route>,
    pub model: Option<String>,
    pub provider: Option<Provider>,
    pub guardrail_name: String,
    pub mode: GuardrailMode,
    pub action: GuardrailAction,
    pub failure_policy: GuardrailFailurePolicy,
    pub latency_ms: i64,
    pub reason: Option<String>,
    pub metadata: Value,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct GuardrailTestRequest {
    #[serde(default)]
    pub guardrails: Vec<String>,
    pub mode: GuardrailMode,
    pub input: Value,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct GuardrailTestResponse {
    pub input: Value,
    pub applied_guardrails: Vec<String>,
    pub results: Vec<GuardrailExecutionRecord>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
pub struct GuardrailEventQuery {
    pub from: Option<DateTime<Utc>>,
    pub to: Option<DateTime<Utc>>,
    pub key_id: Option<Uuid>,
    pub project_id: Option<Uuid>,
    pub route: Option<String>,
    pub model: Option<String>,
    pub provider: Option<String>,
    pub guardrail: Option<String>,
    pub mode: Option<String>,
    pub action: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct GuardrailExecutionSummary {
    pub guardrail_name: String,
    pub mode: GuardrailMode,
    pub action: GuardrailAction,
    pub failure_policy: GuardrailFailurePolicy,
    pub count: i64,
    pub total_latency_ms: i64,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct GuardrailAdminCreateRequest {
    pub name: String,
    pub description: String,
    pub endpoint_url: String,
    #[serde(default)]
    pub modes: Vec<GuardrailMode>,
    #[serde(default)]
    pub default_on: bool,
    pub failure_policy: GuardrailFailurePolicy,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
    #[serde(default)]
    pub bearer_token: Option<String>,
    #[serde(default)]
    pub config_schema: Value,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
pub struct GuardrailAdminPatchRequest {
    pub description: Option<String>,
    pub endpoint_url: Option<String>,
    pub modes: Option<Vec<GuardrailMode>>,
    pub default_on: Option<bool>,
    pub failure_policy: Option<GuardrailFailurePolicy>,
    pub timeout_ms: Option<u64>,
    pub bearer_token: Option<Option<String>>,
    pub config_schema: Option<Value>,
    pub enabled: Option<bool>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct AdminGuardrailDefinitionResponse {
    pub name: String,
    pub description: String,
    pub provider_kind: GuardrailProviderKind,
    pub modes: Vec<GuardrailMode>,
    pub default_on: bool,
    pub failure_policy: GuardrailFailurePolicy,
    pub config_schema: Value,
    pub enabled: bool,
    pub endpoint_configured: bool,
    pub token_configured: bool,
}

#[async_trait]
pub trait GuardrailStore: Send + Sync {
    async fn list_guardrail_definitions(&self) -> GatewayResult<Vec<GuardrailDefinition>>;
    async fn guardrail_policy_for_key(&self, key_id: Uuid) -> GatewayResult<GuardrailPolicy>;
    async fn upsert_guardrail_policy_for_key(
        &self,
        key_id: Uuid,
        policy: &GuardrailPolicy,
    ) -> GatewayResult<()>;
    async fn insert_guardrail_execution_event(
        &self,
        event: &GuardrailExecutionEvent,
    ) -> GatewayResult<()>;
}

#[async_trait]
pub trait GuardrailObservabilityStore: Send + Sync {
    async fn list_admin_guardrail_definitions(
        &self,
    ) -> GatewayResult<Vec<AdminGuardrailDefinitionResponse>>;

    async fn guardrail_execution_events(
        &self,
        query: GuardrailEventQuery,
    ) -> GatewayResult<Vec<GuardrailExecutionEvent>>;

    async fn guardrail_execution_summary(
        &self,
        query: GuardrailEventQuery,
    ) -> GatewayResult<Vec<GuardrailExecutionSummary>>;

    async fn create_http_guardrail(
        &self,
        request: GuardrailAdminCreateRequest,
    ) -> GatewayResult<AdminGuardrailDefinitionResponse>;

    async fn patch_http_guardrail(
        &self,
        name: String,
        request: GuardrailAdminPatchRequest,
    ) -> GatewayResult<AdminGuardrailDefinitionResponse>;
}

#[async_trait]
impl<T> GuardrailObservabilityStore for std::sync::Arc<T>
where
    T: GuardrailObservabilityStore + ?Sized,
{
    async fn list_admin_guardrail_definitions(
        &self,
    ) -> GatewayResult<Vec<AdminGuardrailDefinitionResponse>> {
        (**self).list_admin_guardrail_definitions().await
    }

    async fn guardrail_execution_events(
        &self,
        query: GuardrailEventQuery,
    ) -> GatewayResult<Vec<GuardrailExecutionEvent>> {
        (**self).guardrail_execution_events(query).await
    }

    async fn guardrail_execution_summary(
        &self,
        query: GuardrailEventQuery,
    ) -> GatewayResult<Vec<GuardrailExecutionSummary>> {
        (**self).guardrail_execution_summary(query).await
    }

    async fn create_http_guardrail(
        &self,
        request: GuardrailAdminCreateRequest,
    ) -> GatewayResult<AdminGuardrailDefinitionResponse> {
        (**self).create_http_guardrail(request).await
    }

    async fn patch_http_guardrail(
        &self,
        name: String,
        request: GuardrailAdminPatchRequest,
    ) -> GatewayResult<AdminGuardrailDefinitionResponse> {
        (**self).patch_http_guardrail(name, request).await
    }
}

#[async_trait]
impl<T> GuardrailStore for std::sync::Arc<T>
where
    T: GuardrailStore + ?Sized,
{
    async fn list_guardrail_definitions(&self) -> GatewayResult<Vec<GuardrailDefinition>> {
        (**self).list_guardrail_definitions().await
    }

    async fn guardrail_policy_for_key(&self, key_id: Uuid) -> GatewayResult<GuardrailPolicy> {
        (**self).guardrail_policy_for_key(key_id).await
    }

    async fn upsert_guardrail_policy_for_key(
        &self,
        key_id: Uuid,
        policy: &GuardrailPolicy,
    ) -> GatewayResult<()> {
        (**self)
            .upsert_guardrail_policy_for_key(key_id, policy)
            .await
    }

    async fn insert_guardrail_execution_event(
        &self,
        event: &GuardrailExecutionEvent,
    ) -> GatewayResult<()> {
        (**self).insert_guardrail_execution_event(event).await
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct GuardrailExecution {
    pub request: Option<Value>,
    pub response: Option<Value>,
    pub context: GuardrailContext,
    pub records: Vec<GuardrailExecutionRecord>,
}

pub trait GuardrailHandler: Send + Sync {
    fn execute(&self, input: GuardrailInput) -> GatewayResult<GuardrailResult>;
}

#[derive(Default)]
pub struct InMemoryGuardrailExecutor {
    handlers: BTreeMap<String, Box<dyn GuardrailHandler>>,
}

impl GuardrailDefinition {
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        modes: Vec<GuardrailMode>,
        failure_policy: GuardrailFailurePolicy,
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            modes,
            default_on: false,
            failure_policy,
            config_schema: Value::Object(Default::default()),
            config: Value::Object(Default::default()),
            enabled: true,
        }
    }

    pub fn default_on(mut self, default_on: bool) -> Self {
        self.default_on = default_on;
        self
    }

    pub fn with_config(mut self, config: Value) -> Self {
        self.config = config;
        self
    }

    pub fn with_config_schema(mut self, config_schema: Value) -> Self {
        self.config_schema = config_schema;
        self
    }

    pub fn response(&self) -> GuardrailDefinitionResponse {
        GuardrailDefinitionResponse {
            name: self.name.clone(),
            description: self.description.clone(),
            modes: self.modes.clone(),
            default_on: self.default_on,
            failure_policy: self.failure_policy,
            config_schema: self.config_schema.clone(),
        }
    }

    fn supports_mode(&self, mode: GuardrailMode) -> bool {
        self.enabled && self.modes.contains(&mode)
    }
}

impl Default for GuardrailContext {
    fn default() -> Self {
        Self {
            request_id: uuid::Uuid::new_v4().to_string(),
            key_id: None,
            team_id: None,
            user_id: None,
            project_id: None,
            model: None,
            route: None,
            provider: None,
            pii_mappings: Vec::new(),
            applied_guardrails: Vec::new(),
            guardrail_results: Vec::new(),
        }
    }
}

impl GuardrailResult {
    pub fn allow() -> Self {
        Self {
            action: GuardrailAction::Allow,
            request: None,
            response: None,
            reason: None,
            metadata: Value::Object(Default::default()),
            pii_mappings: Vec::new(),
        }
    }

    pub fn block(reason: impl Into<String>) -> Self {
        Self {
            action: GuardrailAction::Block,
            request: None,
            response: None,
            reason: Some(reason.into()),
            metadata: Value::Object(Default::default()),
            pii_mappings: Vec::new(),
        }
    }

    pub fn modify_request(request: Value) -> Self {
        Self {
            action: GuardrailAction::Modify,
            request: Some(request),
            response: None,
            reason: None,
            metadata: Value::Object(Default::default()),
            pii_mappings: Vec::new(),
        }
    }

    pub fn modify_response(response: Value) -> Self {
        Self {
            action: GuardrailAction::Modify,
            request: None,
            response: Some(response),
            reason: None,
            metadata: Value::Object(Default::default()),
            pii_mappings: Vec::new(),
        }
    }

    pub fn warn(reason: impl Into<String>) -> Self {
        Self {
            action: GuardrailAction::Warn,
            request: None,
            response: None,
            reason: Some(reason.into()),
            metadata: Value::Object(Default::default()),
            pii_mappings: Vec::new(),
        }
    }
}

impl InMemoryGuardrailExecutor {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(
        &mut self,
        name: impl Into<String>,
        handler: impl GuardrailHandler + 'static,
    ) -> &mut Self {
        self.handlers.insert(name.into(), Box::new(handler));
        self
    }

    pub fn execute(
        &self,
        plan: &GuardrailPlan,
        mode: GuardrailMode,
        context: GuardrailContext,
        request: Option<Value>,
        response: Option<Value>,
    ) -> GatewayResult<GuardrailExecution> {
        let mut execution = GuardrailExecution {
            request,
            response,
            context,
            records: Vec::new(),
        };

        for entry in &plan.entries {
            let name = entry.definition.name.clone();
            let Some(handler) = self.handlers.get(&name) else {
                handle_guardrail_failure(
                    &mut execution,
                    &entry.definition,
                    mode,
                    GatewayError::GuardrailUnavailable,
                    0,
                )?;
                continue;
            };

            let started = Instant::now();
            let result = handler.execute(GuardrailInput {
                mode,
                request: execution.request.clone(),
                response: execution.response.clone(),
                context: execution.context.clone(),
                config: entry.definition.config.clone(),
            });
            let latency_ms = started.elapsed().as_millis();

            let result = match result {
                Ok(result) => result,
                Err(error) => {
                    handle_guardrail_failure(
                        &mut execution,
                        &entry.definition,
                        mode,
                        error,
                        latency_ms,
                    )?;
                    continue;
                }
            };

            let effective_action =
                if entry.definition.failure_policy == GuardrailFailurePolicy::DryRun {
                    GuardrailAction::Warn
                } else {
                    result.action
                };
            let record = GuardrailExecutionRecord {
                guardrail_name: name.clone(),
                mode,
                action: effective_action,
                failure_policy: entry.definition.failure_policy,
                latency_ms,
                reason: result.reason.clone(),
                metadata: result.metadata.clone(),
            };
            execution
                .context
                .pii_mappings
                .extend(result.pii_mappings.clone());
            execution.context.applied_guardrails.push(name);
            execution.context.guardrail_results.push(record.clone());
            execution.records.push(record);

            if entry.definition.failure_policy == GuardrailFailurePolicy::DryRun {
                continue;
            }

            match result.action {
                GuardrailAction::Allow | GuardrailAction::Warn => {}
                GuardrailAction::Modify => {
                    if let Some(request) = result.request {
                        execution.request = Some(request);
                    }
                    if let Some(response) = result.response {
                        execution.response = Some(response);
                    }
                }
                GuardrailAction::Block => return Err(GatewayError::GuardrailBlocked),
            }
        }

        Ok(execution)
    }
}

pub fn builtin_guardrail_executor() -> InMemoryGuardrailExecutor {
    let mut executor = InMemoryGuardrailExecutor::new();
    executor.register(PII_REDACT_GUARDRAIL, PiiRedactGuardrail);
    executor
}

pub fn guardrail_executor_for_definitions(
    definitions: &[GuardrailDefinition],
) -> InMemoryGuardrailExecutor {
    let mut executor = builtin_guardrail_executor();
    for definition in definitions {
        if definition.name == PII_REDACT_GUARDRAIL {
            continue;
        }
        if http_guardrail_config(&definition.config).is_some() {
            executor.register(definition.name.clone(), HttpGuardrailHandler);
        }
    }
    executor
}

pub fn pii_redact_definition() -> GuardrailDefinition {
    GuardrailDefinition::new(
        PII_REDACT_GUARDRAIL,
        "Redacts common PII before provider calls and optionally restores placeholders after responses.",
        vec![
            GuardrailMode::PreCall,
            GuardrailMode::PostCall,
            GuardrailMode::DuringCall,
        ],
        GuardrailFailurePolicy::FailClosed,
    )
    .with_config(json!({ "restore_output": true }))
    .with_config_schema(json!({ "restore_output": "boolean" }))
}

pub fn strip_client_guardrails(value: &mut Value) {
    if let Value::Object(object) = value {
        object.remove("guardrails");
    }
}

pub fn extract_client_guardrails(value: &Value) -> GatewayResult<Vec<String>> {
    let Some(guardrails) = value.get("guardrails") else {
        return Ok(Vec::new());
    };
    let Some(items) = guardrails.as_array() else {
        return Err(GatewayError::InvalidGuardrailRequest);
    };
    items
        .iter()
        .map(|item| {
            item.as_str()
                .map(ToOwned::to_owned)
                .ok_or(GatewayError::InvalidGuardrailRequest)
                .and_then(|name| {
                    validate_guardrail_name(&name)?;
                    Ok(name)
                })
        })
        .collect()
}

pub fn execution_events_from_records(
    context: &GuardrailContext,
    records: &[GuardrailExecutionRecord],
    created_at: DateTime<Utc>,
) -> Vec<GuardrailExecutionEvent> {
    records
        .iter()
        .map(|record| GuardrailExecutionEvent {
            request_id: context.request_id.clone(),
            key_id: context.key_id,
            project_id: context.project_id,
            route: context.route,
            model: context.model.clone(),
            provider: context.provider,
            guardrail_name: record.guardrail_name.clone(),
            mode: record.mode,
            action: record.action,
            failure_policy: record.failure_policy,
            latency_ms: i64::try_from(record.latency_ms).unwrap_or(i64::MAX),
            reason: record.reason.clone(),
            metadata: record.metadata.clone(),
            created_at,
        })
        .collect()
}

struct PiiRedactGuardrail;

impl GuardrailHandler for PiiRedactGuardrail {
    fn execute(&self, input: GuardrailInput) -> GatewayResult<GuardrailResult> {
        match input.mode {
            GuardrailMode::PreCall => redact_value(input.request, true),
            GuardrailMode::PostCall => {
                let restore_output = input
                    .config
                    .get("restore_output")
                    .and_then(Value::as_bool)
                    .unwrap_or(true);
                let mut response = input.response.unwrap_or(Value::Null);
                if restore_output {
                    restore_placeholders(&mut response, &input.context);
                }
                let result = redact_json_strings(&mut response, true);
                let mut output = GuardrailResult::modify_response(response);
                output.metadata = result.metadata();
                Ok(output)
            }
            GuardrailMode::DuringCall => {
                let mut response = input.response.unwrap_or(Value::Null);
                let result = redact_json_strings(&mut response, false);
                let mut output = GuardrailResult::modify_response(response);
                output.metadata = result.metadata();
                Ok(output)
            }
        }
    }
}

struct HttpGuardrailHandler;

#[derive(Debug, Clone)]
struct HttpGuardrailConfig {
    endpoint_url: String,
    bearer_token: Option<String>,
    timeout_ms: u64,
}

#[derive(Debug, Serialize)]
struct HttpGuardrailProviderRequest {
    request_id: String,
    guardrail: String,
    mode: GuardrailMode,
    context: Value,
    config: Value,
    request: Option<Value>,
    response: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct HttpGuardrailProviderResponse {
    action: GuardrailAction,
    request: Option<Value>,
    response: Option<Value>,
    reason: Option<String>,
    #[serde(default)]
    metadata: Value,
}

impl GuardrailHandler for HttpGuardrailHandler {
    fn execute(&self, input: GuardrailInput) -> GatewayResult<GuardrailResult> {
        let config =
            http_guardrail_config(&input.config).ok_or(GatewayError::GuardrailUnavailable)?;
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_millis(config.timeout_ms))
            .build()
            .map_err(|_| GatewayError::GuardrailUnavailable)?;
        let guardrail = input
            .config
            .get("guardrail_name")
            .and_then(Value::as_str)
            .unwrap_or("custom")
            .to_owned();
        let mut request = client
            .post(&config.endpoint_url)
            .json(&HttpGuardrailProviderRequest {
                request_id: input.context.request_id.clone(),
                guardrail,
                mode: input.mode,
                context: sanitized_context(&input.context),
                config: input
                    .config
                    .get("provider_config")
                    .cloned()
                    .unwrap_or_else(|| json!({})),
                request: input.request,
                response: input.response,
            });
        if let Some(token) = config.bearer_token {
            request = request.bearer_auth(token);
        }
        let response = request
            .send()
            .map_err(|_| GatewayError::GuardrailUnavailable)?;
        if !response.status().is_success() {
            return Err(GatewayError::GuardrailUnavailable);
        }
        let response: HttpGuardrailProviderResponse = response
            .json()
            .map_err(|_| GatewayError::InvalidGuardrailRequest)?;
        Ok(GuardrailResult {
            action: response.action,
            request: response.request,
            response: response.response,
            reason: response.reason,
            metadata: sanitize_metadata(response.metadata),
            pii_mappings: Vec::new(),
        })
    }
}

fn http_guardrail_config(value: &Value) -> Option<HttpGuardrailConfig> {
    if value.get("provider_kind").and_then(Value::as_str) != Some("http") {
        return None;
    }
    let endpoint_url = value.get("endpoint_url")?.as_str()?.to_owned();
    let timeout_ms = value
        .get("timeout_ms")
        .and_then(Value::as_u64)
        .unwrap_or(1500)
        .clamp(100, 10_000);
    Some(HttpGuardrailConfig {
        endpoint_url,
        bearer_token: value
            .get("bearer_token_secret")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        timeout_ms,
    })
}

fn sanitized_context(context: &GuardrailContext) -> Value {
    json!({
        "key_id": context.key_id,
        "team_id": context.team_id,
        "user_id": context.user_id,
        "project_id": context.project_id,
        "model": context.model,
        "route": context.route.map(Route::as_str),
        "provider": context.provider.map(Provider::as_str)
    })
}

fn sanitize_metadata(value: Value) -> Value {
    match value {
        Value::Object(mut object) => {
            object.retain(|key, _| {
                let normalized = key.to_ascii_lowercase();
                !(normalized.contains("secret")
                    || normalized.contains("token")
                    || normalized.contains("password")
                    || normalized.contains("authorization"))
            });
            Value::Object(
                object
                    .into_iter()
                    .map(|(key, value)| (key, sanitize_metadata(value)))
                    .collect(),
            )
        }
        Value::Array(items) => Value::Array(items.into_iter().map(sanitize_metadata).collect()),
        other => other,
    }
}

fn redact_value(value: Option<Value>, collect_mapping: bool) -> GatewayResult<GuardrailResult> {
    let mut value = value.unwrap_or(Value::Null);
    let result = redact_json_strings(&mut value, collect_mapping);
    let mut output = GuardrailResult::modify_request(value);
    output.metadata = result.metadata();
    output.pii_mappings = result.mappings;
    Ok(output)
}

#[derive(Default)]
struct RedactionResult {
    email_count: usize,
    phone_count: usize,
    ssn_count: usize,
    mappings: Vec<(String, String)>,
}

impl RedactionResult {
    fn metadata(&self) -> Value {
        json!({
            "email_count": self.email_count,
            "phone_count": self.phone_count,
            "ssn_count": self.ssn_count
        })
    }
}

fn redact_json_strings(value: &mut Value, collect_mapping: bool) -> RedactionResult {
    let mut result = RedactionResult::default();
    redact_json_strings_inner(value, &mut result, collect_mapping);
    result
}

fn redact_json_strings_inner(
    value: &mut Value,
    result: &mut RedactionResult,
    collect_mapping: bool,
) {
    match value {
        Value::String(text) => {
            *text = redact_text(text, result, collect_mapping);
        }
        Value::Array(items) => {
            for item in items {
                redact_json_strings_inner(item, result, collect_mapping);
            }
        }
        Value::Object(object) => {
            for value in object.values_mut() {
                redact_json_strings_inner(value, result, collect_mapping);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

fn redact_text(input: &str, result: &mut RedactionResult, collect_mapping: bool) -> String {
    let with_email = redact_tokens(input, result, collect_mapping, PiiKind::Email);
    let with_ssn = redact_tokens(&with_email, result, collect_mapping, PiiKind::Ssn);
    redact_tokens(&with_ssn, result, collect_mapping, PiiKind::Phone)
}

pub fn redact_pii_text(input: &str) -> (String, Value) {
    let mut result = RedactionResult::default();
    let redacted = redact_text(input, &mut result, false);
    (redacted, result.metadata())
}

#[derive(Clone, Copy)]
enum PiiKind {
    Email,
    Phone,
    Ssn,
}

impl PiiKind {
    fn label(self) -> &'static str {
        match self {
            Self::Email => "EMAIL",
            Self::Phone => "PHONE",
            Self::Ssn => "SSN",
        }
    }
}

fn redact_tokens(
    input: &str,
    result: &mut RedactionResult,
    collect_mapping: bool,
    kind: PiiKind,
) -> String {
    let mut output = String::with_capacity(input.len());
    let mut token = String::new();
    for character in input.chars() {
        if pii_token_character(character, kind) {
            token.push(character);
        } else {
            push_redacted_token(&mut output, &token, result, collect_mapping, kind);
            token.clear();
            output.push(character);
        }
    }
    push_redacted_token(&mut output, &token, result, collect_mapping, kind);
    output
}

fn pii_token_character(character: char, kind: PiiKind) -> bool {
    match kind {
        PiiKind::Email => {
            character.is_ascii_alphanumeric()
                || matches!(character, '@' | '.' | '_' | '%' | '+' | '-')
        }
        PiiKind::Phone => {
            character.is_ascii_digit() || matches!(character, '+' | '-' | '(' | ')' | '.')
        }
        PiiKind::Ssn => character.is_ascii_digit() || character == '-',
    }
}

fn push_redacted_token(
    output: &mut String,
    token: &str,
    result: &mut RedactionResult,
    collect_mapping: bool,
    kind: PiiKind,
) {
    if token.is_empty() {
        return;
    }
    let trimmed = token.trim_matches(|character: char| {
        matches!(
            character,
            ',' | '.' | ';' | ':' | ')' | '(' | '[' | ']' | '{' | '}' | '"' | '\''
        )
    });
    let is_match = match kind {
        PiiKind::Email => looks_like_email(trimmed),
        PiiKind::Phone => looks_like_phone(trimmed),
        PiiKind::Ssn => looks_like_ssn(trimmed),
    };
    if !is_match {
        output.push_str(token);
        return;
    }
    let index = match kind {
        PiiKind::Email => {
            result.email_count += 1;
            result.email_count
        }
        PiiKind::Phone => {
            result.phone_count += 1;
            result.phone_count
        }
        PiiKind::Ssn => {
            result.ssn_count += 1;
            result.ssn_count
        }
    };
    let placeholder = format!("[{}_{}]", kind.label(), index);
    if collect_mapping {
        result
            .mappings
            .push((placeholder.clone(), trimmed.to_owned()));
    }
    output.push_str(&token.replacen(trimmed, &placeholder, 1));
}

fn looks_like_email(value: &str) -> bool {
    let Some((local, domain)) = value.split_once('@') else {
        return false;
    };
    !local.is_empty()
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && value.chars().all(|character| {
            character.is_ascii_alphanumeric()
                || matches!(character, '@' | '.' | '_' | '%' | '+' | '-')
        })
}

fn looks_like_ssn(value: &str) -> bool {
    let parts: Vec<&str> = value.split('-').collect();
    parts.len() == 3
        && parts[0].len() == 3
        && parts[1].len() == 2
        && parts[2].len() == 4
        && parts
            .iter()
            .all(|part| part.chars().all(|character| character.is_ascii_digit()))
}

fn looks_like_phone(value: &str) -> bool {
    let digits = value
        .chars()
        .filter(|character| character.is_ascii_digit())
        .count();
    (10..=15).contains(&digits)
        && value.chars().all(|character| {
            character.is_ascii_digit() || matches!(character, '+' | '-' | '(' | ')' | '.')
        })
}

fn restore_placeholders(value: &mut Value, context: &GuardrailContext) {
    match value {
        Value::String(text) => {
            for (placeholder, original) in &context.pii_mappings {
                *text = text.replace(placeholder, original);
            }
        }
        Value::Array(items) => {
            for item in items {
                restore_placeholders(item, context);
            }
        }
        Value::Object(object) => {
            for value in object.values_mut() {
                restore_placeholders(value, context);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

fn validate_guardrail_name(name: &str) -> GatewayResult<()> {
    let valid = !name.is_empty()
        && name.len() <= 128
        && name.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
        });
    if valid {
        Ok(())
    } else {
        Err(GatewayError::InvalidGuardrailRequest)
    }
}

pub fn resolve_guardrail_plan(request: GuardrailPlanRequest) -> GatewayResult<GuardrailPlan> {
    let mut definitions = BTreeMap::new();
    for definition in request.definitions {
        if definition.supports_mode(request.mode) {
            definitions.insert(definition.name.clone(), definition);
        }
    }

    let mut forbidden = request.policies.key_policy.forbidden_guardrails.clone();
    let mut optional = request.policies.key_policy.optional_guardrails.clone();
    for policy in [
        request.policies.team_policy.as_ref(),
        request.policies.route_policy.as_ref(),
        request.policies.model_policy.as_ref(),
    ]
    .into_iter()
    .flatten()
    {
        forbidden.extend(policy.forbidden_guardrails.clone());
        optional.extend(policy.optional_guardrails.clone());
    }

    let mut plan = GuardrailPlan::default();
    for definition in definitions
        .values()
        .filter(|definition| definition.default_on)
    {
        push_definition_once(&mut plan, definition.clone());
    }
    for policy in [
        Some(&request.policies.key_policy),
        request.policies.team_policy.as_ref(),
        request.policies.route_policy.as_ref(),
        request.policies.model_policy.as_ref(),
    ]
    .into_iter()
    .flatten()
    {
        for name in &policy.mandatory_guardrails {
            if forbidden.iter().any(|forbidden| forbidden == name) {
                return Err(GatewayError::GuardrailForbidden);
            }
            let definition = definitions
                .get(name)
                .cloned()
                .ok_or(GatewayError::GuardrailUnavailable)?;
            push_definition_once(&mut plan, definition);
        }
    }

    for name in request.client_requested_guardrails {
        if forbidden.iter().any(|forbidden| forbidden == &name) {
            return Err(GatewayError::GuardrailForbidden);
        }
        if !optional.iter().any(|optional| optional == &name) {
            return Err(GatewayError::GuardrailForbidden);
        }
        let definition = definitions
            .get(&name)
            .cloned()
            .ok_or(GatewayError::InvalidGuardrailRequest)?;
        push_definition_once(&mut plan, definition);
    }

    Ok(plan)
}

fn push_definition_once(plan: &mut GuardrailPlan, definition: GuardrailDefinition) {
    if plan
        .entries
        .iter()
        .any(|entry| entry.definition.name == definition.name)
    {
        return;
    }
    plan.entries.push(GuardrailPlanEntry { definition });
}

fn handle_guardrail_failure(
    execution: &mut GuardrailExecution,
    definition: &GuardrailDefinition,
    mode: GuardrailMode,
    error: GatewayError,
    latency_ms: u128,
) -> GatewayResult<()> {
    let action = match definition.failure_policy {
        GuardrailFailurePolicy::FailClosed => return Err(error),
        GuardrailFailurePolicy::FailOpen | GuardrailFailurePolicy::DryRun => GuardrailAction::Warn,
    };
    let record = GuardrailExecutionRecord {
        guardrail_name: definition.name.clone(),
        mode,
        action,
        failure_policy: definition.failure_policy,
        latency_ms,
        reason: Some(error.code().to_owned()),
        metadata: Value::Object(Default::default()),
    };
    execution
        .context
        .applied_guardrails
        .push(definition.name.clone());
    execution.context.guardrail_results.push(record.clone());
    execution.records.push(record);
    Ok(())
}

fn default_enabled() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct ModifyRequestHandler(&'static str);

    impl GuardrailHandler for ModifyRequestHandler {
        fn execute(&self, input: GuardrailInput) -> GatewayResult<GuardrailResult> {
            let mut request = input.request.unwrap_or_else(|| json!({}));
            request["steps"] = match request.get("steps").and_then(Value::as_array) {
                Some(existing) => {
                    let mut steps = existing.clone();
                    steps.push(json!(self.0));
                    Value::Array(steps)
                }
                None => json!([self.0]),
            };
            Ok(GuardrailResult::modify_request(request))
        }
    }

    struct ErrorHandler(GatewayError);

    impl GuardrailHandler for ErrorHandler {
        fn execute(&self, _input: GuardrailInput) -> GatewayResult<GuardrailResult> {
            Err(self.0.clone())
        }
    }

    struct BlockHandler;

    impl GuardrailHandler for BlockHandler {
        fn execute(&self, _input: GuardrailInput) -> GatewayResult<GuardrailResult> {
            Ok(GuardrailResult::block("blocked"))
        }
    }

    fn definition(name: &str) -> GuardrailDefinition {
        GuardrailDefinition::new(
            name,
            format!("{name} guardrail"),
            vec![GuardrailMode::PreCall],
            GuardrailFailurePolicy::FailClosed,
        )
    }

    fn plan_names(plan: &GuardrailPlan) -> Vec<&str> {
        plan.entries
            .iter()
            .map(|entry| entry.definition.name.as_str())
            .collect()
    }

    #[test]
    fn mandatory_guardrails_apply_without_client_requested_guardrails() {
        let plan = resolve_guardrail_plan(GuardrailPlanRequest {
            mode: GuardrailMode::PreCall,
            definitions: vec![definition("pii-redact")],
            policies: GuardrailPolicySet {
                key_policy: GuardrailPolicy {
                    mandatory_guardrails: vec!["pii-redact".to_owned()],
                    ..GuardrailPolicy::default()
                },
                ..GuardrailPolicySet::default()
            },
            client_requested_guardrails: Vec::new(),
        })
        .expect("plan");

        assert_eq!(plan_names(&plan), vec!["pii-redact"]);
    }

    #[test]
    fn client_requested_guardrails_are_additive_only() {
        let plan = resolve_guardrail_plan(GuardrailPlanRequest {
            mode: GuardrailMode::PreCall,
            definitions: vec![definition("mandatory"), definition("optional")],
            policies: GuardrailPolicySet {
                key_policy: GuardrailPolicy {
                    mandatory_guardrails: vec!["mandatory".to_owned()],
                    optional_guardrails: vec!["optional".to_owned()],
                    ..GuardrailPolicy::default()
                },
                ..GuardrailPolicySet::default()
            },
            client_requested_guardrails: vec!["optional".to_owned()],
        })
        .expect("plan");

        assert_eq!(plan_names(&plan), vec!["mandatory", "optional"]);
    }

    #[test]
    fn forbidden_client_requested_guardrails_are_rejected() {
        let error = resolve_guardrail_plan(GuardrailPlanRequest {
            mode: GuardrailMode::PreCall,
            definitions: vec![definition("debug-raw-pii-viewer")],
            policies: GuardrailPolicySet {
                key_policy: GuardrailPolicy {
                    optional_guardrails: vec!["debug-raw-pii-viewer".to_owned()],
                    forbidden_guardrails: vec!["debug-raw-pii-viewer".to_owned()],
                    ..GuardrailPolicy::default()
                },
                ..GuardrailPolicySet::default()
            },
            client_requested_guardrails: vec!["debug-raw-pii-viewer".to_owned()],
        })
        .unwrap_err();

        assert_eq!(error, GatewayError::GuardrailForbidden);
    }

    #[test]
    fn unknown_client_requested_guardrails_are_rejected() {
        let error = resolve_guardrail_plan(GuardrailPlanRequest {
            mode: GuardrailMode::PreCall,
            definitions: vec![definition("known")],
            policies: GuardrailPolicySet {
                key_policy: GuardrailPolicy {
                    optional_guardrails: vec!["unknown".to_owned()],
                    ..GuardrailPolicy::default()
                },
                ..GuardrailPolicySet::default()
            },
            client_requested_guardrails: vec!["unknown".to_owned()],
        })
        .unwrap_err();

        assert_eq!(error, GatewayError::InvalidGuardrailRequest);
    }

    #[test]
    fn duplicate_guardrails_are_deduplicated_deterministically() {
        let plan = resolve_guardrail_plan(GuardrailPlanRequest {
            mode: GuardrailMode::PreCall,
            definitions: vec![
                definition("default").default_on(true),
                definition("mandatory"),
                definition("optional"),
            ],
            policies: GuardrailPolicySet {
                key_policy: GuardrailPolicy {
                    mandatory_guardrails: vec!["default".to_owned(), "mandatory".to_owned()],
                    optional_guardrails: vec!["mandatory".to_owned(), "optional".to_owned()],
                    ..GuardrailPolicy::default()
                },
                team_policy: Some(GuardrailPolicy {
                    mandatory_guardrails: vec!["mandatory".to_owned()],
                    ..GuardrailPolicy::default()
                }),
                ..GuardrailPolicySet::default()
            },
            client_requested_guardrails: vec!["mandatory".to_owned(), "optional".to_owned()],
        })
        .expect("plan");

        assert_eq!(plan_names(&plan), vec!["default", "mandatory", "optional"]);
    }

    #[test]
    fn dry_run_never_blocks_or_mutates() {
        let mut executor = InMemoryGuardrailExecutor::new();
        executor.register("dry-run", BlockHandler);
        let plan = GuardrailPlan {
            entries: vec![GuardrailPlanEntry {
                definition: definition("dry-run")
                    .default_on(true)
                    .with_config(json!({})),
            }],
        };
        let mut plan = plan;
        plan.entries[0].definition.failure_policy = GuardrailFailurePolicy::DryRun;

        let execution = executor
            .execute(
                &plan,
                GuardrailMode::PreCall,
                GuardrailContext::default(),
                Some(json!({"original": true})),
                None,
            )
            .expect("dry run continues");

        assert_eq!(execution.request, Some(json!({"original": true})));
        assert_eq!(execution.records[0].action, GuardrailAction::Warn);
    }

    #[test]
    fn fail_open_records_failure_and_continues() {
        let mut executor = InMemoryGuardrailExecutor::new();
        executor.register(
            "fail-open",
            ErrorHandler(GatewayError::GuardrailUnavailable),
        );
        let mut definition = definition("fail-open");
        definition.failure_policy = GuardrailFailurePolicy::FailOpen;
        let plan = GuardrailPlan {
            entries: vec![GuardrailPlanEntry { definition }],
        };

        let execution = executor
            .execute(
                &plan,
                GuardrailMode::PreCall,
                GuardrailContext::default(),
                Some(json!({"ok": true})),
                None,
            )
            .expect("fail open continues");

        assert_eq!(execution.records[0].action, GuardrailAction::Warn);
        assert_eq!(
            execution.records[0].reason.as_deref(),
            Some("guardrail_unavailable")
        );
    }

    #[test]
    fn fail_closed_returns_expected_error() {
        let mut executor = InMemoryGuardrailExecutor::new();
        executor.register(
            "fail-closed",
            ErrorHandler(GatewayError::GuardrailUnavailable),
        );
        let plan = GuardrailPlan {
            entries: vec![GuardrailPlanEntry {
                definition: definition("fail-closed"),
            }],
        };

        let error = executor
            .execute(
                &plan,
                GuardrailMode::PreCall,
                GuardrailContext::default(),
                Some(json!({})),
                None,
            )
            .unwrap_err();

        assert_eq!(error, GatewayError::GuardrailUnavailable);
    }

    #[test]
    fn modify_output_flows_to_next_guardrail() {
        let mut executor = InMemoryGuardrailExecutor::new();
        executor
            .register("first", ModifyRequestHandler("first"))
            .register("second", ModifyRequestHandler("second"));
        let plan = GuardrailPlan {
            entries: vec![
                GuardrailPlanEntry {
                    definition: definition("first"),
                },
                GuardrailPlanEntry {
                    definition: definition("second"),
                },
            ],
        };

        let execution = executor
            .execute(
                &plan,
                GuardrailMode::PreCall,
                GuardrailContext::default(),
                Some(json!({})),
                None,
            )
            .expect("execution");

        assert_eq!(
            execution.request,
            Some(json!({"steps": ["first", "second"]}))
        );
    }

    #[test]
    fn pii_redact_replaces_common_patterns_without_raw_metadata() {
        let executor = builtin_guardrail_executor();
        let plan = GuardrailPlan {
            entries: vec![GuardrailPlanEntry {
                definition: pii_redact_definition(),
            }],
        };

        let execution = executor
            .execute(
                &plan,
                GuardrailMode::PreCall,
                GuardrailContext::default(),
                Some(json!({
                    "messages": [{
                        "role": "user",
                        "content": "Email john@example.com, phone +15551234567, ssn 123-45-6789"
                    }]
                })),
                None,
            )
            .expect("redacted");
        let redacted = execution.request.expect("request").to_string();

        assert!(redacted.contains("[EMAIL_1]"));
        assert!(redacted.contains("[PHONE_1]"));
        assert!(redacted.contains("[SSN_1]"));
        assert!(!redacted.contains("john@example.com"));
        assert!(!execution.records[0]
            .metadata
            .to_string()
            .contains("john@example.com"));
    }

    #[test]
    fn pii_redact_supports_during_call_text_redaction() {
        let executor = builtin_guardrail_executor();
        let plan = GuardrailPlan {
            entries: vec![GuardrailPlanEntry {
                definition: pii_redact_definition(),
            }],
        };

        let execution = executor
            .execute(
                &plan,
                GuardrailMode::DuringCall,
                GuardrailContext::default(),
                None,
                Some(json!("data: {\"delta\":\"alice@example.com\"}\n\n")),
            )
            .expect("during call redaction");

        assert_eq!(
            execution.response,
            Some(json!("data: {\"delta\":\"[EMAIL_1]\"}\n\n"))
        );
    }

    #[test]
    fn redact_pii_text_returns_sanitized_counts() {
        let (redacted, metadata) = redact_pii_text("call +15551234567 or bob@example.com");

        assert!(redacted.contains("[PHONE_1]"));
        assert!(redacted.contains("[EMAIL_1]"));
        assert_eq!(metadata["phone_count"], 1);
        assert_eq!(metadata["email_count"], 1);
        assert!(!metadata.to_string().contains("bob@example.com"));
    }
}
