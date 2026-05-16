use crate::{GatewayError, GatewayResult, Provider, Route};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::BTreeMap, time::Instant};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GuardrailMode {
    PreCall,
    PostCall,
    DuringCall,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GuardrailAction {
    Allow,
    Block,
    Modify,
    Warn,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GuardrailFailurePolicy {
    FailClosed,
    FailOpen,
    DryRun,
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

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GuardrailPolicy {
    #[serde(default)]
    pub mandatory_guardrails: Vec<String>,
    #[serde(default)]
    pub optional_guardrails: Vec<String>,
    #[serde(default)]
    pub forbidden_guardrails: Vec<String>,
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
}

#[derive(Debug, Clone, PartialEq)]
pub struct GuardrailExecutionRecord {
    pub guardrail_name: String,
    pub mode: GuardrailMode,
    pub action: GuardrailAction,
    pub failure_policy: GuardrailFailurePolicy,
    pub latency_ms: u128,
    pub reason: Option<String>,
    pub metadata: Value,
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
        }
    }

    pub fn block(reason: impl Into<String>) -> Self {
        Self {
            action: GuardrailAction::Block,
            request: None,
            response: None,
            reason: Some(reason.into()),
            metadata: Value::Object(Default::default()),
        }
    }

    pub fn modify_request(request: Value) -> Self {
        Self {
            action: GuardrailAction::Modify,
            request: Some(request),
            response: None,
            reason: None,
            metadata: Value::Object(Default::default()),
        }
    }

    pub fn modify_response(response: Value) -> Self {
        Self {
            action: GuardrailAction::Modify,
            request: None,
            response: Some(response),
            reason: None,
            metadata: Value::Object(Default::default()),
        }
    }

    pub fn warn(reason: impl Into<String>) -> Self {
        Self {
            action: GuardrailAction::Warn,
            request: None,
            response: None,
            reason: Some(reason.into()),
            metadata: Value::Object(Default::default()),
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
}
