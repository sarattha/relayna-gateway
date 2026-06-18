use crate::body_rewrite::{
    prepare_rewritten_request_headers, prepare_rewritten_response_headers, BoundedBodyRewriter,
};
use async_trait::async_trait;
use bytes::Bytes;
use chrono::Utc;
use gateway_core::{
    auth::{Authenticator, VirtualKeyLookup},
    estimate_generation_tokens, evaluate_policy, evaluate_policy_limits,
    execution_events_from_records, extract_client_guardrails, extract_estimated_cost_usd,
    extract_generation_features, extract_model, extract_usage_tokens,
    guardrail_executor_for_definitions, is_retry_safe_status, redact_pii_text,
    resolve_guardrail_plan, route_pattern_wildcard_suffix, service_wildcard_suffix,
    strip_client_guardrails, validate_relayna_key_header_name, verify_apigee_trusted_identity,
    ApigeeTrustedHeaderConfig, AuthenticatedKey, BudgetDecision, BudgetStore, CredentialHeaderMode,
    EntraAuthConfig, EntraIdentityContext, GatewayAuthRuntimeConfig, GatewayAuthRuntimeSnapshot,
    GatewayError, GatewayResult, GuardrailContext, GuardrailDefinition, GuardrailExecutionEvent,
    GuardrailMode, GuardrailPlan, GuardrailPlanRequest, GuardrailPolicy, GuardrailPolicySet,
    GuardrailStore, KeyPolicy, LiteLlmSensitiveRouteExposure, OpenAiRouteMode,
    OpenAiRouteSettingsLookup, PolicyLookup, Provider, ProviderConfigLookup,
    ProviderIntelligenceStore, RateLimitDecision, RateLimitStore, Route, RouteMatch,
    ServiceRegistryLookup, ServiceRouteLookup, SharedGatewayAuthRuntime, UsageEvent, UsageRecorder,
    ENTRA_DEFAULT_RELAYNA_KEY_HEADER,
};
use http::{header::HeaderName, Uri};
use pingora_core::{
    upstreams::peer::HttpPeer, Error as PingoraError, ErrorSource, ErrorType,
    Result as PingoraResult,
};
use pingora_http::{RequestHeader, ResponseHeader};
use pingora_proxy::{ProxyHttp, Session};
use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    sync::Arc,
    time::{Duration, Instant},
};

#[derive(Debug, Clone)]
pub struct PingoraLiteLlmConfig {
    pub litellm: PingoraUpstreamConfig,
    pub direct_openai: Option<PingoraUpstreamConfig>,
    pub worker_token: Option<String>,
    pub entra_auth: Option<EntraAuthConfig>,
    pub apigee_trusted_header: Option<ApigeeTrustedHeaderConfig>,
    pub relayna_key_header: String,
    auth_runtime: Option<SharedGatewayAuthRuntime>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PingoraUpstreamConfig {
    pub host: String,
    pub port: u16,
    pub tls: bool,
    pub sni: String,
    pub service_key: String,
    pub credential_header_mode: CredentialHeaderMode,
    pub credential_header_name: Option<String>,
}

impl PingoraLiteLlmConfig {
    pub fn from_base_url(
        base_url: impl AsRef<str>,
        service_key: impl Into<String>,
    ) -> gateway_core::GatewayResult<Self> {
        Ok(Self {
            litellm: PingoraUpstreamConfig::from_base_url(base_url, service_key)?,
            direct_openai: None,
            worker_token: None,
            entra_auth: None,
            apigee_trusted_header: None,
            relayna_key_header: ENTRA_DEFAULT_RELAYNA_KEY_HEADER.to_owned(),
            auth_runtime: None,
        })
    }

    pub fn with_direct_openai(mut self, upstream: Option<PingoraUpstreamConfig>) -> Self {
        self.direct_openai = upstream;
        self
    }

    pub fn with_worker_token(mut self, worker_token: Option<String>) -> Self {
        self.worker_token = worker_token;
        self
    }

    pub fn with_relayna_key_header(
        mut self,
        relayna_key_header: impl Into<String>,
    ) -> gateway_core::GatewayResult<Self> {
        let relayna_key_header = relayna_key_header.into();
        validate_relayna_key_header_name(&relayna_key_header)?;
        self.relayna_key_header = relayna_key_header;
        Ok(self)
    }

    pub fn with_entra_auth(mut self, entra_auth: Option<EntraAuthConfig>) -> Self {
        if let Some(config) = entra_auth.as_ref() {
            self.relayna_key_header = config.relayna_key_header.clone();
        }
        self.entra_auth = entra_auth;
        self
    }

    pub fn with_apigee_trusted_header(
        mut self,
        apigee_trusted_header: Option<ApigeeTrustedHeaderConfig>,
    ) -> Self {
        self.apigee_trusted_header = apigee_trusted_header;
        self
    }

    pub fn with_auth_runtime(mut self, auth_runtime: SharedGatewayAuthRuntime) -> Self {
        self.auth_runtime = Some(auth_runtime);
        self
    }

    fn relayna_key_header(&self) -> &str {
        self.relayna_key_header.as_str()
    }
}

impl PingoraUpstreamConfig {
    pub fn from_base_url(
        base_url: impl AsRef<str>,
        service_key: impl Into<String>,
    ) -> gateway_core::GatewayResult<Self> {
        let url =
            url::Url::parse(base_url.as_ref()).map_err(|_| GatewayError::InvalidConfiguration)?;
        let host = url
            .host_str()
            .ok_or(GatewayError::InvalidConfiguration)?
            .to_owned();
        let tls = url.scheme() == "https";
        let port = url
            .port_or_known_default()
            .ok_or(GatewayError::InvalidConfiguration)?;

        Ok(Self {
            sni: host.clone(),
            host,
            port,
            tls,
            service_key: service_key.into(),
            credential_header_mode: CredentialHeaderMode::AuthorizationBearer,
            credential_header_name: None,
        })
    }

    fn with_litellm_credential_header(
        mut self,
        mode: CredentialHeaderMode,
        header_name: Option<String>,
    ) -> gateway_core::GatewayResult<Self> {
        if mode == CredentialHeaderMode::CustomHeader && header_name.as_deref().is_none() {
            return Err(GatewayError::InvalidConfiguration);
        }
        self.credential_header_mode = mode;
        self.credential_header_name = header_name.map(|value| value.trim().to_owned());
        Ok(self)
    }

    fn host_header_value(&self) -> String {
        let host = if self.host.contains(':') && !self.host.starts_with('[') {
            format!("[{}]", self.host)
        } else {
            self.host.clone()
        };

        if (self.tls && self.port == 443) || (!self.tls && self.port == 80) {
            host
        } else {
            format!("{host}:{}", self.port)
        }
    }
}

pub struct RelaynaPingoraProxy<S, R> {
    store: Arc<S>,
    control_state: Arc<R>,
    config: PingoraLiteLlmConfig,
    auth_runtime: SharedGatewayAuthRuntime,
}

impl<S, R> RelaynaPingoraProxy<S, R>
where
    S: VirtualKeyLookup
        + UsageRecorder
        + PolicyLookup
        + ServiceRegistryLookup
        + ServiceRouteLookup
        + OpenAiRouteSettingsLookup
        + GuardrailStore,
    R: RateLimitStore + BudgetStore,
{
    pub fn new(store: Arc<S>, control_state: Arc<R>, config: PingoraLiteLlmConfig) -> Self {
        let auth_runtime = config.auth_runtime.clone().unwrap_or_else(|| {
            SharedGatewayAuthRuntime::new(GatewayAuthRuntimeConfig {
                relayna_key_header: config.relayna_key_header.clone(),
                entra_auth: config.entra_auth.clone(),
                apigee_trusted_header: config.apigee_trusted_header.clone(),
            })
            .expect("validated gateway auth config")
        });
        Self {
            store,
            control_state,
            config,
            auth_runtime,
        }
    }
}

impl<S, R> RelaynaPingoraProxy<S, R> {
    async fn verify_entra_request(
        &self,
        req: &RequestHeader,
        now: chrono::DateTime<Utc>,
        auth: &GatewayAuthRuntimeSnapshot,
    ) -> GatewayResult<EntraIdentityContext> {
        if let Some(config) = auth.config.apigee_trusted_header.as_ref() {
            if header_value(req, "x-apigee-entra-identity").is_some()
                || header_value(req, "x-apigee-entra-signature").is_some()
            {
                return verify_apigee_trusted_identity(
                    header_value(req, "x-apigee-entra-identity"),
                    header_value(req, "x-apigee-entra-signature"),
                    config,
                );
            }
        }
        let verifier = auth
            .entra_verifier
            .as_ref()
            .ok_or(GatewayError::MissingEntraAuthorization)?;
        verifier
            .verify_authorization(header_value(req, "authorization"), now)
            .await
    }
}

#[derive(Debug)]
pub struct PingoraContext {
    started: Instant,
    request_id: String,
    route: Option<Route>,
    route_match: Option<RouteMatch>,
    key: Option<AuthenticatedKey>,
    entra_identity: Option<EntraIdentityContext>,
    relayna_key_header: String,
    body_prefix: Vec<u8>,
    body_bytes_seen: usize,
    response_body_prefix: Vec<u8>,
    response_bytes_seen: usize,
    policy: Option<KeyPolicy>,
    request_rewriter: Option<BoundedBodyRewriter>,
    response_rewriter: Option<BoundedBodyRewriter>,
    is_streaming: bool,
    first_chunk_recorded: bool,
    budget_reserved: bool,
    task_id: Option<String>,
    run_id: Option<String>,
    traceparent: Option<String>,
    trace_id: Option<String>,
    fallback_count: i32,
    terminal_usage_recorded: bool,
    service_upstream: Option<PingoraUpstreamConfig>,
    service_route_pattern: Option<String>,
    litellm_upstream: Option<PingoraUpstreamConfig>,
    litellm_passthrough: bool,
    guardrail_definitions: Vec<GuardrailDefinition>,
    guardrail_policy: GuardrailPolicy,
    pre_guardrail_plan: Option<GuardrailPlan>,
    post_guardrail_plan: Option<GuardrailPlan>,
    during_guardrail_plan: Option<GuardrailPlan>,
    guardrail_context: Option<GuardrailContext>,
    guardrail_events: Vec<GuardrailExecutionEvent>,
    guardrail_error: Option<GatewayError>,
    rewritten_request_len: Option<usize>,
    guardrail_stream_holdback: String,
}

#[async_trait]
impl<S, R> ProxyHttp for RelaynaPingoraProxy<S, R>
where
    S: VirtualKeyLookup
        + UsageRecorder
        + PolicyLookup
        + ServiceRegistryLookup
        + ServiceRouteLookup
        + OpenAiRouteSettingsLookup
        + ProviderConfigLookup
        + GuardrailStore
        + ProviderIntelligenceStore
        + Send
        + Sync
        + 'static,
    R: RateLimitStore + BudgetStore + Send + Sync + 'static,
{
    type CTX = PingoraContext;

    fn new_ctx(&self) -> Self::CTX {
        Self::CTX {
            started: Instant::now(),
            request_id: uuid::Uuid::new_v4().to_string(),
            route: None,
            route_match: None,
            key: None,
            entra_identity: None,
            relayna_key_header: self.config.relayna_key_header().to_owned(),
            body_prefix: Vec::new(),
            body_bytes_seen: 0,
            response_body_prefix: Vec::new(),
            response_bytes_seen: 0,
            policy: None,
            request_rewriter: None,
            response_rewriter: None,
            is_streaming: false,
            first_chunk_recorded: false,
            budget_reserved: false,
            task_id: None,
            run_id: None,
            traceparent: None,
            trace_id: None,
            fallback_count: 0,
            terminal_usage_recorded: false,
            service_upstream: None,
            service_route_pattern: None,
            litellm_upstream: None,
            litellm_passthrough: false,
            guardrail_definitions: Vec::new(),
            guardrail_policy: GuardrailPolicy::default(),
            pre_guardrail_plan: None,
            post_guardrail_plan: None,
            during_guardrail_plan: None,
            guardrail_context: None,
            guardrail_events: Vec::new(),
            guardrail_error: None,
            rewritten_request_len: None,
            guardrail_stream_holdback: String::new(),
        }
    }

    async fn request_filter(
        &self,
        session: &mut Session,
        ctx: &mut Self::CTX,
    ) -> PingoraResult<bool>
    where
        Self::CTX: Send + Sync,
    {
        let req = session.req_header();
        ctx.request_id = req
            .headers
            .get("x-request-id")
            .and_then(|value| value.to_str().ok())
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        gateway_telemetry::request_started();

        let persisted_service = if should_check_service_routes(req.uri.path()) {
            match self
                .store
                .service_registration_for_route(&req.method, req.uri.path())
                .await
            {
                Ok(registration) => registration,
                Err(error) => {
                    respond_error(session, error, &ctx.request_id).await?;
                    return Ok(true);
                }
            }
        } else {
            None
        };
        let mut matched = if let Some(registration) = persisted_service {
            let service_name = registration.name.clone();
            let upstream = match service_upstream_from_registration(&registration) {
                Ok(upstream) => upstream,
                Err(error) => {
                    respond_error(session, error, &ctx.request_id).await?;
                    return Ok(true);
                }
            };
            let mut matched = service_route_match_for_persisted_registration(
                &req.method,
                req.uri.path(),
                &service_name,
            );
            matched.timeout_ms = u64::try_from(registration.timeout_ms)
                .map_err(|_| pingora_core::Error::new(ErrorType::InternalError))?;
            matched.max_body_bytes = usize::try_from(registration.max_body_bytes)
                .map_err(|_| pingora_core::Error::new(ErrorType::InternalError))?;
            matched.estimated_cost_usd = registration.estimated_cost_usd;
            ctx.service_route_pattern = Some(registration.route_pattern);
            ctx.service_upstream = Some(upstream);
            matched
        } else {
            match Route::resolve_match(&req.method, req.uri.path()) {
                Ok(matched) => matched,
                Err(error) => match self.store.litellm_passthrough_settings().await {
                    Ok(settings) if settings.allows(&req.method, req.uri.path()) => {
                        ctx.litellm_passthrough = true;
                        RouteMatch {
                            route: Route::LiteLlmPassthrough,
                            backend: gateway_core::BackendType::LiteLlm,
                            provider: Provider::LiteLlm,
                            service_name: None,
                            timeout_ms: 120_000,
                            max_body_bytes: 1_048_576,
                            estimated_cost_usd: None,
                        }
                    }
                    Ok(_) => {
                        respond_error(session, error, &ctx.request_id).await?;
                        return Ok(true);
                    }
                    Err(error) => {
                        respond_error(session, error, &ctx.request_id).await?;
                        return Ok(true);
                    }
                },
            }
        };
        ctx.route = Some(matched.route);
        ctx.request_rewriter = Some(BoundedBodyRewriter::new(matched.max_body_bytes));
        ctx.response_rewriter = Some(BoundedBodyRewriter::new(matched.max_body_bytes));
        ctx.traceparent = header_value(req, "traceparent")
            .filter(|value| is_valid_traceparent(value))
            .map(ToOwned::to_owned);
        ctx.trace_id = ctx
            .traceparent
            .as_deref()
            .and_then(trace_id_from_traceparent);
        gateway_telemetry::gateway_request_span(
            &ctx.request_id,
            ctx.route.map(Route::as_str),
            Some(matched.provider.as_str()),
            ctx.trace_id.as_deref(),
        )
        .in_scope(|| tracing::info!("gateway request received"));
        if self.trusted_worker(req) {
            ctx.task_id = header_value(req, "x-relayna-task-id").map(ToOwned::to_owned);
            ctx.run_id = header_value(req, "x-relayna-run-id").map(ToOwned::to_owned);
        }

        let auth = match self.auth_runtime.snapshot() {
            Ok(auth) => auth,
            Err(error) => {
                respond_error(session, error, &ctx.request_id).await?;
                return Ok(true);
            }
        };
        ctx.relayna_key_header = auth.config.relayna_key_header.clone();
        let now = Utc::now();
        let authorization = header_value(req, "authorization");
        let key_result = if auth.entra_enabled() {
            match self.verify_entra_request(req, now, &auth).await {
                Ok(identity) => {
                    ctx.entra_identity = Some(identity);
                    gateway_telemetry::phase_span("gateway.auth.entra", &ctx.request_id)
                        .in_scope(|| tracing::info!("Entra identity authenticated"));
                }
                Err(error) => {
                    gateway_telemetry::record_auth_failure(error.code());
                    respond_error(session, error, &ctx.request_id).await?;
                    return Ok(true);
                }
            }
            Authenticator::new(self.store.clone())
                .authenticate_raw_key(header_value(req, &auth.config.relayna_key_header), now)
                .await
        } else {
            Authenticator::new(self.store.clone())
                .authenticate_authorization(authorization, now)
                .await
        };
        match key_result {
            Ok(key) => {
                gateway_telemetry::phase_span("gateway.auth.verify", &ctx.request_id)
                    .in_scope(|| tracing::info!("virtual key authenticated"));
                match self.store.list_guardrail_definitions().await {
                    Ok(definitions) => ctx.guardrail_definitions = definitions,
                    Err(error) => {
                        respond_error(session, error, &ctx.request_id).await?;
                        return Ok(true);
                    }
                }
                if let Some(service_name) = matched.service_name.as_deref() {
                    if ctx.service_upstream.is_some() {
                        ctx.route_match = Some(matched);
                        ctx.key = Some(key);
                        return Ok(false);
                    }
                    let registration = match self.store.service_registration(service_name).await {
                        Ok(Some(registration)) => registration,
                        Ok(None) => {
                            respond_error(session, GatewayError::MissingService, &ctx.request_id)
                                .await?;
                            return Ok(true);
                        }
                        Err(error) => {
                            respond_error(session, error, &ctx.request_id).await?;
                            return Ok(true);
                        }
                    };
                    if !registration
                        .allowed_methods
                        .iter()
                        .any(|method| method.eq_ignore_ascii_case(req.method.as_str()))
                    {
                        respond_error(session, GatewayError::UnsupportedRoute, &ctx.request_id)
                            .await?;
                        return Ok(true);
                    }
                    let upstream = match service_upstream_from_registration(&registration) {
                        Ok(upstream) => upstream,
                        Err(error) => {
                            respond_error(session, error, &ctx.request_id).await?;
                            return Ok(true);
                        }
                    };
                    matched.timeout_ms = u64::try_from(registration.timeout_ms)
                        .map_err(|_| pingora_core::Error::new(ErrorType::InternalError))?;
                    matched.max_body_bytes = usize::try_from(registration.max_body_bytes)
                        .map_err(|_| pingora_core::Error::new(ErrorType::InternalError))?;
                    matched.estimated_cost_usd = registration.estimated_cost_usd;
                    ctx.service_route_pattern = Some(registration.route_pattern);
                    ctx.service_upstream = Some(upstream);
                }
                if matches!(
                    matched.route,
                    Route::ChatCompletions | Route::Responses | Route::LiteLlmEmbeddings
                ) {
                    match self.store.openai_route_mode(matched.route).await {
                        Ok(OpenAiRouteMode::DirectLiteLlmPassthrough) => {
                            ctx.litellm_passthrough = true;
                        }
                        Ok(OpenAiRouteMode::ManagedByGateway) => {}
                        Err(error) => {
                            respond_error(session, error, &ctx.request_id).await?;
                            return Ok(true);
                        }
                    }
                }
                if ctx.litellm_passthrough && matched.route == Route::LiteLlmPassthrough {
                    match self.store.litellm_passthrough_settings().await {
                        Ok(settings) => {
                            if !sensitive_litellm_passthrough_authorized(
                                settings.sensitive_exposure_for_path(req.uri.path()),
                                ctx.entra_identity.as_ref(),
                            ) {
                                respond_error(
                                    session,
                                    GatewayError::InsufficientEntraAuthorization,
                                    &ctx.request_id,
                                )
                                .await?;
                                return Ok(true);
                            }
                        }
                        Err(error) => {
                            respond_error(session, error, &ctx.request_id).await?;
                            return Ok(true);
                        }
                    }
                }
                if matched.provider == Provider::LiteLlm {
                    let litellm_config = match self.store.active_litellm_config().await {
                        Ok(config) => config,
                        Err(error) => {
                            respond_error(session, error, &ctx.request_id).await?;
                            return Ok(true);
                        }
                    };
                    let mapped_credential = match self
                        .store
                        .litellm_credential_mapping_for_context(key.key_id, key.project_id)
                        .await
                    {
                        Ok(mapping) => mapping.map(|mapping| mapping.credential),
                        Err(error) => {
                            respond_error(session, error, &ctx.request_id).await?;
                            return Ok(true);
                        }
                    };
                    let has_mapped_credential = mapped_credential.is_some();
                    let selected_credential = mapped_credential
                        .or_else(|| {
                            litellm_config
                                .as_ref()
                                .and_then(|config| config.credential.clone())
                        })
                        .unwrap_or_else(|| self.config.litellm.service_key.clone());
                    if selected_credential.trim().is_empty() {
                        respond_error(session, GatewayError::InvalidConfiguration, &ctx.request_id)
                            .await?;
                        return Ok(true);
                    }
                    if let Some(config) = litellm_config {
                        let upstream = match PingoraUpstreamConfig::from_base_url(
                            &config.base_url,
                            selected_credential,
                        )
                        .and_then(|upstream| {
                            upstream.with_litellm_credential_header(
                                config.credential_header_mode,
                                config.credential_header_name,
                            )
                        }) {
                            Ok(upstream) => upstream,
                            Err(error) => {
                                respond_error(session, error, &ctx.request_id).await?;
                                return Ok(true);
                            }
                        };
                        ctx.litellm_upstream = Some(upstream);
                    } else if has_mapped_credential {
                        let mut upstream = self.config.litellm.clone();
                        upstream.service_key = selected_credential;
                        ctx.litellm_upstream = Some(upstream);
                    }
                }
                ctx.route_match = Some(matched);
                ctx.key = Some(key);
                Ok(false)
            }
            Err(error) => {
                gateway_telemetry::record_auth_failure(error.code());
                respond_error(session, error, &ctx.request_id).await?;
                Ok(true)
            }
        }
    }

    async fn request_body_filter(
        &self,
        session: &mut Session,
        body: &mut Option<Bytes>,
        end_of_stream: bool,
        ctx: &mut Self::CTX,
    ) -> PingoraResult<()>
    where
        Self::CTX: Send + Sync,
    {
        if let Some(chunk) = body.as_ref() {
            ctx.body_bytes_seen = ctx.body_bytes_seen.saturating_add(chunk.len());
            if ctx.body_prefix.len() < 65_536 {
                let remaining = 65_536 - ctx.body_prefix.len();
                ctx.body_prefix
                    .extend_from_slice(&chunk[..chunk.len().min(remaining)]);
            }
            if let Some(matched) = &ctx.route_match {
                if ctx.body_bytes_seen > matched.max_body_bytes {
                    respond_error(session, GatewayError::RequestBodyTooLarge, &ctx.request_id)
                        .await?;
                }
            }
        }
        if ctx.litellm_passthrough {
            return Ok(());
        }
        let Some(rewriter) = ctx.request_rewriter.as_mut() else {
            return Ok(());
        };

        let key = ctx.key.clone();
        let route = ctx.route;
        let route_match = ctx.route_match.clone();
        let request_id = ctx.request_id.clone();
        let definitions = ctx.guardrail_definitions.clone();
        let mut policy = ctx.guardrail_policy.clone();
        if end_of_stream {
            let raw_body = match rewriter.preview_with_chunk(body.as_ref()) {
                Ok(raw_body) => raw_body,
                Err(error) => {
                    ctx.guardrail_error = Some(error);
                    *body = Some(Bytes::new());
                    return Ok(());
                }
            };
            let features = extract_generation_features(&raw_body);
            if let Some(key) = key.as_ref() {
                match self
                    .store
                    .effective_policy_for_context(
                        key.key_id,
                        key.project_id,
                        None,
                        route,
                        features.model.clone(),
                    )
                    .await
                {
                    Ok(effective) => {
                        policy = effective.guardrail_policy;
                        ctx.guardrail_policy = policy.clone();
                    }
                    Err(error) => {
                        ctx.guardrail_error = Some(error);
                        *body = Some(Bytes::new());
                        return Ok(());
                    }
                }
            }
        }
        let mut guardrail_context = ctx.guardrail_context.clone();
        let mut pre_plan = None;
        let mut post_plan = None;
        let mut during_plan = None;
        let mut guardrail_events = Vec::new();
        let mut guardrail_error = None;

        let result = rewriter.filter_chunk(body, end_of_stream, |raw_body| {
            if !end_of_stream {
                return Ok(raw_body.to_vec());
            }
            let mut request_json = match serde_json::from_slice::<serde_json::Value>(raw_body) {
                Ok(value) => value,
                Err(_) => return Ok(raw_body.to_vec()),
            };
            let client_requested = extract_client_guardrails(&request_json)?;
            let features = extract_generation_features(raw_body);
            let plan = resolve_guardrail_plan(GuardrailPlanRequest {
                mode: GuardrailMode::PreCall,
                definitions: definitions.clone(),
                policies: GuardrailPolicySet {
                    key_policy: policy.clone(),
                    ..GuardrailPolicySet::default()
                },
                client_requested_guardrails: client_requested.clone(),
            })?;
            let post_call_plan = resolve_guardrail_plan(GuardrailPlanRequest {
                mode: GuardrailMode::PostCall,
                definitions: definitions.clone(),
                policies: GuardrailPolicySet {
                    key_policy: policy.clone(),
                    ..GuardrailPolicySet::default()
                },
                client_requested_guardrails: client_requested.clone(),
            })?;
            let response_plan = if features.stream {
                let during_call_plan = resolve_guardrail_plan(GuardrailPlanRequest {
                    mode: GuardrailMode::DuringCall,
                    definitions: definitions.clone(),
                    policies: GuardrailPolicySet {
                        key_policy: policy,
                        ..GuardrailPolicySet::default()
                    },
                    client_requested_guardrails: client_requested,
                })?;
                if !guardrail_plan_names_match(&post_call_plan, &during_call_plan) {
                    guardrail_error = Some(GatewayError::GuardrailUnavailable);
                    return Ok(Vec::new());
                }
                during_call_plan
            } else {
                post_call_plan
            };
            if plan.entries.is_empty() && response_plan.entries.is_empty() {
                return Ok(raw_body.to_vec());
            }
            strip_client_guardrails(&mut request_json);
            let key = key.as_ref().ok_or(GatewayError::MissingAuthorization)?;
            let mut context = guardrail_context
                .take()
                .unwrap_or_else(|| GuardrailContext {
                    request_id: request_id.clone(),
                    key_id: Some(key.key_id),
                    project_id: key.project_id,
                    route,
                    provider: route_match.as_ref().map(|matched| matched.provider),
                    model: extract_model(raw_body),
                    ..GuardrailContext::default()
                });
            let executor = guardrail_executor_for_definitions(&definitions);
            let execution = executor.execute(
                &plan,
                GuardrailMode::PreCall,
                context,
                Some(request_json),
                None,
            )?;
            context = execution.context;
            guardrail_events.extend(execution_events_from_records(
                &context,
                &execution.records,
                Utc::now(),
            ));
            pre_plan = Some(plan);
            if features.stream {
                during_plan = Some(response_plan);
            } else {
                post_plan = Some(response_plan);
            }
            guardrail_context = Some(context);
            serde_json::to_vec(&execution.request.unwrap_or(serde_json::Value::Null))
                .map_err(|_| GatewayError::InvalidGuardrailRequest)
        });

        if let Err(error) = result {
            ctx.guardrail_error = Some(error);
            *body = Some(Bytes::new());
            return Ok(());
        }
        if end_of_stream {
            if let Some(error) = guardrail_error {
                ctx.guardrail_error = Some(error);
            }
            ctx.pre_guardrail_plan = pre_plan;
            ctx.post_guardrail_plan = post_plan;
            ctx.during_guardrail_plan = during_plan;
            ctx.guardrail_context = guardrail_context;
            ctx.guardrail_events.extend(guardrail_events);
            if let Some(body) = body.as_ref() {
                ctx.rewritten_request_len = Some(body.len());
                ctx.body_prefix.clear();
                ctx.body_prefix
                    .extend_from_slice(&body[..body.len().min(65_536)]);
            }
        }
        Ok(())
    }

    async fn proxy_upstream_filter(
        &self,
        session: &mut Session,
        ctx: &mut Self::CTX,
    ) -> PingoraResult<bool>
    where
        Self::CTX: Send + Sync,
    {
        let Some(matched) = ctx.route_match.clone() else {
            respond_error(session, GatewayError::UnsupportedRoute, &ctx.request_id).await?;
            return Ok(false);
        };
        let route = matched.route;
        let Some(key) = ctx.key.clone() else {
            respond_error(session, GatewayError::MissingAuthorization, &ctx.request_id).await?;
            return Ok(false);
        };
        if self.upstream_for(ctx).is_none() {
            respond_error(session, GatewayError::InvalidConfiguration, &ctx.request_id).await?;
            return Ok(false);
        }
        if let Some(error) = ctx.guardrail_error.clone() {
            self.record_terminal_usage(ctx, &key, route, error.status_code().as_u16(), Utc::now())
                .await;
            respond_error(session, error, &ctx.request_id).await?;
            return Ok(false);
        }

        let now = Utc::now();
        if let Err(error) = self.ensure_openai_route_enabled(route).await {
            self.record_terminal_usage(ctx, &key, route, error.status_code().as_u16(), now)
                .await;
            respond_error(session, error, &ctx.request_id).await?;
            return Ok(false);
        }
        if bypass_gateway_governance_for_passthrough(route, ctx.litellm_passthrough) {
            gateway_telemetry::record_provider_selection();
            return Ok(true);
        }

        let mut features = extract_generation_features(&ctx.body_prefix);
        if features.service_name.is_none() {
            features.service_name = matched.service_name.clone();
        }
        ctx.is_streaming = features.stream;
        let policy = match self
            .store
            .policy_for_context(
                key.key_id,
                key.project_id,
                None,
                Some(route),
                features.model.clone(),
            )
            .await
        {
            Ok(policy) => policy,
            Err(error) => {
                self.record_terminal_usage(ctx, &key, route, error.status_code().as_u16(), now)
                    .await;
                respond_error(session, error, &ctx.request_id).await?;
                return Ok(false);
            }
        };

        if let Err(error) = evaluate_policy(&policy, route, matched.provider, &features) {
            gateway_telemetry::record_policy_denial(route.as_str(), error.code());
            self.record_terminal_usage(ctx, &key, route, error.status_code().as_u16(), now)
                .await;
            respond_error(session, error, &ctx.request_id).await?;
            return Ok(false);
        }
        let estimated_tokens = estimate_generation_tokens(&ctx.body_prefix);
        if let Err(error) = evaluate_policy_limits(
            &policy,
            now,
            i64::try_from(ctx.body_bytes_seen).ok(),
            None,
            i32::try_from(estimated_tokens).ok(),
            None,
            matched.estimated_cost_usd,
        ) {
            gateway_telemetry::record_policy_denial(route.as_str(), error.code());
            self.record_terminal_usage(ctx, &key, route, error.status_code().as_u16(), now)
                .await;
            respond_error(session, error, &ctx.request_id).await?;
            return Ok(false);
        }
        ctx.policy = Some(policy.clone());

        match self
            .control_state
            .check_request_rate_limit(key.key_id, policy.rpm_limit, now)
            .await
        {
            Ok(RateLimitDecision::Allowed { .. }) => {}
            Ok(RateLimitDecision::Exceeded {
                retry_after_seconds,
                ..
            }) => {
                gateway_telemetry::record_rate_limit_rejection(route.as_str(), "request");
                let error = GatewayError::RateLimitExceeded {
                    retry_after_seconds,
                };
                self.record_terminal_usage(ctx, &key, route, error.status_code().as_u16(), now)
                    .await;
                respond_error(session, error, &ctx.request_id).await?;
                return Ok(false);
            }
            Err(error) => {
                self.record_terminal_usage(ctx, &key, route, error.status_code().as_u16(), now)
                    .await;
                respond_error(session, error, &ctx.request_id).await?;
                return Ok(false);
            }
        }

        match self
            .control_state
            .check_token_rate_limit(key.key_id, policy.tpm_limit, estimated_tokens, now)
            .await
        {
            Ok(RateLimitDecision::Allowed { .. }) => {}
            Ok(RateLimitDecision::Exceeded {
                retry_after_seconds,
                ..
            }) => {
                gateway_telemetry::record_rate_limit_rejection(route.as_str(), "token");
                let error = GatewayError::TokenRateLimitExceeded {
                    retry_after_seconds,
                };
                self.record_terminal_usage(ctx, &key, route, error.status_code().as_u16(), now)
                    .await;
                respond_error(session, error, &ctx.request_id).await?;
                return Ok(false);
            }
            Err(error) => {
                self.record_terminal_usage(ctx, &key, route, error.status_code().as_u16(), now)
                    .await;
                respond_error(session, error, &ctx.request_id).await?;
                return Ok(false);
            }
        }

        match self
            .control_state
            .check_budget(
                key.key_id,
                policy.daily_budget_usd,
                policy.monthly_budget_usd,
                now,
            )
            .await
        {
            Ok(BudgetDecision::Allowed(_)) => {
                if let Some(estimated_cost_usd) = matched.estimated_cost_usd {
                    if let Err(error) = self
                        .control_state
                        .reserve_budget(key.key_id, &ctx.request_id, estimated_cost_usd, now)
                        .await
                    {
                        self.record_terminal_usage(
                            ctx,
                            &key,
                            route,
                            error.status_code().as_u16(),
                            now,
                        )
                        .await;
                        respond_error(session, error, &ctx.request_id).await?;
                        return Ok(false);
                    }
                    ctx.budget_reserved = true;
                    if ctx.is_streaming {
                        gateway_telemetry::stream_started();
                    }
                }
                gateway_telemetry::record_provider_selection();
                Ok(true)
            }
            Ok(BudgetDecision::Exceeded(_)) => {
                gateway_telemetry::record_budget_rejection(route.as_str(), "spend");
                let error = GatewayError::BudgetExceeded;
                self.record_terminal_usage(ctx, &key, route, error.status_code().as_u16(), now)
                    .await;
                respond_error(session, error, &ctx.request_id).await?;
                Ok(false)
            }
            Err(error) => {
                self.record_terminal_usage(ctx, &key, route, error.status_code().as_u16(), now)
                    .await;
                respond_error(session, error, &ctx.request_id).await?;
                Ok(false)
            }
        }
    }

    async fn upstream_peer(
        &self,
        _session: &mut Session,
        ctx: &mut Self::CTX,
    ) -> PingoraResult<Box<HttpPeer>> {
        let upstream = self.upstream_for(ctx).unwrap_or(&self.config.litellm);
        let addr = format!("{}:{}", upstream.host, upstream.port);
        let mut peer = HttpPeer::new(addr, upstream.tls, upstream.sni.clone());
        if let Some(matched) = &ctx.route_match {
            let timeout = Duration::from_millis(matched.timeout_ms);
            peer.options.connection_timeout = Some(timeout);
            peer.options.total_connection_timeout = Some(timeout);
            peer.options.read_timeout = Some(timeout);
            peer.options.write_timeout = Some(timeout);
        }
        Ok(Box::new(peer))
    }

    async fn upstream_request_filter(
        &self,
        _session: &mut Session,
        upstream_request: &mut RequestHeader,
        ctx: &mut Self::CTX,
    ) -> PingoraResult<()>
    where
        Self::CTX: Send + Sync,
    {
        let upstream = self.upstream_for(ctx).unwrap_or(&self.config.litellm);
        prepare_upstream_authority_and_credentials(
            upstream_request,
            upstream,
            Some(ctx.relayna_key_header.as_str()),
        )?;
        if ctx
            .route_match
            .as_ref()
            .is_some_and(|matched| matched.provider == Provider::OpenAiCompatible)
        {
            rewrite_direct_openai_uri(upstream_request)?;
        }
        if let Some(matched) = &ctx.route_match {
            if matched.route == Route::ServiceWildcard {
                if let Some(service_name) = matched.service_name.as_deref() {
                    rewrite_service_wildcard_uri(
                        upstream_request,
                        service_name,
                        ctx.service_route_pattern.as_deref(),
                    )?;
                }
            }
        }
        upstream_request.insert_header("x-relayna-request-id", &ctx.request_id)?;
        if let Some(traceparent) = &ctx.traceparent {
            upstream_request.insert_header("traceparent", traceparent)?;
        }

        if let Some(key) = &ctx.key {
            upstream_request.insert_header("x-relayna-key-id", key.key_id.to_string())?;
            if let Some(project_id) = key.project_id {
                upstream_request.insert_header("x-relayna-project-id", project_id.to_string())?;
            }
        }
        if let Some(task_id) = &ctx.task_id {
            upstream_request.insert_header("x-relayna-task-id", task_id)?;
        }
        if let Some(run_id) = &ctx.run_id {
            upstream_request.insert_header("x-relayna-run-id", run_id)?;
        }
        if let Some(matched) = &ctx.route_match {
            if let Some(service_name) = &matched.service_name {
                upstream_request.insert_header("x-relayna-service", service_name)?;
            }
        }
        if ctx
            .pre_guardrail_plan
            .as_ref()
            .is_some_and(|plan| !plan.entries.is_empty())
        {
            if let Some(rewritten_len) = ctx.rewritten_request_len {
                prepare_rewritten_request_headers(upstream_request, rewritten_len);
            }
        }

        Ok(())
    }

    async fn upstream_response_filter(
        &self,
        _session: &mut Session,
        upstream_response: &mut ResponseHeader,
        ctx: &mut Self::CTX,
    ) -> PingoraResult<()>
    where
        Self::CTX: Send + Sync,
    {
        let status_code = upstream_response.status.as_u16();
        if is_retry_safe_status(status_code) && self.activate_provider_fallback(ctx) {
            let mut error = PingoraError::new_up(ErrorType::HTTPStatus(status_code));
            error.set_retry(true);
            return Err(error);
        }
        Ok(())
    }

    async fn response_filter(
        &self,
        session: &mut Session,
        upstream_response: &mut ResponseHeader,
        ctx: &mut Self::CTX,
    ) -> PingoraResult<()>
    where
        Self::CTX: Send + Sync,
    {
        upstream_response.remove_header("alt-svc");
        let has_post_guardrails = ctx
            .post_guardrail_plan
            .as_ref()
            .is_some_and(|plan| !plan.entries.is_empty());
        let has_during_guardrails = ctx
            .during_guardrail_plan
            .as_ref()
            .is_some_and(|plan| !plan.entries.is_empty());
        if has_post_guardrails || has_during_guardrails {
            upstream_response.insert_header(
                "x-relayna-applied-guardrails",
                applied_guardrails_header(ctx),
            )?;
        }
        if has_post_guardrails {
            prepare_rewritten_response_headers(
                upstream_response,
                !session.as_downstream().is_http2(),
            );
        }
        Ok(())
    }

    fn response_body_filter(
        &self,
        _session: &mut Session,
        body: &mut Option<Bytes>,
        end_of_stream: bool,
        ctx: &mut Self::CTX,
    ) -> PingoraResult<Option<std::time::Duration>>
    where
        Self::CTX: Send + Sync,
    {
        if ctx
            .during_guardrail_plan
            .as_ref()
            .is_some_and(|plan| !plan.entries.is_empty())
        {
            apply_streaming_guardrails(body, end_of_stream, ctx);
        }
        if let Err(error) = apply_post_call_guardrails(body, end_of_stream, ctx) {
            ctx.guardrail_error = Some(error);
            *body = Some(Bytes::new());
            return Err(PingoraError::new(ErrorType::InternalError));
        }
        if let Some(body) = body.as_ref() {
            ctx.response_bytes_seen = ctx.response_bytes_seen.saturating_add(body.len());
            if let Some(policy) = &ctx.policy {
                if let Err(error) = evaluate_policy_limits(
                    policy,
                    Utc::now(),
                    None,
                    i64::try_from(ctx.response_bytes_seen).ok(),
                    None,
                    None,
                    None,
                ) {
                    ctx.guardrail_error = Some(error);
                    return Err(PingoraError::new(ErrorType::InternalError));
                }
            }
            observe_response_body_chunk(ctx, body);
            if end_of_stream {
                if let Some(policy) = &ctx.policy {
                    let (_, output_tokens, _) = extract_usage_tokens(&ctx.response_body_prefix);
                    if let Err(error) = evaluate_policy_limits(
                        policy,
                        Utc::now(),
                        None,
                        None,
                        None,
                        output_tokens.and_then(|tokens| i32::try_from(tokens).ok()),
                        extract_estimated_cost_usd(&ctx.response_body_prefix),
                    ) {
                        ctx.guardrail_error = Some(error);
                        return Err(PingoraError::new(ErrorType::InternalError));
                    }
                }
            }
        }
        Ok(None)
    }

    async fn logging(
        &self,
        session: &mut Session,
        error: Option<&pingora_core::Error>,
        ctx: &mut Self::CTX,
    ) {
        gateway_telemetry::request_finished();
        let Some(route) = ctx.route else {
            return;
        };
        let Some(key) = &ctx.key else {
            return;
        };
        if ctx.terminal_usage_recorded {
            return;
        }

        let status_code = session
            .response_written()
            .map(|response| response.status.as_u16())
            .unwrap_or_else(|| if error.is_some() { 502 } else { 500 });
        let estimated_cost_usd = if ctx.litellm_passthrough {
            None
        } else {
            extract_estimated_cost_usd(&ctx.response_body_prefix).or_else(|| {
                ctx.route_match
                    .as_ref()
                    .and_then(|matched| matched.estimated_cost_usd)
            })
        };
        let (input_tokens, output_tokens, total_tokens) = if ctx.litellm_passthrough {
            (None, None, None)
        } else {
            extract_usage_tokens(&ctx.response_body_prefix)
        };
        let latency_ms = i64::try_from(ctx.started.elapsed().as_millis()).unwrap_or(i64::MAX);
        let provider = provider_for_usage(ctx);
        let event = UsageEvent::new(
            &ctx.request_id,
            key,
            route,
            extract_model(&ctx.body_prefix),
            status_code,
            latency_ms,
            Utc::now(),
        )
        .with_provider(provider)
        .with_usage_tokens(input_tokens, output_tokens, total_tokens)
        .with_estimated_cost_usd(estimated_cost_usd)
        .with_service_name(
            ctx.route_match
                .as_ref()
                .and_then(|matched| matched.service_name.clone()),
        )
        .with_task_context(ctx.task_id.clone(), ctx.run_id.clone())
        .with_trace_id(ctx.trace_id.clone())
        .with_fallback_count(ctx.fallback_count);
        let _ = self.store.insert_usage_event(&event).await;
        let _ = self
            .store
            .insert_debug_bundle(debug_bundle_for_ctx(ctx, status_code))
            .await;
        gateway_telemetry::record_request_with_dimensions(
            route.as_str(),
            provider.as_str(),
            status_code,
            u64::try_from(latency_ms.max(0)).unwrap_or(u64::MAX),
            ctx.is_streaming,
        );
        gateway_telemetry::record_upstream_duration_ms(
            route.as_str(),
            provider.as_str(),
            ctx.is_streaming,
            u64::try_from(latency_ms.max(0)).unwrap_or(u64::MAX),
        );
        if let Some(tokens) = event.total_tokens {
            gateway_telemetry::record_tokens(tokens);
        }
        if let Some(estimated_cost_usd) = estimated_cost_usd {
            gateway_telemetry::record_estimated_cost_usd(estimated_cost_usd);
        }
        if let Some(estimated_cost_usd) = estimated_cost_usd {
            if ctx.budget_reserved {
                let _ = self
                    .control_state
                    .reconcile_budget_reservation(
                        key.key_id,
                        &ctx.request_id,
                        estimated_cost_usd,
                        Utc::now(),
                    )
                    .await;
            } else {
                let _ = self
                    .control_state
                    .add_budget_spend(key.key_id, estimated_cost_usd, Utc::now())
                    .await;
            }
        } else if ctx.budget_reserved {
            let _ = self
                .control_state
                .release_budget_reservation(key.key_id, &ctx.request_id)
                .await;
        }
        for event in &ctx.guardrail_events {
            gateway_telemetry::record_guardrail_execution(
                &event.guardrail_name,
                event.mode.as_str(),
                event.action.as_str(),
                event.failure_policy.as_str(),
                u64::try_from(event.latency_ms.max(0)).unwrap_or(u64::MAX),
                event.reason.is_some(),
            );
            let _ = self.store.insert_guardrail_execution_event(event).await;
        }
        if ctx.is_streaming {
            gateway_telemetry::stream_finished(error.is_some() || status_code >= 500);
        }
    }

    fn fail_to_connect(
        &self,
        _session: &mut Session,
        _peer: &HttpPeer,
        ctx: &mut Self::CTX,
        mut error: Box<PingoraError>,
    ) -> Box<PingoraError> {
        if self.activate_provider_fallback(ctx) {
            error.set_retry(true);
        }
        error
    }

    fn error_while_proxy(
        &self,
        _peer: &HttpPeer,
        _session: &mut Session,
        mut error: Box<PingoraError>,
        ctx: &mut Self::CTX,
        _client_reused: bool,
    ) -> Box<PingoraError> {
        if is_retry_safe_proxy_error(&error) && self.activate_provider_fallback(ctx) {
            error.set_retry(true);
        }
        error
    }
}

impl<S, R> RelaynaPingoraProxy<S, R>
where
    S: UsageRecorder + GuardrailStore,
    R: BudgetStore,
{
    fn upstream_for<'a>(&'a self, ctx: &'a PingoraContext) -> Option<&'a PingoraUpstreamConfig> {
        if ctx.route_match.is_none() {
            return Some(&self.config.litellm);
        }
        match provider_for_usage(ctx) {
            Provider::LiteLlm => ctx.litellm_upstream.as_ref().or(Some(&self.config.litellm)),
            Provider::OpenAiCompatible => self.config.direct_openai.as_ref(),
            Provider::InternalService => ctx.service_upstream.as_ref(),
        }
    }

    fn activate_provider_fallback(&self, ctx: &mut PingoraContext) -> bool {
        let Some(matched) = &ctx.route_match else {
            return false;
        };
        if matched.provider != Provider::OpenAiCompatible || ctx.fallback_count > 0 {
            return false;
        }
        if self.config.direct_openai.is_none() {
            return false;
        }
        ctx.fallback_count = 1;
        gateway_telemetry::record_provider_fallback_with_dimensions(
            matched.provider.as_str(),
            Provider::LiteLlm.as_str(),
            "proxy_error",
        );
        true
    }

    fn trusted_worker(&self, req: &RequestHeader) -> bool {
        let Some(expected) = self.config.worker_token.as_deref() else {
            return false;
        };
        header_value(req, "x-relayna-worker-token")
            .is_some_and(|actual| constant_time_eq(actual.as_bytes(), expected.as_bytes()))
    }
}

fn constant_time_eq(actual: &[u8], expected: &[u8]) -> bool {
    let mut diff = actual.len() ^ expected.len();
    let max_len = actual.len().max(expected.len());
    for index in 0..max_len {
        let actual_byte = actual.get(index).copied().unwrap_or(0);
        let expected_byte = expected.get(index).copied().unwrap_or(0);
        diff |= usize::from(actual_byte ^ expected_byte);
    }
    diff == 0
}

impl<S, R> RelaynaPingoraProxy<S, R>
where
    S: OpenAiRouteSettingsLookup,
{
    async fn ensure_openai_route_enabled(&self, route: Route) -> GatewayResult<()> {
        if self.store.openai_route_enabled(route).await? {
            Ok(())
        } else {
            Err(GatewayError::DisabledRoute)
        }
    }
}

impl<S, R> RelaynaPingoraProxy<S, R>
where
    S: UsageRecorder + ProviderIntelligenceStore,
    R: BudgetStore,
{
    async fn record_terminal_usage(
        &self,
        ctx: &mut PingoraContext,
        key: &AuthenticatedKey,
        route: Route,
        status_code: u16,
        now: chrono::DateTime<Utc>,
    ) {
        if ctx.terminal_usage_recorded {
            return;
        }

        let latency_ms = i64::try_from(ctx.started.elapsed().as_millis()).unwrap_or(i64::MAX);
        let provider = provider_for_usage(ctx);
        let estimated_cost_usd = if ctx.litellm_passthrough {
            None
        } else {
            ctx.route_match
                .as_ref()
                .and_then(|matched| matched.estimated_cost_usd)
        };
        let event = UsageEvent::new(
            &ctx.request_id,
            key,
            route,
            extract_model(&ctx.body_prefix),
            status_code,
            latency_ms,
            now,
        )
        .with_provider(provider)
        .with_estimated_cost_usd(estimated_cost_usd)
        .with_service_name(
            ctx.route_match
                .as_ref()
                .and_then(|matched| matched.service_name.clone()),
        )
        .with_task_context(ctx.task_id.clone(), ctx.run_id.clone())
        .with_trace_id(ctx.trace_id.clone())
        .with_fallback_count(ctx.fallback_count);
        let _ = self.store.insert_usage_event(&event).await;
        let _ = self
            .store
            .insert_debug_bundle(debug_bundle_for_ctx(ctx, status_code))
            .await;
        gateway_telemetry::record_request_with_dimensions(
            route.as_str(),
            provider.as_str(),
            status_code,
            u64::try_from(latency_ms.max(0)).unwrap_or(u64::MAX),
            ctx.is_streaming,
        );
        if ctx.budget_reserved {
            let _ = self
                .control_state
                .release_budget_reservation(key.key_id, &ctx.request_id)
                .await;
        }
        if ctx.is_streaming {
            gateway_telemetry::stream_finished(true);
        }
        ctx.terminal_usage_recorded = true;
    }
}

fn header_value<'a>(req: &'a RequestHeader, name: &str) -> Option<&'a str> {
    req.headers.get(name).and_then(|value| value.to_str().ok())
}

fn is_valid_traceparent(value: &str) -> bool {
    let parts: Vec<&str> = value.split('-').collect();
    parts.len() == 4
        && parts[0].len() == 2
        && parts[1].len() == 32
        && parts[2].len() == 16
        && parts[3].len() == 2
        && parts
            .iter()
            .all(|part| part.chars().all(|character| character.is_ascii_hexdigit()))
}

fn prepare_upstream_authority_and_credentials(
    upstream_request: &mut RequestHeader,
    upstream: &PingoraUpstreamConfig,
    relayna_key_header: Option<&str>,
) -> PingoraResult<()> {
    upstream_request.remove_header("authorization");
    upstream_request.remove_header("host");
    upstream_request.remove_header("x-apigee-entra-identity");
    upstream_request.remove_header("x-apigee-entra-signature");
    upstream_request.remove_header("proxy-authorization");
    if let Some(relayna_key_header) = relayna_key_header {
        upstream_request.remove_header(relayna_key_header);
    }
    upstream_request.remove_header("x-relayna-key");
    upstream_request.remove_header("x-aih-api-key");
    upstream_request.remove_header("x-api-key");
    upstream_request.remove_header("x-relayna-worker-token");
    if let Some(header_name) = upstream.credential_header_name.as_deref() {
        upstream_request.remove_header(header_name);
    }
    upstream_request.insert_header("host", upstream.host_header_value())?;
    match upstream.credential_header_mode {
        CredentialHeaderMode::AuthorizationBearer => {
            upstream_request
                .insert_header("authorization", format!("Bearer {}", upstream.service_key))?;
        }
        CredentialHeaderMode::CustomHeader => {
            let header_name = upstream
                .credential_header_name
                .as_deref()
                .ok_or_else(|| pingora_core::Error::new(ErrorType::InternalError))?;
            let header_name = HeaderName::from_bytes(header_name.as_bytes())
                .map_err(|_| pingora_core::Error::new(ErrorType::InternalError))?;
            upstream_request.insert_header(header_name, upstream.service_key.clone())?;
        }
    }
    Ok(())
}

fn rewrite_direct_openai_uri(upstream_request: &mut RequestHeader) -> PingoraResult<()> {
    let Some(path_and_query) = upstream_request
        .uri
        .path_and_query()
        .map(|value| value.as_str())
    else {
        return Ok(());
    };
    let rewritten = direct_openai_path_and_query(path_and_query);
    let uri = Uri::builder()
        .path_and_query(rewritten)
        .build()
        .map_err(|_| {
            pingora_core::Error::explain(
                pingora_core::ErrorType::InvalidHTTPHeader,
                "invalid rewritten OpenAI-compatible upstream URI",
            )
        })?;
    upstream_request.set_uri(uri);
    Ok(())
}

fn direct_openai_path_and_query(path_and_query: &str) -> String {
    let Some(rest) = path_and_query.strip_prefix("/providers/openai") else {
        return path_and_query.to_owned();
    };
    if rest.is_empty() {
        return "/".to_owned();
    }
    if rest.starts_with('/') || rest.starts_with('?') {
        rest.to_owned()
    } else {
        format!("/{rest}")
    }
}

fn bypass_gateway_governance_for_passthrough(route: Route, litellm_passthrough: bool) -> bool {
    litellm_passthrough && route == Route::LiteLlmPassthrough
}

fn sensitive_litellm_passthrough_authorized(
    exposure: Option<LiteLlmSensitiveRouteExposure>,
    entra_identity: Option<&EntraIdentityContext>,
) -> bool {
    match exposure {
        Some(LiteLlmSensitiveRouteExposure::Disabled) => false,
        Some(LiteLlmSensitiveRouteExposure::OperatorOnly) => entra_identity.is_some(),
        Some(LiteLlmSensitiveRouteExposure::ExplicitlyExposed) | None => true,
    }
}

fn rewrite_service_wildcard_uri(
    upstream_request: &mut RequestHeader,
    service_name: &str,
    route_pattern: Option<&str>,
) -> PingoraResult<()> {
    let Some(path_and_query) = upstream_request
        .uri
        .path_and_query()
        .map(|value| value.as_str())
    else {
        return Ok(());
    };
    let Some(rewritten) = route_pattern
        .and_then(|pattern| route_pattern_wildcard_suffix(path_and_query, pattern))
        .or_else(|| service_wildcard_suffix(path_and_query, service_name))
    else {
        return Ok(());
    };
    let uri = Uri::builder()
        .path_and_query(rewritten)
        .build()
        .map_err(|_| {
            pingora_core::Error::explain(
                pingora_core::ErrorType::InvalidHTTPHeader,
                "invalid rewritten service upstream URI",
            )
        })?;
    upstream_request.set_uri(uri);
    Ok(())
}

fn should_check_service_routes(path: &str) -> bool {
    !path.starts_with("/v1/") && !path.starts_with("/providers/openai/")
}

fn service_route_match_for_persisted_registration(
    method: &http::Method,
    path: &str,
    service_name: &str,
) -> RouteMatch {
    match Route::resolve_match(method, path) {
        Ok(matched) if matched.provider == Provider::InternalService => {
            RouteMatch::service(matched.route, service_name)
        }
        _ => RouteMatch::service(Route::ServiceWildcard, service_name),
    }
}

fn observe_response_body_chunk(ctx: &mut PingoraContext, body: &[u8]) {
    if ctx.is_streaming && !ctx.first_chunk_recorded {
        ctx.first_chunk_recorded = true;
        let latency_ms = u64::try_from(ctx.started.elapsed().as_millis()).unwrap_or(u64::MAX);
        gateway_telemetry::record_first_token_latency_ms(latency_ms);
    }
    if ctx.response_body_prefix.len() < 65_536 {
        let remaining = 65_536 - ctx.response_body_prefix.len();
        ctx.response_body_prefix
            .extend_from_slice(&body[..body.len().min(remaining)]);
    }
}

fn apply_post_call_guardrails(
    body: &mut Option<Bytes>,
    end_of_stream: bool,
    ctx: &mut PingoraContext,
) -> GatewayResult<()> {
    if ctx
        .post_guardrail_plan
        .as_ref()
        .is_none_or(|plan| plan.entries.is_empty())
    {
        return Ok(());
    }
    let Some(rewriter) = ctx.response_rewriter.as_mut() else {
        return Ok(());
    };
    let plan = ctx.post_guardrail_plan.clone().unwrap_or_default();
    let context = ctx.guardrail_context.clone();
    let definitions = ctx.guardrail_definitions.clone();
    let mut events = Vec::new();
    rewriter.filter_chunk(body, end_of_stream, |raw_body| {
        if !end_of_stream {
            return Ok(raw_body.to_vec());
        }
        let response_json = match serde_json::from_slice::<serde_json::Value>(raw_body) {
            Ok(value) => value,
            Err(_) => return Ok(raw_body.to_vec()),
        };
        let Some(context) = context.clone() else {
            return Ok(raw_body.to_vec());
        };
        let executor = guardrail_executor_for_definitions(&definitions);
        let execution = executor.execute(
            &plan,
            GuardrailMode::PostCall,
            context,
            None,
            Some(response_json),
        )?;
        events.extend(execution_events_from_records(
            &execution.context,
            &execution.records,
            Utc::now(),
        ));
        serde_json::to_vec(&execution.response.unwrap_or(serde_json::Value::Null))
            .map_err(|_| GatewayError::InvalidGuardrailRequest)
    })?;
    ctx.guardrail_events.extend(events);
    Ok(())
}

fn apply_streaming_guardrails(
    body: &mut Option<Bytes>,
    end_of_stream: bool,
    ctx: &mut PingoraContext,
) {
    let Some(chunk) = body.take() else {
        if end_of_stream && !ctx.guardrail_stream_holdback.is_empty() {
            let flushed = std::mem::take(&mut ctx.guardrail_stream_holdback);
            let (redacted, _) = redact_pii_text(&flushed);
            *body = Some(Bytes::from(redacted));
        }
        return;
    };
    let Ok(text) = std::str::from_utf8(&chunk) else {
        ctx.guardrail_error = Some(GatewayError::GuardrailUnavailable);
        *body = Some(Bytes::new());
        return;
    };
    let mut combined = String::new();
    combined.push_str(&ctx.guardrail_stream_holdback);
    combined.push_str(text);
    let split_at = if end_of_stream {
        combined.len()
    } else if combined.len() <= 64 {
        0
    } else {
        combined
            .char_indices()
            .rev()
            .nth(64)
            .map(|(index, _)| index)
            .unwrap_or(0)
    };
    let holdback = combined.split_off(split_at);
    ctx.guardrail_stream_holdback = holdback;
    let (redacted, metadata) = redact_pii_text(&combined);
    if let (Some(plan), Some(context)) = (
        ctx.during_guardrail_plan.clone(),
        ctx.guardrail_context.clone(),
    ) {
        let executor = guardrail_executor_for_definitions(&ctx.guardrail_definitions);
        match executor.execute(
            &plan,
            GuardrailMode::DuringCall,
            context,
            None,
            Some(serde_json::Value::String(redacted)),
        ) {
            Ok(execution) => {
                ctx.guardrail_context = Some(execution.context.clone());
                ctx.guardrail_events.extend(execution_events_from_records(
                    &execution.context,
                    &execution.records,
                    Utc::now(),
                ));
                let output = execution
                    .response
                    .and_then(|value| value.as_str().map(ToOwned::to_owned))
                    .unwrap_or_default();
                *body = Some(Bytes::from(output));
            }
            Err(error) => {
                ctx.guardrail_error = Some(error);
                *body = Some(Bytes::new());
            }
        }
    } else {
        let _ = metadata;
        *body = Some(Bytes::from(redacted));
    }
}

fn guardrail_plan_names_match(left: &GuardrailPlan, right: &GuardrailPlan) -> bool {
    left.entries
        .iter()
        .map(|entry| entry.definition.name.as_str())
        .eq(right
            .entries
            .iter()
            .map(|entry| entry.definition.name.as_str()))
}

fn applied_guardrails_header(ctx: &PingoraContext) -> String {
    ctx.pre_guardrail_plan
        .iter()
        .chain(ctx.post_guardrail_plan.iter())
        .chain(ctx.during_guardrail_plan.iter())
        .flat_map(|plan| plan.entries.iter())
        .map(|entry| entry.definition.name.as_str())
        .fold(Vec::<&str>::new(), |mut names, name| {
            if !names.contains(&name) {
                names.push(name);
            }
            names
        })
        .join(",")
}

fn provider_for_usage(ctx: &PingoraContext) -> Provider {
    if ctx.fallback_count > 0 {
        return Provider::LiteLlm;
    }
    ctx.route_match
        .as_ref()
        .map(|matched| matched.provider)
        .unwrap_or(Provider::LiteLlm)
}

fn debug_bundle_for_ctx(ctx: &PingoraContext, status_code: u16) -> gateway_core::DebugBundle {
    let provider = provider_for_usage(ctx);
    let route = ctx.route;
    let service_name = ctx
        .route_match
        .as_ref()
        .and_then(|matched| matched.service_name.clone());
    let mut selection_trace = vec![format!("provider={}", provider.as_str())];
    if let Some(matched) = &ctx.route_match {
        selection_trace.push(format!("backend={:?}", matched.backend));
        selection_trace.push(format!("timeout_ms={}", matched.timeout_ms));
    }
    let fallback_history = if ctx.fallback_count > 0 {
        vec![gateway_core::FallbackAttempt {
            from_provider: Provider::OpenAiCompatible.as_str().to_owned(),
            to_provider: Provider::LiteLlm.as_str().to_owned(),
            reason: "retry_safe_upstream_failure".to_owned(),
            status_code: Some(status_code),
            latency_ms: Some(i64::try_from(ctx.started.elapsed().as_millis()).unwrap_or(i64::MAX)),
        }]
    } else {
        Vec::new()
    };
    gateway_core::DebugBundle {
        request_id: ctx.request_id.clone(),
        route,
        provider: Some(provider),
        service_name,
        trace_id: ctx.trace_id.clone(),
        policy_trace: ctx
            .policy
            .as_ref()
            .map(|policy| {
                vec![
                    format!("policy_version={}", policy.policy_version),
                    format!("deny={}", policy.deny),
                ]
            })
            .unwrap_or_else(|| vec!["policy_not_loaded".to_owned()]),
        guardrail_trace: ctx
            .guardrail_events
            .iter()
            .map(|event| format!("{}:{}", event.mode.as_str(), event.guardrail_name))
            .collect(),
        selection_trace,
        fallback_history,
        upstream_latency_ms: Some(
            i64::try_from(ctx.started.elapsed().as_millis()).unwrap_or(i64::MAX),
        ),
        request_hash: hash_prefix(&ctx.body_prefix),
        response_hash: hash_prefix(&ctx.response_body_prefix),
        redaction_version: 1,
        created_at: Utc::now(),
    }
}

fn hash_prefix(bytes: &[u8]) -> Option<String> {
    if bytes.is_empty() {
        return None;
    }
    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);
    Some(format!(
        "siphash:{:016x}:len={}",
        hasher.finish(),
        bytes.len()
    ))
}

fn trace_id_from_traceparent(value: &str) -> Option<String> {
    let mut parts = value.split('-');
    let _version = parts.next()?;
    let trace_id = parts.next()?;
    if trace_id.len() == 32
        && trace_id
            .chars()
            .all(|character| character.is_ascii_hexdigit())
    {
        Some(trace_id.to_ascii_lowercase())
    } else {
        None
    }
}

fn service_upstream_from_registration(
    registration: &gateway_core::ServiceRegistration,
) -> GatewayResult<PingoraUpstreamConfig> {
    registration.ensure_routable()?;
    PingoraUpstreamConfig::from_base_url(
        registration
            .upstream_base_url
            .as_deref()
            .unwrap_or_default(),
        registration.credential_secret.clone().unwrap_or_default(),
    )
}

fn is_retry_safe_proxy_error(error: &PingoraError) -> bool {
    if error.esource() != &ErrorSource::Upstream {
        return false;
    }
    matches!(
        error.etype(),
        ErrorType::ReadTimedout | ErrorType::WriteTimedout
    )
}

#[cfg(test)]
fn new_pingora_context_for_tests() -> PingoraContext {
    PingoraContext {
        started: Instant::now(),
        request_id: uuid::Uuid::new_v4().to_string(),
        route: None,
        route_match: None,
        key: None,
        entra_identity: None,
        relayna_key_header: ENTRA_DEFAULT_RELAYNA_KEY_HEADER.to_owned(),
        body_prefix: Vec::new(),
        body_bytes_seen: 0,
        response_body_prefix: Vec::new(),
        response_bytes_seen: 0,
        policy: None,
        request_rewriter: None,
        response_rewriter: None,
        is_streaming: false,
        first_chunk_recorded: false,
        budget_reserved: false,
        task_id: None,
        run_id: None,
        traceparent: None,
        trace_id: None,
        fallback_count: 0,
        terminal_usage_recorded: false,
        service_upstream: None,
        service_route_pattern: None,
        litellm_upstream: None,
        litellm_passthrough: false,
        guardrail_definitions: Vec::new(),
        guardrail_policy: GuardrailPolicy::default(),
        pre_guardrail_plan: None,
        post_guardrail_plan: None,
        during_guardrail_plan: None,
        guardrail_context: None,
        guardrail_events: Vec::new(),
        guardrail_error: None,
        rewritten_request_len: None,
        guardrail_stream_holdback: String::new(),
    }
}

#[cfg(test)]
fn default_auth_runtime_for_tests() -> SharedGatewayAuthRuntime {
    SharedGatewayAuthRuntime::new(GatewayAuthRuntimeConfig::default()).expect("auth runtime")
}

async fn respond_error(
    session: &mut Session,
    error: GatewayError,
    request_id: &str,
) -> PingoraResult<()> {
    let body = serde_json::to_vec(&error.body(request_id)).unwrap_or_else(|_| b"{}".to_vec());
    session
        .respond_error_with_body(error.status_code().as_u16(), Bytes::from(body))
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{DateTime, Utc};
    use gateway_core::{
        AuthenticatedKey, BudgetDecision, BudgetState, EntraIdentitySource, GatewayResult,
        LiteLlmPassthroughSettings, OpenAiRouteMode, RateLimitDecision, UsageEvent,
    };
    use std::sync::Mutex;
    use uuid::Uuid;

    #[test]
    fn parses_https_litellm_base_url_for_pingora_peer() {
        let config = PingoraLiteLlmConfig::from_base_url("https://litellm.internal", "service-key")
            .expect("config");

        assert_eq!(config.litellm.host, "litellm.internal");
        assert_eq!(config.litellm.port, 443);
        assert!(config.litellm.tls);
        assert_eq!(config.litellm.sni, "litellm.internal");
        assert_eq!(config.litellm.service_key, "service-key");
    }

    #[test]
    fn parses_http_litellm_base_url_for_pingora_peer() {
        let config = PingoraLiteLlmConfig::from_base_url("http://127.0.0.1:4000", "service-key")
            .expect("config");

        assert_eq!(config.litellm.host, "127.0.0.1");
        assert_eq!(config.litellm.port, 4000);
        assert!(!config.litellm.tls);
        assert_eq!(config.litellm.sni, "127.0.0.1");
    }

    #[test]
    fn formats_upstream_host_header_for_default_and_custom_ports() {
        let https_default =
            PingoraUpstreamConfig::from_base_url("https://litellm.internal", "service-key")
                .expect("https config");
        assert_eq!(https_default.host_header_value(), "litellm.internal");

        let http_default =
            PingoraUpstreamConfig::from_base_url("http://example.internal", "service-key")
                .expect("http config");
        assert_eq!(http_default.host_header_value(), "example.internal");

        let service = PingoraUpstreamConfig::from_base_url(
            "http://document-upload-api-service.default.svc.cluster.local:8886",
            "service-key",
        )
        .expect("service config");
        assert_eq!(
            service.host_header_value(),
            "document-upload-api-service.default.svc.cluster.local:8886"
        );

        let ipv6 = PingoraUpstreamConfig::from_base_url("http://[::1]:8886", "service-key")
            .expect("ipv6 config");
        assert_eq!(ipv6.host_header_value(), "[::1]:8886");
    }

    #[test]
    fn upstream_header_preparation_replaces_downstream_host() {
        let upstream = PingoraUpstreamConfig::from_base_url(
            "http://document-upload-api-service.default.svc.cluster.local:8886",
            "internal-service-key",
        )
        .expect("service config");
        let mut request = RequestHeader::build("GET", b"/services/document-ingestion/health", None)
            .expect("request");
        request
            .insert_header("host", "relayna-gateway-proxy.relayna.svc.cluster.local")
            .expect("client host");
        request
            .insert_header("authorization", "Bearer rk_live_client_key")
            .expect("client authorization");
        request
            .insert_header("x-relayna-key", "rk_live_client_key")
            .expect("client Relayna key");
        request
            .insert_header("x-aih-api-key", "rk_live_legacy_client_key")
            .expect("client Relayna key");
        request
            .insert_header("x-apigee-entra-identity", "identity")
            .expect("Apigee identity");
        request
            .insert_header("x-apigee-entra-signature", "signature")
            .expect("Apigee signature");
        request
            .insert_header("proxy-authorization", "Bearer proxy-client")
            .expect("client proxy authorization");
        request
            .insert_header("x-api-key", "client-api-key")
            .expect("client api key");
        request
            .insert_header("x-relayna-worker-token", "client-worker-token")
            .expect("client worker token");

        prepare_upstream_authority_and_credentials(&mut request, &upstream, Some("x-relayna-key"))
            .expect("prepared upstream headers");

        assert_eq!(
            request
                .headers
                .get("host")
                .and_then(|value| value.to_str().ok()),
            Some("document-upload-api-service.default.svc.cluster.local:8886")
        );
        assert_eq!(
            request
                .headers
                .get("authorization")
                .and_then(|value| value.to_str().ok()),
            Some("Bearer internal-service-key")
        );
        assert!(!request.headers.contains_key("x-relayna-key"));
        assert!(!request.headers.contains_key("proxy-authorization"));
        assert!(!request.headers.contains_key("x-aih-api-key"));
        assert!(!request.headers.contains_key("x-apigee-entra-identity"));
        assert!(!request.headers.contains_key("x-apigee-entra-signature"));
        assert!(!request.headers.contains_key("x-api-key"));
        assert!(!request.headers.contains_key("x-relayna-worker-token"));
    }

    #[test]
    fn upstream_header_preparation_can_use_custom_litellm_header() {
        let upstream =
            PingoraUpstreamConfig::from_base_url("https://litellm.internal", "vk-litellm")
                .expect("service config")
                .with_litellm_credential_header(
                    CredentialHeaderMode::CustomHeader,
                    Some("x-litellm-api-key".to_owned()),
                )
                .expect("custom header");
        let mut request = RequestHeader::build("POST", b"/v1/responses", None).expect("request");
        request
            .insert_header("authorization", "Bearer rk_live_client_key")
            .expect("client authorization");
        request
            .insert_header("x-api-key", "client-api-key")
            .expect("client api key");
        request
            .insert_header("x-litellm-api-key", "client-supplied-litellm-key")
            .expect("client litellm key");

        prepare_upstream_authority_and_credentials(&mut request, &upstream, Some("x-relayna-key"))
            .expect("prepared upstream headers");

        assert!(!request.headers.contains_key("authorization"));
        assert!(!request.headers.contains_key("x-api-key"));
        assert_eq!(
            request
                .headers
                .get("x-litellm-api-key")
                .and_then(|value| value.to_str().ok()),
            Some("vk-litellm")
        );
    }

    #[test]
    fn validates_traceparent_shape() {
        assert!(is_valid_traceparent(
            "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01"
        ));
        assert!(!is_valid_traceparent("not-a-traceparent"));
    }

    #[test]
    fn stores_optional_worker_token_in_proxy_config() {
        let config = PingoraLiteLlmConfig::from_base_url("http://127.0.0.1:4000", "service-key")
            .expect("config")
            .with_worker_token(Some("worker-token".to_owned()));

        assert_eq!(config.worker_token.as_deref(), Some("worker-token"));
    }

    #[test]
    fn relayna_key_header_is_available_for_apigee_only_mode() {
        let default_config =
            PingoraLiteLlmConfig::from_base_url("http://127.0.0.1:4000", "service-key")
                .expect("config")
                .with_apigee_trusted_header(Some(ApigeeTrustedHeaderConfig {
                    secret: "trusted-secret".to_owned(),
                    required_scope: None,
                    required_role: None,
                    allowed_groups: Vec::new(),
                }));
        assert_eq!(default_config.relayna_key_header(), "X-Relayna-Key");

        let custom_config =
            PingoraLiteLlmConfig::from_base_url("http://127.0.0.1:4000", "service-key")
                .expect("config")
                .with_relayna_key_header("X-Custom-Relayna-Key")
                .expect("key header")
                .with_apigee_trusted_header(Some(ApigeeTrustedHeaderConfig {
                    secret: "trusted-secret".to_owned(),
                    required_scope: None,
                    required_role: None,
                    allowed_groups: Vec::new(),
                }));
        assert_eq!(custom_config.relayna_key_header(), "X-Custom-Relayna-Key");
    }

    #[test]
    fn worker_token_comparison_accepts_only_exact_match() {
        assert!(constant_time_eq(b"worker-token", b"worker-token"));
        assert!(!constant_time_eq(b"worker-token", b"worker-tokem"));
        assert!(!constant_time_eq(b"worker-token", b"worker-token-extra"));
        assert!(!constant_time_eq(b"", b"worker-token"));
    }

    #[test]
    fn rewrites_direct_openai_prefix_and_preserves_query() {
        assert_eq!(
            direct_openai_path_and_query("/providers/openai/v1/chat/completions?stream=true"),
            "/v1/chat/completions?stream=true"
        );
        assert_eq!(direct_openai_path_and_query("/providers/openai"), "/");
        assert_eq!(
            direct_openai_path_and_query("/v1/chat/completions"),
            "/v1/chat/completions"
        );
    }

    #[test]
    fn rewrites_service_wildcard_prefix_and_preserves_query() {
        assert_eq!(
            service_wildcard_suffix("/services/custom-ai/run?trace=1", "custom-ai").as_deref(),
            Some("/run?trace=1")
        );
        assert_eq!(
            service_wildcard_suffix("/services/custom-ai", "custom-ai").as_deref(),
            Some("/")
        );
        assert_eq!(
            route_pattern_wildcard_suffix(
                "/services/translation/translations?trace=1",
                "/services/translation/*"
            )
            .as_deref(),
            Some("/translations?trace=1")
        );
        assert_eq!(
            route_pattern_wildcard_suffix("/translations?trace=1", "/translations").as_deref(),
            None
        );
    }

    #[test]
    fn persisted_service_match_preserves_canonical_route_policy_identity() {
        let matched = service_route_match_for_persisted_registration(
            &http::Method::POST,
            "/summary",
            "summary",
        );

        assert_eq!(matched.route, Route::Summary);
        assert_eq!(matched.provider, Provider::InternalService);
        assert_eq!(matched.service_name.as_deref(), Some("summary"));

        let custom = service_route_match_for_persisted_registration(
            &http::Method::POST,
            "/internal/custom",
            "custom",
        );
        assert_eq!(custom.route, Route::ServiceWildcard);
        assert_eq!(custom.service_name.as_deref(), Some("custom"));
    }

    #[test]
    fn guardrail_header_deduplicates_pre_and_post_plans() {
        let mut ctx = new_pingora_context_for_tests();
        let definition = gateway_core::pii_redact_definition();
        ctx.pre_guardrail_plan = Some(GuardrailPlan {
            entries: vec![gateway_core::GuardrailPlanEntry {
                definition: definition.clone(),
            }],
        });
        ctx.post_guardrail_plan = Some(GuardrailPlan {
            entries: vec![gateway_core::GuardrailPlanEntry { definition }],
        });

        assert_eq!(applied_guardrails_header(&ctx), "pii-redact");
    }

    #[test]
    fn streaming_plan_compatibility_compares_guardrail_names() {
        let first = GuardrailPlan {
            entries: vec![gateway_core::GuardrailPlanEntry {
                definition: gateway_core::GuardrailDefinition::new(
                    "post-only",
                    "Post only",
                    vec![GuardrailMode::PostCall],
                    gateway_core::GuardrailFailurePolicy::FailClosed,
                ),
            }],
        };
        let second = GuardrailPlan {
            entries: vec![gateway_core::GuardrailPlanEntry {
                definition: gateway_core::GuardrailDefinition::new(
                    "during-only",
                    "During only",
                    vec![GuardrailMode::DuringCall],
                    gateway_core::GuardrailFailurePolicy::FailClosed,
                ),
            }],
        };

        assert!(!guardrail_plan_names_match(&first, &second));
    }

    #[test]
    fn post_call_guardrail_errors_are_propagated() {
        let mut ctx = new_pingora_context_for_tests();
        ctx.response_rewriter = Some(BoundedBodyRewriter::new(1024));
        ctx.guardrail_context = Some(GuardrailContext::default());
        ctx.post_guardrail_plan = Some(GuardrailPlan {
            entries: vec![gateway_core::GuardrailPlanEntry {
                definition: gateway_core::GuardrailDefinition::new(
                    "missing-handler",
                    "Missing handler",
                    vec![GuardrailMode::PostCall],
                    gateway_core::GuardrailFailurePolicy::FailClosed,
                ),
            }],
        });
        let mut body = Some(Bytes::from_static(br#"{"choices":[]}"#));

        let error = apply_post_call_guardrails(&mut body, true, &mut ctx).unwrap_err();

        assert_eq!(error, GatewayError::GuardrailUnavailable);
        assert!(ctx.guardrail_events.is_empty());
    }

    #[test]
    fn streaming_guardrails_redact_pii_across_chunks() {
        let mut ctx = new_pingora_context_for_tests();
        let definition = gateway_core::pii_redact_definition();
        ctx.guardrail_definitions = vec![definition.clone()];
        ctx.during_guardrail_plan = Some(GuardrailPlan {
            entries: vec![gateway_core::GuardrailPlanEntry { definition }],
        });
        ctx.guardrail_context = Some(GuardrailContext::default());

        let mut first = Some(Bytes::from("data: {\"delta\":\"alice@"));
        apply_streaming_guardrails(&mut first, false, &mut ctx);
        let mut second = Some(Bytes::from("example.com\"}\n\n"));
        apply_streaming_guardrails(&mut second, true, &mut ctx);

        let output = format!(
            "{}{}",
            String::from_utf8(first.unwrap().to_vec()).expect("utf8"),
            String::from_utf8(second.unwrap().to_vec()).expect("utf8")
        );
        assert!(output.contains("[EMAIL_1]"));
        assert!(!output.contains("alice@example.com"));
        assert!(!ctx.guardrail_events.is_empty());
    }

    #[test]
    fn delayed_stream_chunk_records_first_chunk_once_and_caps_prefix() {
        let mut ctx = new_pingora_context_for_tests();
        ctx.started = Instant::now() - Duration::from_millis(25);
        ctx.is_streaming = true;

        observe_response_body_chunk(&mut ctx, b"data: first\n\n");
        observe_response_body_chunk(&mut ctx, b"data: second\n\n");

        assert!(ctx.first_chunk_recorded);
        assert_eq!(ctx.response_body_prefix, b"data: first\n\ndata: second\n\n");

        observe_response_body_chunk(&mut ctx, &vec![b'x'; 70_000]);
        assert_eq!(ctx.response_body_prefix.len(), 65_536);
    }

    #[test]
    fn direct_provider_fallback_switches_once_to_litellm() {
        let store = Arc::new(MemoryUsageStore::default());
        let control_state = Arc::new(MemoryControlState::default());
        let proxy = RelaynaPingoraProxy {
            store,
            control_state,
            config: PingoraLiteLlmConfig::from_base_url("http://litellm.internal", "litellm-key")
                .expect("litellm config")
                .with_direct_openai(Some(
                    PingoraUpstreamConfig::from_base_url("https://api.openai.test", "openai-key")
                        .expect("direct config"),
                )),
            auth_runtime: default_auth_runtime_for_tests(),
        };
        let mut ctx = new_pingora_context_for_tests();
        ctx.route_match = Some(
            Route::resolve_match(&http::Method::POST, "/providers/openai/v1/chat/completions")
                .expect("route"),
        );

        assert_eq!(provider_for_usage(&ctx), Provider::OpenAiCompatible);
        assert_eq!(
            proxy
                .upstream_for(&ctx)
                .expect("direct upstream")
                .service_key,
            "openai-key"
        );

        assert!(proxy.activate_provider_fallback(&mut ctx));
        assert_eq!(ctx.fallback_count, 1);
        assert_eq!(provider_for_usage(&ctx), Provider::LiteLlm);
        assert_eq!(
            proxy
                .upstream_for(&ctx)
                .expect("fallback upstream")
                .service_key,
            "litellm-key"
        );
        assert!(!proxy.activate_provider_fallback(&mut ctx));
    }

    #[tokio::test]
    async fn openai_route_setting_blocks_disabled_litellm_routes_only() {
        let store = Arc::new(MemoryUsageStore::default());
        *store.openai_routes_enabled.lock().expect("routes lock") = false;
        let proxy = RelaynaPingoraProxy {
            store,
            control_state: Arc::new(MemoryControlState::default()),
            config: PingoraLiteLlmConfig::from_base_url("http://127.0.0.1:4000", "service-key")
                .expect("config"),
            auth_runtime: default_auth_runtime_for_tests(),
        };

        assert_eq!(
            proxy
                .ensure_openai_route_enabled(Route::ChatCompletions)
                .await
                .unwrap_err(),
            GatewayError::DisabledRoute
        );
        assert_eq!(
            proxy
                .ensure_openai_route_enabled(Route::LiteLlmEmbeddings)
                .await
                .unwrap_err(),
            GatewayError::DisabledRoute
        );
        proxy
            .ensure_openai_route_enabled(Route::ServiceWildcard)
            .await
            .expect("service wildcard is not controlled by OpenAI route settings");
    }

    #[test]
    fn litellm_passthrough_settings_allow_v1_and_block_sensitive_paths() {
        let mut settings = LiteLlmPassthroughSettings::default_with_updated_at(Utc::now());
        settings.enabled = true;

        assert!(settings.allows(&http::Method::GET, "/v1/models"));
        assert!(settings.allows(&http::Method::POST, "/v1/chat/completions"));
        assert!(!settings.allows(&http::Method::DELETE, "/v1/models/model-a"));
        assert!(!settings.allows(&http::Method::GET, "/ui"));
        assert!(!settings.allows(&http::Method::GET, "/key/list"));
    }

    #[test]
    fn direct_litellm_route_mode_still_uses_gateway_governance() {
        assert!(bypass_gateway_governance_for_passthrough(
            Route::LiteLlmPassthrough,
            true
        ));
        assert!(!bypass_gateway_governance_for_passthrough(
            Route::ChatCompletions,
            true
        ));
        assert!(!bypass_gateway_governance_for_passthrough(
            Route::Responses,
            true
        ));
        assert!(!bypass_gateway_governance_for_passthrough(
            Route::LiteLlmEmbeddings,
            true
        ));
    }

    #[test]
    fn operator_only_litellm_paths_require_entra_identity() {
        assert!(!sensitive_litellm_passthrough_authorized(
            Some(LiteLlmSensitiveRouteExposure::OperatorOnly),
            None
        ));
        assert!(sensitive_litellm_passthrough_authorized(
            Some(LiteLlmSensitiveRouteExposure::ExplicitlyExposed),
            None
        ));

        let identity = EntraIdentityContext {
            tenant_id: "tenant".to_owned(),
            subject: Some("operator".to_owned()),
            object_id: Some("object".to_owned()),
            app_id: None,
            authorized_party: None,
            scopes: vec!["gateway.invoke".to_owned()],
            roles: Vec::new(),
            groups: Vec::new(),
            token_version: "2.0".to_owned(),
            source: EntraIdentitySource::Jwt,
        };
        assert!(sensitive_litellm_passthrough_authorized(
            Some(LiteLlmSensitiveRouteExposure::OperatorOnly),
            Some(&identity)
        ));
    }

    #[tokio::test]
    async fn direct_litellm_passthrough_terminal_usage_is_status_only() {
        let store = Arc::new(MemoryUsageStore::default());
        let control_state = Arc::new(MemoryControlState::default());
        let proxy = RelaynaPingoraProxy {
            store: store.clone(),
            control_state,
            config: PingoraLiteLlmConfig::from_base_url("http://127.0.0.1:4000", "service-key")
                .expect("config"),
            auth_runtime: default_auth_runtime_for_tests(),
        };
        let key = AuthenticatedKey {
            key_id: Uuid::new_v4(),
            project_id: Some(Uuid::new_v4()),
            key_prefix: "rk_live_test_key".to_owned(),
        };
        let mut ctx = new_pingora_context_for_tests();
        ctx.request_id = "req_direct_passthrough".to_owned();
        ctx.route = Some(Route::ChatCompletions);
        ctx.route_match =
            Some(Route::resolve_match(&http::Method::POST, "/v1/chat/completions").expect("route"));
        ctx.litellm_passthrough = true;
        ctx.key = Some(key.clone());

        proxy
            .record_terminal_usage(&mut ctx, &key, Route::ChatCompletions, 502, Utc::now())
            .await;

        let events = store.events.lock().expect("events lock");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].request_id, "req_direct_passthrough");
        assert_eq!(events[0].estimated_cost_usd, None);
        assert_eq!(events[0].input_tokens, None);
        assert_eq!(events[0].output_tokens, None);
    }

    struct MemoryUsageStore {
        events: Mutex<Vec<UsageEvent>>,
        debug_bundles: Mutex<Vec<gateway_core::DebugBundle>>,
        guardrail_events: Mutex<Vec<GuardrailExecutionEvent>>,
        openai_routes_enabled: Mutex<bool>,
        openai_route_mode: Mutex<OpenAiRouteMode>,
        litellm_passthrough_settings: Mutex<LiteLlmPassthroughSettings>,
    }

    impl Default for MemoryUsageStore {
        fn default() -> Self {
            Self {
                events: Mutex::new(Vec::new()),
                debug_bundles: Mutex::new(Vec::new()),
                guardrail_events: Mutex::new(Vec::new()),
                openai_routes_enabled: Mutex::new(true),
                openai_route_mode: Mutex::new(OpenAiRouteMode::ManagedByGateway),
                litellm_passthrough_settings: Mutex::new(
                    LiteLlmPassthroughSettings::default_with_updated_at(Utc::now()),
                ),
            }
        }
    }

    #[async_trait]
    impl UsageRecorder for MemoryUsageStore {
        async fn insert_usage_event(&self, event: &UsageEvent) -> GatewayResult<()> {
            self.events.lock().expect("events lock").push(event.clone());
            Ok(())
        }
    }

    #[async_trait]
    impl ProviderIntelligenceStore for MemoryUsageStore {
        async fn list_provider_health_states(
            &self,
        ) -> GatewayResult<Vec<gateway_core::ProviderHealthState>> {
            Ok(Vec::new())
        }

        async fn provider_health_check_targets(
            &self,
        ) -> GatewayResult<Vec<gateway_core::ProviderHealthCheckTarget>> {
            Ok(Vec::new())
        }

        async fn upsert_provider_health_state(
            &self,
            state: gateway_core::ProviderHealthState,
        ) -> GatewayResult<gateway_core::ProviderHealthState> {
            Ok(state)
        }

        async fn get_debug_bundle(
            &self,
            request_id: &str,
        ) -> GatewayResult<Option<gateway_core::DebugBundle>> {
            Ok(self
                .debug_bundles
                .lock()
                .expect("debug bundles lock")
                .iter()
                .find(|bundle| bundle.request_id == request_id)
                .cloned())
        }

        async fn insert_debug_bundle(
            &self,
            bundle: gateway_core::DebugBundle,
        ) -> GatewayResult<()> {
            self.debug_bundles
                .lock()
                .expect("debug bundles lock")
                .push(bundle);
            Ok(())
        }

        async fn list_service_registry_snapshots(
            &self,
        ) -> GatewayResult<Vec<gateway_core::ServiceRegistrySnapshot>> {
            Ok(Vec::new())
        }

        async fn insert_service_registry_snapshot(
            &self,
            snapshot: gateway_core::ServiceRegistrySnapshot,
        ) -> GatewayResult<gateway_core::ServiceRegistrySnapshot> {
            Ok(snapshot)
        }

        async fn service_registry_snapshot(
            &self,
            _version: i64,
        ) -> GatewayResult<Option<gateway_core::ServiceRegistrySnapshot>> {
            Ok(None)
        }

        async fn activate_service_registry_import(
            &self,
            _source: String,
            _diff: gateway_core::ServiceImportDiff,
            _services: Vec<gateway_core::StudioServiceImportRequest>,
            _rolled_back_from_version: Option<i64>,
        ) -> GatewayResult<(
            gateway_core::ServiceRegistrySnapshot,
            Vec<gateway_core::ServiceResponse>,
        )> {
            Err(GatewayError::StoreUnavailable)
        }
    }

    #[async_trait]
    impl OpenAiRouteSettingsLookup for MemoryUsageStore {
        async fn openai_route_enabled(&self, route: Route) -> GatewayResult<bool> {
            if gateway_core::openai_route_id(route).is_some() {
                Ok(*self.openai_routes_enabled.lock().expect("routes lock"))
            } else {
                Ok(true)
            }
        }

        async fn openai_route_mode(&self, route: Route) -> GatewayResult<OpenAiRouteMode> {
            if gateway_core::openai_route_id(route).is_some() {
                Ok(*self.openai_route_mode.lock().expect("route mode lock"))
            } else {
                Ok(OpenAiRouteMode::ManagedByGateway)
            }
        }

        async fn litellm_passthrough_settings(&self) -> GatewayResult<LiteLlmPassthroughSettings> {
            Ok(self
                .litellm_passthrough_settings
                .lock()
                .expect("passthrough settings lock")
                .clone())
        }
    }

    #[async_trait]
    impl GuardrailStore for MemoryUsageStore {
        async fn list_guardrail_definitions(&self) -> GatewayResult<Vec<GuardrailDefinition>> {
            Ok(vec![gateway_core::pii_redact_definition()])
        }

        async fn guardrail_policy_for_key(&self, _key_id: Uuid) -> GatewayResult<GuardrailPolicy> {
            Ok(GuardrailPolicy::default())
        }

        async fn upsert_guardrail_policy_for_key(
            &self,
            _key_id: Uuid,
            _policy: &GuardrailPolicy,
        ) -> GatewayResult<()> {
            Ok(())
        }

        async fn insert_guardrail_execution_event(
            &self,
            event: &GuardrailExecutionEvent,
        ) -> GatewayResult<()> {
            self.guardrail_events
                .lock()
                .expect("guardrail events lock")
                .push(event.clone());
            Ok(())
        }
    }

    #[derive(Default)]
    struct MemoryControlState {
        released: Mutex<Vec<(Uuid, String)>>,
    }

    #[async_trait]
    impl BudgetStore for MemoryControlState {
        async fn check_budget(
            &self,
            _key_id: Uuid,
            _daily_budget_usd: Option<f64>,
            _monthly_budget_usd: Option<f64>,
            _now: DateTime<Utc>,
        ) -> GatewayResult<BudgetDecision> {
            Ok(BudgetDecision::Allowed(BudgetState {
                daily_spend_usd: 0.0,
                monthly_spend_usd: 0.0,
            }))
        }

        async fn add_budget_spend(
            &self,
            _key_id: Uuid,
            _estimated_cost_usd: f64,
            _now: DateTime<Utc>,
        ) -> GatewayResult<()> {
            Ok(())
        }

        async fn reserve_budget(
            &self,
            _key_id: Uuid,
            _request_id: &str,
            _estimated_cost_usd: f64,
            _now: DateTime<Utc>,
        ) -> GatewayResult<()> {
            Ok(())
        }

        async fn reconcile_budget_reservation(
            &self,
            _key_id: Uuid,
            _request_id: &str,
            _actual_cost_usd: f64,
            _now: DateTime<Utc>,
        ) -> GatewayResult<()> {
            Ok(())
        }

        async fn release_budget_reservation(
            &self,
            key_id: Uuid,
            request_id: &str,
        ) -> GatewayResult<()> {
            self.released
                .lock()
                .expect("released lock")
                .push((key_id, request_id.to_owned()));
            Ok(())
        }
    }

    #[async_trait]
    impl RateLimitStore for MemoryControlState {
        async fn check_request_rate_limit(
            &self,
            _key_id: Uuid,
            _rpm_limit: Option<i32>,
            _now: DateTime<Utc>,
        ) -> GatewayResult<RateLimitDecision> {
            Ok(RateLimitDecision::Allowed { count: 1 })
        }

        async fn check_token_rate_limit(
            &self,
            _key_id: Uuid,
            _tpm_limit: Option<i32>,
            _estimated_tokens: i64,
            _now: DateTime<Utc>,
        ) -> GatewayResult<RateLimitDecision> {
            Ok(RateLimitDecision::Allowed { count: 1 })
        }
    }

    #[tokio::test]
    async fn disconnect_cleanup_records_failure_usage_and_releases_stream_reservation() {
        let store = Arc::new(MemoryUsageStore::default());
        let control_state = Arc::new(MemoryControlState::default());
        let proxy = RelaynaPingoraProxy {
            store: store.clone(),
            control_state: control_state.clone(),
            config: PingoraLiteLlmConfig::from_base_url("http://127.0.0.1:4000", "service-key")
                .expect("config"),
            auth_runtime: default_auth_runtime_for_tests(),
        };
        let key = AuthenticatedKey {
            key_id: Uuid::new_v4(),
            project_id: Some(Uuid::new_v4()),
            key_prefix: "rk_live_test_key".to_owned(),
        };
        let mut ctx = new_pingora_context_for_tests();
        ctx.request_id = "req_disconnect".to_owned();
        ctx.route = Some(Route::ChatCompletions);
        ctx.route_match =
            Some(Route::resolve_match(&http::Method::POST, "/v1/chat/completions").expect("route"));
        ctx.key = Some(key.clone());
        ctx.is_streaming = true;
        ctx.budget_reserved = true;

        proxy
            .record_terminal_usage(&mut ctx, &key, Route::ChatCompletions, 502, Utc::now())
            .await;

        let events = store.events.lock().expect("events lock");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].request_id, "req_disconnect");
        assert_eq!(events[0].status_code, 502);
        drop(events);
        assert!(ctx.terminal_usage_recorded);
        assert_eq!(
            control_state
                .released
                .lock()
                .expect("released lock")
                .as_slice(),
            &[(key.key_id, "req_disconnect".to_owned())]
        );
    }

    #[tokio::test]
    async fn fallback_usage_records_final_provider_and_count() {
        let store = Arc::new(MemoryUsageStore::default());
        let control_state = Arc::new(MemoryControlState::default());
        let proxy = RelaynaPingoraProxy {
            store: store.clone(),
            control_state,
            config: PingoraLiteLlmConfig::from_base_url("http://127.0.0.1:4000", "service-key")
                .expect("config"),
            auth_runtime: default_auth_runtime_for_tests(),
        };
        let key = AuthenticatedKey {
            key_id: Uuid::new_v4(),
            project_id: Some(Uuid::new_v4()),
            key_prefix: "rk_live_test_key".to_owned(),
        };
        let mut ctx = new_pingora_context_for_tests();
        ctx.request_id = "req_fallback".to_owned();
        ctx.route = Some(Route::DirectOpenAi);
        ctx.route_match = Some(
            Route::resolve_match(&http::Method::POST, "/providers/openai/v1/chat/completions")
                .expect("route"),
        );
        ctx.key = Some(key.clone());
        ctx.fallback_count = 1;

        proxy
            .record_terminal_usage(&mut ctx, &key, Route::DirectOpenAi, 502, Utc::now())
            .await;

        let events = store.events.lock().expect("events lock");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].request_id, "req_fallback");
        assert_eq!(events[0].provider, Provider::LiteLlm);
        assert_eq!(events[0].fallback_count, 1);
    }

    #[test]
    fn debug_bundle_hashes_prefixes_without_storing_prompt_text() {
        let mut ctx = new_pingora_context_for_tests();
        ctx.request_id = "req_debug".to_owned();
        ctx.route = Some(Route::ChatCompletions);
        ctx.route_match =
            Some(Route::resolve_match(&http::Method::POST, "/v1/chat/completions").expect("route"));
        ctx.body_prefix = br#"{"messages":[{"content":"secret prompt"}]}"#.to_vec();
        ctx.response_body_prefix =
            br#"{"choices":[{"message":{"content":"secret answer"}}]}"#.to_vec();

        let bundle = debug_bundle_for_ctx(&ctx, 200);

        assert_eq!(bundle.request_id, "req_debug");
        assert!(bundle
            .request_hash
            .as_ref()
            .expect("request hash")
            .starts_with("siphash:"));
        assert!(bundle
            .response_hash
            .as_ref()
            .expect("response hash")
            .starts_with("siphash:"));
        let encoded = serde_json::to_string(&bundle).expect("json");
        assert!(!encoded.contains("secret prompt"));
        assert!(!encoded.contains("secret answer"));
    }
}
