use crate::body_rewrite::{
    prepare_rewritten_request_headers, prepare_rewritten_response_headers, BoundedBodyRewriter,
};
use crate::{
    BodyAdmissionController, BodyAdmissionLease, DEFAULT_MAX_BUFFERED_REQUESTS,
    DEFAULT_MAX_INFLIGHT_BUFFER_BYTES,
};
use async_trait::async_trait;
use bytes::Bytes;
use chrono::Utc;
use gateway_core::{
    analyze_generation_request,
    auth::{Authenticator, VirtualKeyLookup},
    estimate_generation_tokens, evaluate_policy, evaluate_policy_limits,
    execution_events_from_records, extract_client_guardrails_value, extract_estimated_cost_usd,
    extract_generation_features, extract_model, extract_usage_tokens,
    guardrail_executor_for_definitions, is_retry_safe_status, matching_openapi_endpoint,
    matching_service_pricing_rule, redact_pii_text, resolve_endpoint_pricing_rule,
    resolve_guardrail_plan, resolve_service_cost_from_value, route_pattern_wildcard_suffix,
    service_preflight_estimated_cost, service_wildcard_suffix, strip_client_guardrails,
    validate_relayna_key_header_name, verify_apigee_trusted_identity, ApigeeTrustedHeaderConfig,
    AuthenticatedKey, BudgetDecision, BudgetStore, CredentialHeaderMode,
    CredentialHeaderValueFormat, EntraAuthConfig, EntraIdentityContext, GatewayAuthRuntimeConfig,
    GatewayAuthRuntimeSnapshot, GatewayError, GatewayResult, GuardrailContext, GuardrailDefinition,
    GuardrailExecutionEvent, GuardrailMode, GuardrailPlan, GuardrailPlanRequest, GuardrailPolicy,
    GuardrailPolicySet, GuardrailStore, KeyPolicy, LiteLlmSensitiveRouteExposure, OpenAiRouteMode,
    OpenAiRouteSettingsLookup, PolicyLookup, Provider, ProviderConfigLookup,
    ProviderIntelligenceStore, RateLimitDecision, RateLimitStore, ResolvedServiceCost, Route,
    RouteMatch, ServiceCostMode, ServicePricingRule, ServiceRegistryLookup, ServiceRouteLookup,
    SharedGatewayAuthRuntime, UsageEvent, UsageRecorder, ENTRA_DEFAULT_RELAYNA_KEY_HEADER,
};
use http::{header, header::HeaderName, Uri};
use multra::{Constraints, Multipart, SizeLimit};
use pingora_core::{
    upstreams::peer::HttpPeer, Error as PingoraError, ErrorSource, ErrorType,
    Result as PingoraResult,
};
use pingora_http::{RequestHeader, ResponseHeader};
use pingora_proxy::{FailToProxy, ProxyHttp, Session};
use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    sync::Arc,
    time::{Duration, Instant},
};

const MAX_MULTIPART_PRICING_FIELDS: usize = 128;
const MAX_MULTIPART_PRICING_FIELD_NAME_BYTES: usize = 256;
const MAX_MULTIPART_PRICING_FIELD_VALUE_BYTES: usize = 16 * 1024;
const MAX_MULTIPART_PRICING_METADATA_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone)]
pub struct PingoraLiteLlmConfig {
    pub litellm: PingoraUpstreamConfig,
    pub direct_openai: Option<PingoraUpstreamConfig>,
    pub worker_token: Option<String>,
    pub entra_auth: Option<EntraAuthConfig>,
    pub apigee_trusted_header: Option<ApigeeTrustedHeaderConfig>,
    pub relayna_key_header: String,
    body_admission: BodyAdmissionController,
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
    pub credential_header_value_format: CredentialHeaderValueFormat,
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
            body_admission: BodyAdmissionController::new(
                DEFAULT_MAX_BUFFERED_REQUESTS,
                DEFAULT_MAX_INFLIGHT_BUFFER_BYTES,
            )?,
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

    pub fn with_body_admission_limits(
        mut self,
        max_buffered_requests: usize,
        max_inflight_buffer_bytes: usize,
    ) -> GatewayResult<Self> {
        self.body_admission =
            BodyAdmissionController::new(max_buffered_requests, max_inflight_buffer_bytes)?;
        Ok(self)
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
            credential_header_value_format: CredentialHeaderValueFormat::Raw,
        })
    }

    fn with_litellm_credential_header(
        mut self,
        mode: CredentialHeaderMode,
        header_name: Option<String>,
        value_format: CredentialHeaderValueFormat,
    ) -> gateway_core::GatewayResult<Self> {
        if mode == CredentialHeaderMode::CustomHeader && header_name.as_deref().is_none() {
            return Err(GatewayError::InvalidConfiguration);
        }
        self.credential_header_mode = mode;
        self.credential_header_name = header_name.map(|value| value.trim().to_owned());
        self.credential_header_value_format = value_format;
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
    request_content_type: Option<String>,
    body_prefix: Vec<u8>,
    body_bytes_seen: usize,
    response_body_prefix: Vec<u8>,
    response_bytes_seen: usize,
    policy: Option<KeyPolicy>,
    request_rewriter: Option<BoundedBodyRewriter>,
    response_rewriter: Option<BoundedBodyRewriter>,
    request_body_lease: Option<BodyAdmissionLease>,
    response_body_lease: Option<BodyAdmissionLease>,
    is_streaming: bool,
    first_chunk_recorded: bool,
    budget_reserved: bool,
    task_id: Option<String>,
    run_id: Option<String>,
    traceparent: Option<String>,
    trace_id: Option<String>,
    public_origin: Option<String>,
    fallback_count: i32,
    terminal_usage_recorded: bool,
    terminal_status_code: Option<u16>,
    upstream_timeout: bool,
    service_upstream: Option<PingoraUpstreamConfig>,
    service_route_pattern: Option<String>,
    http_method: Option<String>,
    endpoint_path: Option<String>,
    endpoint_template: Option<String>,
    service_cost_mode: Option<ServiceCostMode>,
    service_estimated_cost_usd: Option<f64>,
    service_pricing_rules: Vec<ServicePricingRule>,
    resolved_endpoint_cost: Option<ResolvedServiceCost>,
    resolved_service_cost: Option<ResolvedServiceCost>,
    litellm_upstream: Option<PingoraUpstreamConfig>,
    litellm_passthrough: bool,
    trusted_ingress_passthrough: bool,
    direct_litellm_passthrough: bool,
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
            request_content_type: None,
            body_prefix: Vec::new(),
            body_bytes_seen: 0,
            response_body_prefix: Vec::new(),
            response_bytes_seen: 0,
            policy: None,
            request_rewriter: None,
            response_rewriter: None,
            request_body_lease: None,
            response_body_lease: None,
            is_streaming: false,
            first_chunk_recorded: false,
            budget_reserved: false,
            task_id: None,
            run_id: None,
            traceparent: None,
            trace_id: None,
            public_origin: None,
            fallback_count: 0,
            terminal_usage_recorded: false,
            terminal_status_code: None,
            upstream_timeout: false,
            service_upstream: None,
            service_route_pattern: None,
            http_method: None,
            endpoint_path: None,
            endpoint_template: None,
            service_cost_mode: None,
            service_estimated_cost_usd: None,
            service_pricing_rules: Vec::new(),
            resolved_endpoint_cost: None,
            resolved_service_cost: None,
            litellm_upstream: None,
            litellm_passthrough: false,
            trusted_ingress_passthrough: false,
            direct_litellm_passthrough: false,
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
        ctx.request_content_type =
            header_value(req, header::CONTENT_TYPE.as_str()).map(ToOwned::to_owned);
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
            apply_service_registration_runtime_limits(
                &mut matched,
                registration.timeout_ms,
                registration.max_body_bytes,
            )?;
            configure_service_pricing_context(ctx, &registration, &req.method, req.uri.path());
            matched.estimated_cost_usd = registration.estimated_cost_usd;
            ctx.service_route_pattern = Some(registration.route_pattern);
            ctx.service_upstream = Some(upstream);
            matched
        } else {
            match Route::resolve_match(&req.method, req.uri.path()) {
                Ok(matched) => matched,
                Err(error) => match self.store.litellm_passthrough_settings().await {
                    Ok(settings)
                        if settings.allows(&req.method, req.uri.path())
                            || settings.trusted_ingress_passthrough_path_allowed(
                                &req.method,
                                req.uri.path(),
                            ) =>
                    {
                        ctx.litellm_passthrough = true;
                        RouteMatch {
                            route: Route::LiteLlmPassthrough,
                            backend: gateway_core::BackendType::LiteLlm,
                            provider: Provider::LiteLlm,
                            service_name: None,
                            timeout_ms: 120_000,
                            max_body_bytes: 1_048_576,
                            max_response_body_bytes: 1_048_576,
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
        if gateway_core::is_litellm_canonical_route(matched.route) {
            match self.apply_litellm_route_limits(&mut matched).await {
                Ok(()) => {}
                Err(error) => {
                    respond_error(session, error, &ctx.request_id).await?;
                    return Ok(true);
                }
            }
        } else if matched.route == Route::LiteLlmPassthrough {
            match self.apply_litellm_passthrough_limits(&mut matched).await {
                Ok(()) => {}
                Err(error) => {
                    respond_error(session, error, &ctx.request_id).await?;
                    return Ok(true);
                }
            }
        }
        ctx.route = Some(matched.route);
        ctx.request_rewriter = Some(BoundedBodyRewriter::new(matched.max_body_bytes));
        ctx.response_rewriter = Some(BoundedBodyRewriter::new(matched.max_response_body_bytes));
        ctx.traceparent = header_value(req, "traceparent")
            .filter(|value| is_valid_traceparent(value))
            .map(ToOwned::to_owned);
        ctx.trace_id = ctx
            .traceparent
            .as_deref()
            .and_then(trace_id_from_traceparent);
        ctx.public_origin = request_public_origin(req);
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
        if ctx.litellm_passthrough && matched.route == Route::LiteLlmPassthrough {
            match self.store.litellm_passthrough_settings().await {
                Ok(settings)
                    if settings
                        .trusted_ingress_passthrough_path_allowed(&req.method, req.uri.path()) =>
                {
                    if let Err(error) = self.configure_litellm_upstream(ctx, None).await {
                        respond_error(session, error, &ctx.request_id).await?;
                        return Ok(true);
                    }
                    ctx.trusted_ingress_passthrough = true;
                    ctx.route_match = Some(matched);
                    return Ok(false);
                }
                Ok(_) => {}
                Err(error) => {
                    respond_error(session, error, &ctx.request_id).await?;
                    return Ok(true);
                }
            }
        }
        if gateway_core::is_litellm_canonical_route(matched.route) {
            match self.route_mode(matched.route).await {
                Ok(OpenAiRouteMode::DirectLiteLlmPassthrough)
                    if !authorization_has_relayna_key(authorization) =>
                {
                    if let Err(error) = self
                        .ensure_litellm_canonical_route_enabled(matched.route)
                        .await
                    {
                        respond_error(session, error, &ctx.request_id).await?;
                        return Ok(true);
                    }
                    let credential = match litellm_bearer_credential(authorization) {
                        Ok(credential) => credential,
                        Err(error) => {
                            gateway_telemetry::record_auth_failure(error.code());
                            respond_error(session, error, &ctx.request_id).await?;
                            return Ok(true);
                        }
                    };
                    if let Err(error) = self
                        .configure_litellm_upstream_with_credential(ctx, credential)
                        .await
                    {
                        respond_error(session, error, &ctx.request_id).await?;
                        return Ok(true);
                    }
                    ctx.litellm_passthrough = true;
                    ctx.direct_litellm_passthrough = true;
                    ctx.route_match = Some(matched);
                    return Ok(false);
                }
                Ok(OpenAiRouteMode::DirectLiteLlmPassthrough) => {}
                Ok(OpenAiRouteMode::ManagedByGateway) => {}
                Err(error) => {
                    respond_error(session, error, &ctx.request_id).await?;
                    return Ok(true);
                }
            }
        }
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
                    apply_service_registration_runtime_limits(
                        &mut matched,
                        registration.timeout_ms,
                        registration.max_body_bytes,
                    )?;
                    configure_service_pricing_context(
                        ctx,
                        &registration,
                        &req.method,
                        req.uri.path(),
                    );
                    matched.estimated_cost_usd = registration.estimated_cost_usd;
                    ctx.service_route_pattern = Some(registration.route_pattern);
                    ctx.service_upstream = Some(upstream);
                }
                if gateway_core::is_litellm_canonical_route(matched.route) {
                    match self.route_mode(matched.route).await {
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
                    if let Err(error) = self.configure_litellm_upstream(ctx, Some(&key)).await {
                        respond_error(session, error, &ctx.request_id).await?;
                        return Ok(true);
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
        if ctx.guardrail_error.is_some() {
            *body = Some(Bytes::new());
            return Ok(());
        }
        if let Some(chunk) = body.as_ref() {
            ctx.body_bytes_seen = ctx.body_bytes_seen.saturating_add(chunk.len());
            if ctx.body_prefix.len() < 65_536 {
                let remaining = 65_536 - ctx.body_prefix.len();
                ctx.body_prefix
                    .extend_from_slice(&chunk[..chunk.len().min(remaining)]);
            }
            if let Some(matched) = &ctx.route_match {
                if ctx.body_bytes_seen > matched.max_body_bytes {
                    let error = GatewayError::RequestBodyTooLarge;
                    ctx.guardrail_error = Some(error.clone());
                    *body = Some(Bytes::new());
                    respond_error(session, error, &ctx.request_id).await?;
                    return Ok(());
                }
            }
            if let Some(policy) = &ctx.policy {
                if let Err(error) = evaluate_policy_limits(
                    policy,
                    Utc::now(),
                    i64::try_from(ctx.body_bytes_seen).ok(),
                    None,
                    None,
                    None,
                    None,
                ) {
                    ctx.guardrail_error = Some(error.clone());
                    *body = Some(Bytes::new());
                    respond_error(session, error, &ctx.request_id).await?;
                    return Ok(());
                }
            }
        }
        if ctx.litellm_passthrough || managed_service_request_can_stream(ctx) {
            return Ok(());
        }
        let Some(rewriter) = ctx.request_rewriter.as_mut() else {
            return Ok(());
        };

        let chunk_len = body.as_ref().map_or(0, Bytes::len);
        if chunk_len > 0 {
            if ctx.request_body_lease.is_none() {
                match self.config.body_admission.try_acquire() {
                    Ok(lease) => ctx.request_body_lease = Some(lease),
                    Err(error) => {
                        ctx.guardrail_error = Some(error.clone());
                        *body = Some(Bytes::new());
                        respond_error(session, error, &ctx.request_id).await?;
                        return Ok(());
                    }
                }
            }
            if let Some(lease) = ctx.request_body_lease.as_mut() {
                if let Err(error) = lease.try_reserve(chunk_len) {
                    ctx.request_body_lease.take();
                    drop(rewriter.take_buffer());
                    ctx.guardrail_error = Some(error.clone());
                    *body = Some(Bytes::new());
                    respond_error(session, error, &ctx.request_id).await?;
                    return Ok(());
                }
            }
        }
        if let Err(error) = rewriter.append_chunk(body.as_ref()) {
            ctx.request_body_lease.take();
            drop(rewriter.take_buffer());
            ctx.guardrail_error = Some(error);
            *body = Some(Bytes::new());
            return Ok(());
        }
        *body = Some(Bytes::new());
        if !end_of_stream {
            return Ok(());
        }

        let raw_body = rewriter.take_buffer();
        let key = ctx.key.clone();
        let route = ctx.route;
        let route_match = ctx.route_match.clone();
        let request_id = ctx.request_id.clone();
        let definitions = ctx.guardrail_definitions.clone();
        let mut policy = ctx.guardrail_policy.clone();
        let mut pricing_selector = None;
        if !ctx.service_pricing_rules.is_empty() {
            let max_body_bytes = ctx
                .route_match
                .as_ref()
                .map(|matched| matched.max_body_bytes)
                .unwrap_or(raw_body.len());
            pricing_selector = service_pricing_selector(
                &raw_body,
                ctx.request_content_type.as_deref(),
                max_body_bytes,
            )
            .await;
        }
        let analysis = analyze_generation_request(&raw_body);
        let features = analysis
            .as_ref()
            .map(|analysis| analysis.features.clone())
            .unwrap_or_default();
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
                    return Ok(());
                }
            }
        }
        let client_requested = match extract_client_guardrails_value(
            analysis
                .as_ref()
                .and_then(|analysis| analysis.client_guardrails.as_ref()),
        ) {
            Ok(client_requested) => client_requested,
            Err(error) => {
                ctx.guardrail_error = Some(error);
                return Ok(());
            }
        };
        let mut guardrail_context = ctx.guardrail_context.clone();
        let mut guardrail_events = Vec::new();
        let plan = match resolve_guardrail_plan(GuardrailPlanRequest {
            mode: GuardrailMode::PreCall,
            definitions: definitions.clone(),
            policies: GuardrailPolicySet {
                key_policy: policy.clone(),
                ..GuardrailPolicySet::default()
            },
            client_requested_guardrails: client_requested.clone(),
        }) {
            Ok(plan) => plan,
            Err(error) => {
                ctx.guardrail_error = Some(error);
                return Ok(());
            }
        };
        let post_call_plan = match resolve_guardrail_plan(GuardrailPlanRequest {
            mode: GuardrailMode::PostCall,
            definitions: definitions.clone(),
            policies: GuardrailPolicySet {
                key_policy: policy.clone(),
                ..GuardrailPolicySet::default()
            },
            client_requested_guardrails: client_requested.clone(),
        }) {
            Ok(plan) => plan,
            Err(error) => {
                ctx.guardrail_error = Some(error);
                return Ok(());
            }
        };
        let response_plan = if features.stream {
            match resolve_guardrail_plan(GuardrailPlanRequest {
                mode: GuardrailMode::DuringCall,
                definitions: definitions.clone(),
                policies: GuardrailPolicySet {
                    key_policy: policy,
                    ..GuardrailPolicySet::default()
                },
                client_requested_guardrails: client_requested,
            }) {
                Ok(during_call_plan) => {
                    if !guardrail_plan_names_match(&post_call_plan, &during_call_plan) {
                        ctx.guardrail_error = Some(GatewayError::GuardrailUnavailable);
                        return Ok(());
                    }
                    during_call_plan
                }
                Err(error) => {
                    ctx.guardrail_error = Some(error);
                    return Ok(());
                }
            }
        } else {
            post_call_plan
        };

        if plan.entries.is_empty() && response_plan.entries.is_empty() {
            *body = Some(Bytes::from(raw_body));
            if let Some(body) = body.as_ref() {
                ctx.rewritten_request_len = Some(body.len());
                ctx.body_prefix.clear();
                ctx.body_prefix
                    .extend_from_slice(&body[..body.len().min(65_536)]);
            }
            resolve_service_cost_for_ctx(ctx, pricing_selector.as_ref());
            return Ok(());
        }

        let mut request_json = match serde_json::from_slice::<serde_json::Value>(&raw_body) {
            Ok(value) => value,
            Err(_) => {
                *body = Some(Bytes::from(raw_body));
                resolve_service_cost_for_ctx(ctx, pricing_selector.as_ref());
                return Ok(());
            }
        };
        strip_client_guardrails(&mut request_json);
        let result = (|| {
            let key = key.as_ref().ok_or(GatewayError::MissingAuthorization)?;
            let mut context = guardrail_context
                .take()
                .unwrap_or_else(|| GuardrailContext {
                    request_id: request_id.clone(),
                    key_id: Some(key.key_id),
                    project_id: key.project_id,
                    route,
                    provider: route_match.as_ref().map(|matched| matched.provider),
                    model: features.model.clone(),
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
            guardrail_context = Some(context);
            serde_json::to_vec(&execution.request.unwrap_or(serde_json::Value::Null))
                .map_err(|_| GatewayError::InvalidGuardrailRequest)
        })();

        match result {
            Ok(rewritten) => *body = Some(Bytes::from(rewritten)),
            Err(error) => {
                ctx.guardrail_error = Some(error);
                *body = Some(Bytes::new());
                return Ok(());
            }
        }
        ctx.pre_guardrail_plan = Some(plan);
        if features.stream {
            ctx.during_guardrail_plan = Some(response_plan);
        } else {
            ctx.post_guardrail_plan = Some(response_plan);
        }
        ctx.guardrail_context = guardrail_context;
        ctx.guardrail_events.extend(guardrail_events);
        if let Some(body) = body.as_ref() {
            ctx.rewritten_request_len = Some(body.len());
            ctx.body_prefix.clear();
            ctx.body_prefix
                .extend_from_slice(&body[..body.len().min(65_536)]);
        }
        resolve_service_cost_for_ctx(ctx, pricing_selector.as_ref());
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
        let Some(mut matched) = ctx.route_match.clone() else {
            respond_error(session, GatewayError::UnsupportedRoute, &ctx.request_id).await?;
            return Ok(false);
        };
        let route = matched.route;
        if self.upstream_for(ctx).is_none() {
            respond_error(session, GatewayError::InvalidConfiguration, &ctx.request_id).await?;
            return Ok(false);
        }
        if ctx.trusted_ingress_passthrough {
            gateway_telemetry::record_provider_selection();
            return Ok(true);
        }
        if ctx.direct_litellm_passthrough {
            if let Err(error) = self.ensure_litellm_canonical_route_enabled(route).await {
                respond_error(session, error, &ctx.request_id).await?;
                return Ok(false);
            }
            gateway_telemetry::record_provider_selection();
            return Ok(true);
        }
        let Some(key) = ctx.key.clone() else {
            respond_error(session, GatewayError::MissingAuthorization, &ctx.request_id).await?;
            return Ok(false);
        };
        if let Some(error) = ctx.guardrail_error.clone() {
            self.record_terminal_usage(ctx, &key, route, error.status_code().as_u16(), Utc::now())
                .await;
            respond_error(session, error, &ctx.request_id).await?;
            return Ok(false);
        }
        // Body selectors are unavailable at this lifecycle stage. Reserve a
        // conservative fixed-cost ceiling and reconcile after body parsing.
        prepare_service_cost_for_ctx(ctx);
        if let Some(updated) = ctx.route_match.clone() {
            matched = updated;
        }

        let now = Utc::now();
        if let Err(error) = self.ensure_litellm_canonical_route_enabled(route).await {
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
        let effective_policy = match self
            .store
            .effective_policy_for_context(
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
        let policy = effective_policy.policy;
        ctx.guardrail_policy = effective_policy.guardrail_policy;

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
        // Receipt of upstream response headers proves the complete request body
        // has left the request-side buffer, so its process admission can be
        // released before any post-call response buffering begins.
        ctx.request_body_lease.take();
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
        if ctx.trusted_ingress_passthrough {
            rewrite_trusted_ingress_location(upstream_response, ctx)?;
        }
        let has_post_guardrails = ctx
            .post_guardrail_plan
            .as_ref()
            .is_some_and(|plan| !plan.entries.is_empty());
        if has_post_guardrails && ctx.response_body_lease.is_none() {
            let mut lease = match self.config.body_admission.try_acquire() {
                Ok(lease) => lease,
                Err(error) => {
                    ctx.guardrail_error = Some(error);
                    return Err(PingoraError::new(ErrorType::InternalError));
                }
            };
            let reservation_bytes = response_buffer_reservation_bytes(
                upstream_response,
                self.config.body_admission.max_bytes(),
            );
            if let Err(error) = lease.try_reserve(reservation_bytes) {
                ctx.guardrail_error = Some(error);
                return Err(PingoraError::new(ErrorType::InternalError));
            }
            ctx.response_body_lease = Some(lease);
        }
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
        if ctx
            .post_guardrail_plan
            .as_ref()
            .is_some_and(|plan| !plan.entries.is_empty())
        {
            let chunk_len = body.as_ref().map_or(0, Bytes::len);
            if chunk_len > 0 {
                let buffered_len = ctx
                    .response_rewriter
                    .as_ref()
                    .map_or(0, BoundedBodyRewriter::buffered_len);
                let required_bytes = buffered_len.saturating_add(chunk_len);
                if let Some(lease) = ctx.response_body_lease.as_mut() {
                    let additional_bytes = required_bytes.saturating_sub(lease.reserved_bytes());
                    if let Err(error) = lease.try_reserve(additional_bytes) {
                        ctx.response_body_lease.take();
                        if let Some(rewriter) = ctx.response_rewriter.as_mut() {
                            drop(rewriter.take_buffer());
                        }
                        ctx.guardrail_error = Some(error);
                        *body = Some(Bytes::new());
                        return Err(PingoraError::new(ErrorType::InternalError));
                    }
                }
            }
        }
        if let Err(error) = apply_post_call_guardrails(body, end_of_stream, ctx) {
            ctx.guardrail_error = Some(error);
            *body = Some(Bytes::new());
            return Err(PingoraError::new(ErrorType::InternalError));
        }
        if let Some(body) = body.as_ref() {
            ctx.response_bytes_seen = ctx.response_bytes_seen.saturating_add(body.len());
            if let Some(matched) = &ctx.route_match {
                if ctx.response_bytes_seen > matched.max_response_body_bytes {
                    ctx.guardrail_error = Some(GatewayError::ResponseBodyTooLarge);
                    return Err(PingoraError::new(ErrorType::InternalError));
                }
            }
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
                        resolved_usage_cost(ctx).estimated_cost_usd,
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
            .or(ctx.terminal_status_code)
            .unwrap_or_else(|| if error.is_some() { 502 } else { 500 });
        let usage_cost = resolved_usage_cost(ctx);
        let estimated_cost_usd = usage_cost.estimated_cost_usd;
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
        .with_cost_metadata(
            usage_cost.cost_source,
            usage_cost.cost_mode,
            usage_cost.pricing_rule_name,
        )
        .with_service_name(
            ctx.route_match
                .as_ref()
                .and_then(|matched| matched.service_name.clone()),
        )
        .with_endpoint_context(
            ctx.http_method.clone(),
            ctx.endpoint_path.clone(),
            ctx.endpoint_template.clone(),
        )
        .with_task_context(ctx.task_id.clone(), ctx.run_id.clone())
        .with_trace_id(ctx.trace_id.clone())
        .with_fallback_count(ctx.fallback_count);
        ctx.terminal_usage_recorded = true;
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

    async fn fail_to_proxy(
        &self,
        session: &mut Session,
        error: &PingoraError,
        ctx: &mut Self::CTX,
    ) -> FailToProxy
    where
        Self::CTX: Send + Sync,
    {
        if let Some(gateway_error) = ctx.guardrail_error.clone() {
            let status_code = gateway_error.status_code().as_u16();
            ctx.terminal_status_code = Some(status_code);
            if session.response_written().is_none() {
                if let Err(write_error) =
                    respond_error(session, gateway_error, &ctx.request_id).await
                {
                    tracing::error!(
                        request_id = %ctx.request_id,
                        error = %write_error,
                        "failed to write gateway body-processing error response"
                    );
                }
            }
            return FailToProxy {
                error_code: status_code,
                can_reuse_downstream: false,
            };
        }
        if is_upstream_timeout_error(error) {
            ctx.upstream_timeout = true;
            if session.response_written().is_none() {
                let gateway_error = GatewayError::UpstreamTimeout;
                ctx.terminal_status_code = Some(gateway_error.status_code().as_u16());
                if let Err(write_error) =
                    respond_error(session, gateway_error, &ctx.request_id).await
                {
                    tracing::error!(
                        request_id = %ctx.request_id,
                        error = %write_error,
                        "failed to write upstream timeout response"
                    );
                }
            }
            return FailToProxy {
                error_code: GatewayError::UpstreamTimeout.status_code().as_u16(),
                can_reuse_downstream: false,
            };
        }

        let error_code = default_proxy_failure_status(error);
        if error_code > 0 {
            session
                .respond_error(error_code)
                .await
                .unwrap_or_else(|write_error| {
                    tracing::error!(
                        request_id = %ctx.request_id,
                        error = %write_error,
                        "failed to write proxy error response"
                    );
                });
        }
        FailToProxy {
            error_code,
            can_reuse_downstream: false,
        }
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
    async fn route_enabled(&self, route: Route) -> GatewayResult<bool> {
        if gateway_core::is_anthropic_route(route) {
            self.store.anthropic_route_enabled(route).await
        } else {
            self.store.openai_route_enabled(route).await
        }
    }

    async fn route_mode(&self, route: Route) -> GatewayResult<OpenAiRouteMode> {
        if gateway_core::is_anthropic_route(route) {
            self.store.anthropic_route_mode(route).await
        } else {
            self.store.openai_route_mode(route).await
        }
    }

    async fn apply_litellm_route_limits(&self, matched: &mut RouteMatch) -> GatewayResult<()> {
        let limits = if gateway_core::is_anthropic_route(matched.route) {
            self.store.anthropic_route_limits(matched.route).await?
        } else {
            self.store.openai_route_limits(matched.route).await?
        };
        apply_litellm_limits_to_match(matched, limits)
    }

    async fn apply_litellm_passthrough_limits(
        &self,
        matched: &mut RouteMatch,
    ) -> GatewayResult<()> {
        let settings = self.store.litellm_passthrough_settings().await?;
        apply_litellm_limits_to_match(
            matched,
            gateway_core::LiteLlmRouteLimits {
                timeout_ms: settings.timeout_ms,
                max_request_body_bytes: settings.max_request_body_bytes,
                max_response_body_bytes: settings.max_response_body_bytes,
            },
        )
    }

    async fn ensure_litellm_canonical_route_enabled(&self, route: Route) -> GatewayResult<()> {
        if self.route_enabled(route).await? {
            Ok(())
        } else {
            Err(GatewayError::DisabledRoute)
        }
    }
}

impl<S, R> RelaynaPingoraProxy<S, R>
where
    S: ProviderConfigLookup,
{
    async fn configure_litellm_upstream(
        &self,
        ctx: &mut PingoraContext,
        key: Option<&AuthenticatedKey>,
    ) -> GatewayResult<()> {
        self.configure_litellm_upstream_for_credential(ctx, key, None)
            .await
    }

    async fn configure_litellm_upstream_with_credential(
        &self,
        ctx: &mut PingoraContext,
        credential: String,
    ) -> GatewayResult<()> {
        self.configure_litellm_upstream_for_credential(ctx, None, Some(credential))
            .await
    }

    async fn configure_litellm_upstream_for_credential(
        &self,
        ctx: &mut PingoraContext,
        key: Option<&AuthenticatedKey>,
        passthrough_credential: Option<String>,
    ) -> GatewayResult<()> {
        let litellm_config = self.store.active_litellm_config().await?;
        let mapped_credential = if let Some(key) = key {
            self.store
                .litellm_credential_mapping_for_context(key.key_id, key.project_id)
                .await?
                .map(|mapping| mapping.credential)
        } else {
            None
        };
        let has_passthrough_credential = passthrough_credential.is_some();
        let has_mapped_credential = mapped_credential.is_some();
        let selected_credential = passthrough_credential
            .or(mapped_credential)
            .or_else(|| {
                litellm_config
                    .as_ref()
                    .and_then(|config| config.credential.clone())
            })
            .unwrap_or_else(|| self.config.litellm.service_key.clone());
        if selected_credential.trim().is_empty() {
            return Err(GatewayError::InvalidConfiguration);
        }
        if let Some(config) = litellm_config {
            let upstream =
                PingoraUpstreamConfig::from_base_url(&config.base_url, selected_credential)?
                    .with_litellm_credential_header(
                        config.credential_header_mode,
                        config.credential_header_name,
                        config.credential_header_value_format,
                    )?;
            ctx.litellm_upstream = Some(upstream);
        } else if has_mapped_credential || has_passthrough_credential {
            let mut upstream = self.config.litellm.clone();
            upstream.service_key = selected_credential;
            ctx.litellm_upstream = Some(upstream);
        }
        Ok(())
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
        let usage_cost = resolved_usage_cost(ctx);
        let estimated_cost_usd = usage_cost.estimated_cost_usd;
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
        .with_cost_metadata(
            usage_cost.cost_source,
            usage_cost.cost_mode,
            usage_cost.pricing_rule_name,
        )
        .with_service_name(
            ctx.route_match
                .as_ref()
                .and_then(|matched| matched.service_name.clone()),
        )
        .with_endpoint_context(
            ctx.http_method.clone(),
            ctx.endpoint_path.clone(),
            ctx.endpoint_template.clone(),
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

fn litellm_bearer_credential(authorization: Option<&str>) -> GatewayResult<String> {
    let authorization = authorization.ok_or(GatewayError::MissingAuthorization)?;
    let Some(token) = authorization.strip_prefix("Bearer ") else {
        return Err(GatewayError::MalformedAuthorization);
    };
    let token = token.trim();
    if token.is_empty() {
        return Err(GatewayError::MalformedAuthorization);
    }
    Ok(token.to_owned())
}

fn authorization_has_relayna_key(authorization: Option<&str>) -> bool {
    authorization
        .and_then(|value| value.strip_prefix("Bearer "))
        .is_some_and(|token| token.trim().starts_with("rk_live_"))
}

fn request_public_origin(req: &RequestHeader) -> Option<String> {
    let host = header_value(req, "x-forwarded-host")
        .or_else(|| header_value(req, "host"))?
        .trim();
    if host.is_empty() || host.contains(char::is_whitespace) {
        return None;
    }
    let proto = header_value(req, "x-forwarded-proto")
        .unwrap_or("http")
        .trim();
    if !matches!(proto, "http" | "https") {
        return None;
    }
    Some(format!("{proto}://{host}"))
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
    upstream_request.remove_header("x-litellm-api-key");
    upstream_request.remove_header("x-litellm-key");
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
            upstream_request
                .insert_header(header_name, custom_header_credential_value(upstream))?;
        }
    }
    Ok(())
}

fn custom_header_credential_value(upstream: &PingoraUpstreamConfig) -> String {
    match upstream.credential_header_value_format {
        CredentialHeaderValueFormat::Raw => upstream.service_key.clone(),
        CredentialHeaderValueFormat::Bearer => format!("Bearer {}", upstream.service_key),
    }
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

fn rewrite_trusted_ingress_location(
    upstream_response: &mut ResponseHeader,
    ctx: &PingoraContext,
) -> PingoraResult<()> {
    let Some(public_origin) = ctx.public_origin.as_deref() else {
        return Ok(());
    };
    let Some(location) = upstream_response
        .headers
        .get("location")
        .and_then(|value| value.to_str().ok())
    else {
        return Ok(());
    };
    let Some(rewritten) = trusted_ingress_location(location, public_origin) else {
        return Ok(());
    };
    upstream_response.remove_header("location");
    upstream_response.insert_header("location", rewritten)?;
    Ok(())
}

fn trusted_ingress_location(location: &str, public_origin: &str) -> Option<String> {
    if location == "/ui" || location.starts_with("/ui/") || location.starts_with("/ui?") {
        return Some(format!("{public_origin}{location}"));
    }
    let uri = location.parse::<Uri>().ok()?;
    let path_and_query = uri.path_and_query()?.as_str();
    if path_and_query == "/ui"
        || path_and_query.starts_with("/ui/")
        || path_and_query.starts_with("/ui?")
    {
        Some(format!("{public_origin}{path_and_query}"))
    } else {
        None
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
        Some(LiteLlmSensitiveRouteExposure::ExplicitlyExposed)
        | Some(LiteLlmSensitiveRouteExposure::TrustedIngress)
        | None => true,
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

fn apply_litellm_limits_to_match(
    matched: &mut RouteMatch,
    limits: gateway_core::LiteLlmRouteLimits,
) -> GatewayResult<()> {
    matched.timeout_ms =
        u64::try_from(limits.timeout_ms).map_err(|_| GatewayError::InvalidConfiguration)?;
    matched.max_body_bytes = usize::try_from(limits.max_request_body_bytes)
        .map_err(|_| GatewayError::InvalidConfiguration)?;
    matched.max_response_body_bytes = usize::try_from(limits.max_response_body_bytes)
        .map_err(|_| GatewayError::InvalidConfiguration)?;
    Ok(())
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

fn apply_service_registration_runtime_limits(
    matched: &mut RouteMatch,
    timeout_ms: i64,
    max_body_bytes: i64,
) -> PingoraResult<()> {
    matched.timeout_ms = u64::try_from(timeout_ms)
        .map_err(|_| pingora_core::Error::new(ErrorType::InternalError))?;
    matched.max_body_bytes = usize::try_from(max_body_bytes)
        .map_err(|_| pingora_core::Error::new(ErrorType::InternalError))?;
    matched.max_response_body_bytes = usize::MAX;
    Ok(())
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

fn managed_service_request_can_stream(ctx: &PingoraContext) -> bool {
    let is_service = ctx
        .route_match
        .as_ref()
        .and_then(|matched| matched.service_name.as_ref())
        .is_some();
    let is_non_json = ctx
        .request_content_type
        .as_deref()
        .is_some_and(|content_type| !is_json_content_type(content_type));
    if !is_service || !is_non_json || ctx.policy.is_none() || !ctx.service_pricing_rules.is_empty()
    {
        return false;
    }
    resolve_guardrail_plan(GuardrailPlanRequest {
        mode: GuardrailMode::PreCall,
        definitions: ctx.guardrail_definitions.clone(),
        policies: GuardrailPolicySet {
            key_policy: ctx.guardrail_policy.clone(),
            ..GuardrailPolicySet::default()
        },
        client_requested_guardrails: Vec::new(),
    })
    .is_ok_and(|plan| plan.entries.is_empty())
}

fn is_json_content_type(content_type: &str) -> bool {
    content_type.split(';').next().is_some_and(|media_type| {
        let media_type = media_type.trim().to_ascii_lowercase();
        media_type == "application/json" || media_type.ends_with("+json")
    })
}

fn response_buffer_reservation_bytes(
    upstream_response: &ResponseHeader,
    unknown_length_reservation: usize,
) -> usize {
    upstream_response
        .headers
        .get(header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(unknown_length_reservation)
}

#[derive(Debug, Clone, PartialEq)]
struct ResolvedUsageCost {
    estimated_cost_usd: Option<f64>,
    cost_source: Option<String>,
    cost_mode: Option<ServiceCostMode>,
    pricing_rule_name: Option<String>,
}

async fn service_pricing_selector(
    body: &[u8],
    content_type: Option<&str>,
    max_body_bytes: usize,
) -> Option<serde_json::Value> {
    let boundary = content_type.and_then(|value| multra::parse_boundary(value).ok());
    let Some(boundary) = boundary else {
        return serde_json::from_slice(body).ok();
    };

    let whole_stream_limit = u64::try_from(max_body_bytes).unwrap_or(u64::MAX);
    let constraints = Constraints::new().size_limit(
        SizeLimit::new()
            .whole_stream(whole_stream_limit)
            .per_field(whole_stream_limit),
    );
    // The request rewriter already owns this bounded body. Multra borrows it
    // and drains file fields without copying them into selector metadata.
    let mut multipart = Multipart::with_reader_and_constraints(body, boundary, constraints);
    let mut selector = serde_json::Map::new();
    let mut retained_bytes = 0_usize;
    let mut retained_fields = 0_usize;

    loop {
        let mut field = match multipart.next_field().await {
            Ok(Some(field)) => field,
            Ok(None) => break,
            Err(_) => return None,
        };
        let name = field
            .name()
            .filter(|name| name.len() <= MAX_MULTIPART_PRICING_FIELD_NAME_BYTES)
            .map(ToOwned::to_owned);
        let retain = field.file_name().is_none()
            && name.is_some()
            && retained_fields < MAX_MULTIPART_PRICING_FIELDS;
        let mut value = Vec::new();
        let mut oversized = false;

        loop {
            let chunk = match field.chunk().await {
                Ok(Some(chunk)) => chunk,
                Ok(None) => break,
                Err(_) => return None,
            };
            if !retain || oversized {
                continue;
            }
            let field_len = value.len().saturating_add(chunk.len());
            let total_len = retained_bytes.saturating_add(field_len);
            if field_len > MAX_MULTIPART_PRICING_FIELD_VALUE_BYTES
                || total_len > MAX_MULTIPART_PRICING_METADATA_BYTES
            {
                value.clear();
                oversized = true;
                continue;
            }
            value.extend_from_slice(&chunk);
        }

        if !retain || oversized {
            continue;
        }
        retained_fields = retained_fields.saturating_add(1);
        let Ok(value) = String::from_utf8(value) else {
            continue;
        };
        retained_bytes = retained_bytes.saturating_add(value.len());
        let Some(name) = name else {
            continue;
        };
        selector.insert(name, value.into());
    }

    Some(serde_json::Value::Object(selector))
}

fn prepare_service_cost_for_ctx(ctx: &mut PingoraContext) {
    let Some(matched) = ctx.route_match.as_mut() else {
        return;
    };
    if matched.provider != Provider::InternalService {
        return;
    }
    let base = ctx
        .resolved_endpoint_cost
        .clone()
        .unwrap_or(ResolvedServiceCost {
            cost_mode: ctx.service_cost_mode.unwrap_or(ServiceCostMode::None),
            estimated_cost_usd: ctx.service_estimated_cost_usd,
            pricing_rule_name: None,
        });
    if ctx.resolved_endpoint_cost.is_some() && base.cost_mode == ServiceCostMode::None {
        matched.estimated_cost_usd = None;
        ctx.resolved_service_cost = Some(base);
        return;
    }
    matched.estimated_cost_usd = service_preflight_estimated_cost(
        base.cost_mode,
        base.estimated_cost_usd,
        &ctx.service_pricing_rules,
    );
    ctx.resolved_service_cost = Some(base);
}

fn resolve_service_cost_for_ctx(ctx: &mut PingoraContext, selector: Option<&serde_json::Value>) {
    let Some(matched) = ctx.route_match.as_mut() else {
        return;
    };
    if matched.provider != Provider::InternalService {
        return;
    }
    let base = ctx
        .resolved_endpoint_cost
        .clone()
        .unwrap_or(ResolvedServiceCost {
            cost_mode: ctx.service_cost_mode.unwrap_or(ServiceCostMode::None),
            estimated_cost_usd: ctx.service_estimated_cost_usd,
            pricing_rule_name: None,
        });
    if ctx.resolved_endpoint_cost.is_some() && base.cost_mode == ServiceCostMode::None {
        matched.estimated_cost_usd = None;
        ctx.resolved_service_cost = Some(base);
        return;
    }
    let resolved = selector.map_or_else(
        || base.clone(),
        |value| {
            let body_rule_matched =
                matching_service_pricing_rule(value, &ctx.service_pricing_rules).is_some();
            let mut resolved = resolve_service_cost_from_value(
                value,
                base.cost_mode,
                base.estimated_cost_usd,
                &ctx.service_pricing_rules,
            );
            if !body_rule_matched {
                resolved
                    .pricing_rule_name
                    .clone_from(&base.pricing_rule_name);
            }
            resolved
        },
    );
    matched.estimated_cost_usd = match resolved.cost_mode {
        ServiceCostMode::Fixed => resolved.estimated_cost_usd,
        ServiceCostMode::Passthrough | ServiceCostMode::None => None,
    };
    ctx.resolved_service_cost = Some(resolved);
}

fn configure_service_pricing_context(
    ctx: &mut PingoraContext,
    registration: &gateway_core::ServiceRegistration,
    method: &http::Method,
    public_path: &str,
) {
    ctx.service_cost_mode = Some(registration.cost_mode);
    ctx.service_estimated_cost_usd = registration.estimated_cost_usd;
    ctx.service_pricing_rules = registration.pricing_rules.clone();
    let endpoint_path = route_pattern_wildcard_suffix(public_path, &registration.route_pattern)
        .unwrap_or_else(|| public_path.to_owned());
    ctx.http_method = Some(method.as_str().to_ascii_uppercase());
    ctx.endpoint_path = Some(endpoint_path.clone());
    ctx.endpoint_template =
        matching_openapi_endpoint(method, &endpoint_path, &registration.openapi_endpoints)
            .map(|endpoint| endpoint.path_template.clone());
    ctx.resolved_endpoint_cost =
        resolve_endpoint_pricing_rule(method, &endpoint_path, &registration.endpoint_pricing_rules);
}

fn resolved_usage_cost(ctx: &PingoraContext) -> ResolvedUsageCost {
    if ctx.litellm_passthrough {
        return ResolvedUsageCost {
            estimated_cost_usd: None,
            cost_source: Some("none".to_owned()),
            cost_mode: Some(ServiceCostMode::None),
            pricing_rule_name: None,
        };
    }

    if let Some(service_cost) = &ctx.resolved_service_cost {
        let from_rule = service_cost.pricing_rule_name.is_some();
        return match service_cost.cost_mode {
            ServiceCostMode::None => ResolvedUsageCost {
                estimated_cost_usd: None,
                cost_source: Some("none".to_owned()),
                cost_mode: Some(ServiceCostMode::None),
                pricing_rule_name: service_cost.pricing_rule_name.clone(),
            },
            ServiceCostMode::Fixed => ResolvedUsageCost {
                estimated_cost_usd: service_cost.estimated_cost_usd,
                cost_source: Some(
                    if from_rule {
                        "service_pricing_rule_fixed"
                    } else {
                        "service_default_fixed"
                    }
                    .to_owned(),
                ),
                cost_mode: Some(ServiceCostMode::Fixed),
                pricing_rule_name: service_cost.pricing_rule_name.clone(),
            },
            ServiceCostMode::Passthrough => {
                let upstream_cost = extract_estimated_cost_usd(&ctx.response_body_prefix);
                ResolvedUsageCost {
                    estimated_cost_usd: upstream_cost,
                    cost_source: Some(
                        if upstream_cost.is_some() {
                            if from_rule {
                                "service_pricing_rule_passthrough"
                            } else {
                                "service_default_passthrough"
                            }
                        } else {
                            "missing_upstream_cost"
                        }
                        .to_owned(),
                    ),
                    cost_mode: Some(ServiceCostMode::Passthrough),
                    pricing_rule_name: service_cost.pricing_rule_name.clone(),
                }
            }
        };
    }

    if let Some(upstream_cost) = extract_estimated_cost_usd(&ctx.response_body_prefix) {
        return ResolvedUsageCost {
            estimated_cost_usd: Some(upstream_cost),
            cost_source: Some("upstream_passthrough".to_owned()),
            cost_mode: None,
            pricing_rule_name: None,
        };
    }

    let route_cost = ctx
        .route_match
        .as_ref()
        .and_then(|matched| matched.estimated_cost_usd);
    ResolvedUsageCost {
        estimated_cost_usd: route_cost,
        cost_source: route_cost.map(|_| "route_default".to_owned()),
        cost_mode: None,
        pricing_rule_name: None,
    }
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
    if ctx.upstream_timeout {
        selection_trace.push("terminal_error=upstream_timeout".to_owned());
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

fn is_upstream_timeout_error(error: &PingoraError) -> bool {
    error.esource() == &ErrorSource::Upstream
        && matches!(
            error.etype(),
            ErrorType::ConnectTimedout
                | ErrorType::TLSHandshakeTimedout
                | ErrorType::ReadTimedout
                | ErrorType::WriteTimedout
        )
}

fn default_proxy_failure_status(error: &PingoraError) -> u16 {
    match error.etype() {
        ErrorType::HTTPStatus(code) => *code,
        _ => match error.esource() {
            ErrorSource::Upstream => 502,
            ErrorSource::Downstream => match error.etype() {
                ErrorType::WriteError | ErrorType::ReadError | ErrorType::ConnectionClosed => 0,
                _ => 400,
            },
            ErrorSource::Internal | ErrorSource::Unset => 500,
        },
    }
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
        request_content_type: None,
        body_prefix: Vec::new(),
        body_bytes_seen: 0,
        response_body_prefix: Vec::new(),
        response_bytes_seen: 0,
        policy: None,
        request_rewriter: None,
        response_rewriter: None,
        request_body_lease: None,
        response_body_lease: None,
        is_streaming: false,
        first_chunk_recorded: false,
        budget_reserved: false,
        task_id: None,
        run_id: None,
        traceparent: None,
        trace_id: None,
        public_origin: None,
        fallback_count: 0,
        terminal_usage_recorded: false,
        terminal_status_code: None,
        upstream_timeout: false,
        service_upstream: None,
        service_route_pattern: None,
        http_method: None,
        endpoint_path: None,
        endpoint_template: None,
        service_cost_mode: None,
        service_estimated_cost_usd: None,
        service_pricing_rules: Vec::new(),
        resolved_endpoint_cost: None,
        resolved_service_cost: None,
        litellm_upstream: None,
        litellm_passthrough: false,
        trusted_ingress_passthrough: false,
        direct_litellm_passthrough: false,
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
    let (response, body) = gateway_error_response(error, request_id)?;
    session
        .write_response_header(Box::new(response), false)
        .await?;
    session.write_response_body(Some(body), true).await
}

fn gateway_error_response(
    error: GatewayError,
    request_id: &str,
) -> PingoraResult<(ResponseHeader, Bytes)> {
    let body = serde_json::to_vec(&error.body(request_id)).unwrap_or_else(|_| b"{}".to_vec());
    let mut response = ResponseHeader::build(error.status_code().as_u16(), Some(4))?;
    response.insert_header(header::CONTENT_TYPE, "application/json")?;
    response.insert_header(header::CONTENT_LENGTH, body.len().to_string())?;
    response.insert_header(header::CACHE_CONTROL, "private, no-store")?;
    Ok((response, Bytes::from(body)))
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
    fn gateway_timeout_response_uses_stable_json_envelope() {
        let request_id = "req_timeout_123";
        let (response, body) = gateway_error_response(GatewayError::UpstreamTimeout, request_id)
            .expect("gateway error response");

        assert_eq!(response.status.as_u16(), 504);
        assert_eq!(
            response
                .headers
                .get(header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok()),
            Some("application/json")
        );
        let content_length = body.len().to_string();
        assert_eq!(
            response
                .headers
                .get(header::CONTENT_LENGTH)
                .and_then(|value| value.to_str().ok()),
            Some(content_length.as_str())
        );
        let value: serde_json::Value = serde_json::from_slice(&body).expect("json body");
        assert_eq!(value["error"]["code"], "upstream_timeout");
        assert_eq!(value["error"]["message"], "Upstream provider timed out.");
        assert_eq!(value["error"]["request_id"], request_id);
    }

    #[test]
    fn only_upstream_timeout_types_receive_timeout_classification() {
        for error_type in [
            ErrorType::ConnectTimedout,
            ErrorType::TLSHandshakeTimedout,
            ErrorType::ReadTimedout,
            ErrorType::WriteTimedout,
        ] {
            let error = PingoraError::new_up(error_type);
            assert!(is_upstream_timeout_error(&error));
        }

        assert!(!is_upstream_timeout_error(&PingoraError::new_up(
            ErrorType::ConnectRefused
        )));
        assert!(!is_upstream_timeout_error(&PingoraError::new_down(
            ErrorType::ReadTimedout
        )));
        assert_eq!(
            default_proxy_failure_status(&PingoraError::new_up(ErrorType::ConnectRefused)),
            502
        );
        assert_eq!(
            default_proxy_failure_status(&PingoraError::new_up(ErrorType::HTTPStatus(503))),
            503
        );
    }

    #[tokio::test]
    async fn multipart_pricing_extracts_text_field_after_large_file_part() {
        let boundary = "ocr-pricing-boundary";
        let mut body = format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"document.pdf\"\r\nContent-Type: application/pdf\r\n\r\n"
        )
        .into_bytes();
        body.extend(std::iter::repeat_n(b'x', 70_000));
        body.extend_from_slice(
            format!(
                "\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"engine\"\r\n\r\ndocint\r\n--{boundary}--\r\n"
            )
            .as_bytes(),
        );
        let content_type = format!("multipart/form-data; boundary=\"{boundary}\"");

        let selector = service_pricing_selector(&body, Some(&content_type), body.len())
            .await
            .expect("multipart selector");

        assert_eq!(
            selector.pointer("/engine").and_then(|value| value.as_str()),
            Some("docint")
        );
        assert!(selector.pointer("/file").is_none());

        let resolved = resolve_service_cost_from_value(
            &selector,
            ServiceCostMode::Fixed,
            Some(0.01),
            &[ServicePricingRule {
                name: Some("docint".to_owned()),
                json_pointer: "/engine".to_owned(),
                equals: "docint".to_owned(),
                cost_mode: ServiceCostMode::Fixed,
                estimated_cost_usd: Some(0.5),
            }],
        );
        assert_eq!(resolved.estimated_cost_usd, Some(0.5));
        assert_eq!(resolved.pricing_rule_name.as_deref(), Some("docint"));
    }

    #[tokio::test]
    async fn multipart_pricing_skips_oversized_text_and_continues() {
        let boundary = "bounded-pricing";
        let mut body =
            format!("--{boundary}\r\nContent-Disposition: form-data; name=\"notes\"\r\n\r\n")
                .into_bytes();
        body.extend(std::iter::repeat_n(
            b'n',
            MAX_MULTIPART_PRICING_FIELD_VALUE_BYTES + 1,
        ));
        body.extend_from_slice(
            format!(
                "\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"engine\"\r\n\r\ninternal\r\n--{boundary}--\r\n"
            )
            .as_bytes(),
        );
        let content_type = format!("multipart/form-data; boundary={boundary}");

        let selector = service_pricing_selector(&body, Some(&content_type), body.len())
            .await
            .expect("multipart selector");

        assert!(selector.pointer("/notes").is_none());
        assert_eq!(
            selector.pointer("/engine").and_then(|value| value.as_str()),
            Some("internal")
        );
    }

    #[tokio::test]
    async fn malformed_multipart_pricing_falls_back_without_selector() {
        let selector = service_pricing_selector(
            b"--expected\r\nContent-Disposition: form-data; name=\"engine\"\r\n\r\ndocint",
            Some("multipart/form-data; boundary=expected"),
            1024,
        )
        .await;

        assert!(selector.is_none());
    }

    #[test]
    fn service_pricing_preflight_reserves_ceiling_then_resolves_selector() {
        let mut ctx = new_pingora_context_for_tests();
        ctx.route_match = Some(service_route_match_for_persisted_registration(
            &http::Method::POST,
            "/services/ocr/ocr",
            "ocr",
        ));
        ctx.service_cost_mode = Some(ServiceCostMode::Fixed);
        ctx.service_estimated_cost_usd = Some(0.01);
        ctx.service_pricing_rules = vec![ServicePricingRule {
            name: Some("docint".to_owned()),
            json_pointer: "/engine".to_owned(),
            equals: "docint".to_owned(),
            cost_mode: ServiceCostMode::Fixed,
            estimated_cost_usd: Some(0.5),
        }];

        prepare_service_cost_for_ctx(&mut ctx);

        assert_eq!(
            ctx.route_match
                .as_ref()
                .and_then(|matched| matched.estimated_cost_usd),
            Some(0.5)
        );
        assert_eq!(
            ctx.resolved_service_cost
                .as_ref()
                .and_then(|cost| cost.estimated_cost_usd),
            Some(0.01)
        );

        resolve_service_cost_for_ctx(&mut ctx, Some(&serde_json::json!({"engine": "docint"})));

        let resolved = ctx.resolved_service_cost.as_ref().expect("resolved cost");
        assert_eq!(resolved.estimated_cost_usd, Some(0.5));
        assert_eq!(resolved.pricing_rule_name.as_deref(), Some("docint"));
        let usage = resolved_usage_cost(&ctx);
        assert_eq!(
            usage.cost_source.as_deref(),
            Some("service_pricing_rule_fixed")
        );
        assert_eq!(usage.estimated_cost_usd, Some(0.5));
    }

    #[test]
    fn service_endpoint_context_uses_template_with_concrete_fallback() {
        let now = Utc::now();
        let registration = gateway_core::ServiceRegistration {
            name: "jobs".to_owned(),
            project_id: None,
            studio_service_id: None,
            route_pattern: "/services/jobs/*".to_owned(),
            upstream_base_url: Some("http://jobs.example".to_owned()),
            health_check_path: None,
            health_check_method: "GET".to_owned(),
            enabled: true,
            allowed_methods: vec!["POST".to_owned()],
            timeout_ms: 60_000,
            max_body_bytes: 1_024,
            cost_mode: ServiceCostMode::None,
            estimated_cost_usd: None,
            pricing_rules: Vec::new(),
            openapi_source_path: Some("/openapi.json".to_owned()),
            openapi_schema_hash: Some("schema".to_owned()),
            openapi_synced_at: Some(now),
            openapi_endpoints: vec![gateway_core::ServiceOpenApiEndpoint {
                method: "POST".to_owned(),
                path_template: "/jobs/{job_id}".to_owned(),
                operation_id: Some("submit_job".to_owned()),
                summary: None,
                relayna_default: false,
            }],
            endpoint_pricing_rules: Vec::new(),
            credential_secret: None,
            fallback_services: Vec::new(),
            source: gateway_core::ServiceSource::Gateway,
            sync_status: gateway_core::ServiceSyncStatus::Synced,
            last_synced_at: Some(now),
            disabled_at: None,
            created_at: now,
            updated_at: now,
        };
        let mut ctx = new_pingora_context_for_tests();

        configure_service_pricing_context(
            &mut ctx,
            &registration,
            &http::Method::POST,
            "/services/jobs/jobs/123",
        );
        assert_eq!(ctx.http_method.as_deref(), Some("POST"));
        assert_eq!(ctx.endpoint_path.as_deref(), Some("/jobs/123"));
        assert_eq!(ctx.endpoint_template.as_deref(), Some("/jobs/{job_id}"));

        configure_service_pricing_context(
            &mut ctx,
            &registration,
            &http::Method::POST,
            "/services/jobs/unlisted/123",
        );
        assert_eq!(ctx.endpoint_path.as_deref(), Some("/unlisted/123"));
        assert_eq!(ctx.endpoint_template, None);
    }

    #[test]
    fn free_endpoint_skips_unrelated_body_price_and_budget_ceiling() {
        let mut ctx = new_pingora_context_for_tests();
        ctx.route_match = Some(service_route_match_for_persisted_registration(
            &http::Method::GET,
            "/services/ocr/events/feed",
            "ocr",
        ));
        ctx.service_cost_mode = Some(ServiceCostMode::Fixed);
        ctx.service_estimated_cost_usd = Some(0.01);
        ctx.service_pricing_rules = vec![ServicePricingRule {
            name: Some("docint".to_owned()),
            json_pointer: "/engine".to_owned(),
            equals: "docint".to_owned(),
            cost_mode: ServiceCostMode::Fixed,
            estimated_cost_usd: Some(0.5),
        }];
        ctx.resolved_endpoint_cost = Some(ResolvedServiceCost {
            cost_mode: ServiceCostMode::None,
            estimated_cost_usd: None,
            pricing_rule_name: Some("feed_events_feed_get".to_owned()),
        });

        prepare_service_cost_for_ctx(&mut ctx);
        assert_eq!(
            ctx.route_match
                .as_ref()
                .and_then(|matched| matched.estimated_cost_usd),
            None
        );
        resolve_service_cost_for_ctx(&mut ctx, Some(&serde_json::json!({"engine": "docint"})));

        let usage = resolved_usage_cost(&ctx);
        assert_eq!(usage.estimated_cost_usd, None);
        assert_eq!(usage.cost_source.as_deref(), Some("none"));
        assert_eq!(
            usage.pricing_rule_name.as_deref(),
            Some("feed_events_feed_get")
        );
    }

    #[test]
    fn fixed_endpoint_reserves_body_ceiling_and_allows_body_override() {
        let mut ctx = new_pingora_context_for_tests();
        ctx.route_match = Some(service_route_match_for_persisted_registration(
            &http::Method::POST,
            "/services/ocr/ocr",
            "ocr",
        ));
        ctx.service_pricing_rules = vec![ServicePricingRule {
            name: Some("docint".to_owned()),
            json_pointer: "/engine".to_owned(),
            equals: "docint".to_owned(),
            cost_mode: ServiceCostMode::Fixed,
            estimated_cost_usd: Some(0.5),
        }];
        ctx.resolved_endpoint_cost = Some(ResolvedServiceCost {
            cost_mode: ServiceCostMode::Fixed,
            estimated_cost_usd: Some(0.01),
            pricing_rule_name: Some("submit_ocr_task_ocr_post".to_owned()),
        });

        prepare_service_cost_for_ctx(&mut ctx);
        assert_eq!(
            ctx.route_match
                .as_ref()
                .and_then(|matched| matched.estimated_cost_usd),
            Some(0.5)
        );
        resolve_service_cost_for_ctx(&mut ctx, Some(&serde_json::json!({"engine": "docint"})));
        let resolved = ctx.resolved_service_cost.as_ref().expect("resolved cost");
        assert_eq!(resolved.estimated_cost_usd, Some(0.5));
        assert_eq!(resolved.pricing_rule_name.as_deref(), Some("docint"));
    }

    #[test]
    fn anonymous_body_rule_does_not_inherit_endpoint_operation_id() {
        let mut ctx = new_pingora_context_for_tests();
        ctx.route_match = Some(service_route_match_for_persisted_registration(
            &http::Method::POST,
            "/services/ocr/ocr",
            "ocr",
        ));
        ctx.service_pricing_rules = vec![ServicePricingRule {
            name: None,
            json_pointer: "/engine".to_owned(),
            equals: "docint".to_owned(),
            cost_mode: ServiceCostMode::Fixed,
            estimated_cost_usd: Some(0.5),
        }];
        ctx.resolved_endpoint_cost = Some(ResolvedServiceCost {
            cost_mode: ServiceCostMode::Fixed,
            estimated_cost_usd: Some(0.01),
            pricing_rule_name: Some("submit_ocr".to_owned()),
        });

        resolve_service_cost_for_ctx(&mut ctx, Some(&serde_json::json!({"engine": "docint"})));

        let resolved = ctx.resolved_service_cost.as_ref().expect("resolved cost");
        assert_eq!(resolved.estimated_cost_usd, Some(0.5));
        assert_eq!(resolved.pricing_rule_name, None);
    }

    #[test]
    fn usage_cost_resolution_covers_fixed_passthrough_route_and_unpriced_sources() {
        let mut empty = new_pingora_context_for_tests();
        prepare_service_cost_for_ctx(&mut empty);
        resolve_service_cost_for_ctx(&mut empty, None);
        assert_eq!(resolved_usage_cost(&empty).estimated_cost_usd, None);

        let mut non_service = new_pingora_context_for_tests();
        non_service.route_match = Some(
            Route::resolve_match(&http::Method::POST, "/v1/chat/completions").expect("chat route"),
        );
        non_service
            .route_match
            .as_mut()
            .expect("chat route match")
            .estimated_cost_usd = Some(0.01);
        prepare_service_cost_for_ctx(&mut non_service);
        resolve_service_cost_for_ctx(&mut non_service, None);
        assert_eq!(
            resolved_usage_cost(&non_service).cost_source.as_deref(),
            Some("route_default")
        );

        let mut fixed = new_pingora_context_for_tests();
        fixed.route_match = Some(service_route_match_for_persisted_registration(
            &http::Method::POST,
            "/services/ocr/run",
            "ocr",
        ));
        fixed.service_cost_mode = Some(ServiceCostMode::Fixed);
        fixed.service_estimated_cost_usd = Some(0.01);
        prepare_service_cost_for_ctx(&mut fixed);
        resolve_service_cost_for_ctx(&mut fixed, None);
        assert_eq!(
            resolved_usage_cost(&fixed).cost_source.as_deref(),
            Some("service_default_fixed")
        );

        let mut passthrough = new_pingora_context_for_tests();
        passthrough.resolved_service_cost = Some(ResolvedServiceCost {
            cost_mode: ServiceCostMode::Passthrough,
            estimated_cost_usd: None,
            pricing_rule_name: Some("upstream-price".to_owned()),
        });
        passthrough.response_body_prefix = br#"{"usage":{"total_cost":0.42}}"#.to_vec();
        let usage = resolved_usage_cost(&passthrough);
        assert_eq!(usage.estimated_cost_usd, Some(0.42));
        assert_eq!(
            usage.cost_source.as_deref(),
            Some("service_pricing_rule_passthrough")
        );
        passthrough
            .resolved_service_cost
            .as_mut()
            .expect("service cost")
            .pricing_rule_name = None;
        assert_eq!(
            resolved_usage_cost(&passthrough).cost_source.as_deref(),
            Some("service_default_passthrough")
        );
        passthrough.response_body_prefix.clear();
        assert_eq!(
            resolved_usage_cost(&passthrough).cost_source.as_deref(),
            Some("missing_upstream_cost")
        );

        let mut upstream = new_pingora_context_for_tests();
        upstream.response_body_prefix = br#"{"usage":{"total_cost":0.05}}"#.to_vec();
        assert_eq!(
            resolved_usage_cost(&upstream).cost_source.as_deref(),
            Some("upstream_passthrough")
        );
        upstream.litellm_passthrough = true;
        assert_eq!(
            resolved_usage_cost(&upstream).cost_source.as_deref(),
            Some("none")
        );
    }

    #[test]
    fn trace_and_proxy_error_helpers_cover_invalid_and_non_upstream_inputs() {
        assert_eq!(
            trace_id_from_traceparent("00-ABCDEF0123456789ABCDEF0123456789-0123456789abcdef-01")
                .as_deref(),
            Some("abcdef0123456789abcdef0123456789")
        );
        assert_eq!(trace_id_from_traceparent("missing"), None);
        assert_eq!(trace_id_from_traceparent("00-short-span-01"), None);

        let downstream_closed = PingoraError::new_down(ErrorType::ConnectionClosed);
        assert_eq!(default_proxy_failure_status(&downstream_closed), 0);
        assert!(!is_retry_safe_proxy_error(&downstream_closed));
        assert_eq!(
            default_proxy_failure_status(&PingoraError::new_down(ErrorType::InvalidHTTPHeader)),
            400
        );
        assert_eq!(
            default_proxy_failure_status(&PingoraError::new(ErrorType::InternalError)),
            500
        );
        assert!(is_retry_safe_proxy_error(&PingoraError::new_up(
            ErrorType::WriteTimedout
        )));
        assert!(!is_retry_safe_proxy_error(&PingoraError::new_up(
            ErrorType::ConnectRefused
        )));
    }

    #[test]
    fn debug_bundle_records_committed_stream_timeout_without_replacing_status() {
        let mut ctx = new_pingora_context_for_tests();
        ctx.route = Some(Route::ChatCompletions);
        ctx.route_match = Some(
            Route::resolve_match(&http::Method::POST, "/v1/chat/completions").expect("route match"),
        );
        ctx.upstream_timeout = true;

        let bundle = debug_bundle_for_ctx(&ctx, 200);

        assert!(bundle
            .selection_trace
            .iter()
            .any(|entry| entry == "terminal_error=upstream_timeout"));
        assert!(bundle.fallback_history.is_empty());
    }

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
                    CredentialHeaderValueFormat::Raw,
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
    fn upstream_header_preparation_can_use_bearer_custom_litellm_header() {
        let upstream =
            PingoraUpstreamConfig::from_base_url("https://litellm.internal", "vk-litellm")
                .expect("service config")
                .with_litellm_credential_header(
                    CredentialHeaderMode::CustomHeader,
                    Some("x-litellm-key".to_owned()),
                    CredentialHeaderValueFormat::Bearer,
                )
                .expect("custom header");
        let mut request = RequestHeader::build("POST", b"/v1/responses", None).expect("request");
        request
            .insert_header("authorization", "Bearer rk_live_client_key")
            .expect("client authorization");
        request
            .insert_header("x-litellm-key", "client-supplied-litellm-key")
            .expect("client litellm key");
        request
            .insert_header("x-litellm-api-key", "client-supplied-alt-litellm-key")
            .expect("client alternate litellm key");

        prepare_upstream_authority_and_credentials(&mut request, &upstream, Some("x-relayna-key"))
            .expect("prepared upstream headers");

        assert!(!request.headers.contains_key("authorization"));
        assert!(!request.headers.contains_key("x-litellm-api-key"));
        assert_eq!(
            request
                .headers
                .get("x-litellm-key")
                .and_then(|value| value.to_str().ok()),
            Some("Bearer vk-litellm")
        );
    }

    #[test]
    fn litellm_bearer_credential_accepts_non_relayna_keys() {
        assert_eq!(
            litellm_bearer_credential(Some("Bearer sk-litellm-client")).expect("credential"),
            "sk-litellm-client"
        );
        assert_eq!(
            litellm_bearer_credential(None).unwrap_err(),
            GatewayError::MissingAuthorization
        );
        assert_eq!(
            litellm_bearer_credential(Some("Basic sk-litellm-client")).unwrap_err(),
            GatewayError::MalformedAuthorization
        );
        assert_eq!(
            litellm_bearer_credential(Some("Bearer   ")).unwrap_err(),
            GatewayError::MalformedAuthorization
        );
    }

    #[test]
    fn relayna_authorization_is_not_litellm_passthrough_credential() {
        let relayna_key = ["rk", "live", "existing_client_key"].join("_");
        let relayna_bearer = format!("Bearer {relayna_key}");
        let spaced_relayna_bearer = format!("Bearer   {relayna_key}");
        let basic_relayna = format!("Basic {relayna_key}");

        assert!(authorization_has_relayna_key(Some(&relayna_bearer)));
        assert!(authorization_has_relayna_key(Some(&spaced_relayna_bearer)));
        assert!(!authorization_has_relayna_key(Some(
            "Bearer sk-litellm-client"
        )));
        assert!(!authorization_has_relayna_key(Some(&basic_relayna)));
        assert!(!authorization_has_relayna_key(None));
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
    fn persisted_service_runtime_limits_do_not_cap_responses() {
        let mut matched = service_route_match_for_persisted_registration(
            &http::Method::POST,
            "/services/custom/run",
            "custom",
        );

        apply_service_registration_runtime_limits(&mut matched, 45_000, 64 * 1024).expect("limits");

        assert_eq!(matched.timeout_ms, 45_000);
        assert_eq!(matched.max_body_bytes, 64 * 1024);
        assert_eq!(matched.max_response_body_bytes, usize::MAX);
    }

    #[test]
    fn managed_service_streams_only_when_body_work_is_not_required() {
        let mut ctx = new_pingora_context_for_tests();
        ctx.route_match = Some(RouteMatch::service(Route::ServiceWildcard, "documents"));
        ctx.policy = Some(KeyPolicy::default());
        ctx.request_content_type = Some("multipart/form-data; boundary=documents".to_owned());
        ctx.guardrail_definitions
            .push(gateway_core::pii_redact_definition());

        assert!(managed_service_request_can_stream(&ctx));

        ctx.service_pricing_rules.push(ServicePricingRule {
            name: Some("priced".to_owned()),
            json_pointer: "/tier".to_owned(),
            equals: "premium".to_owned(),
            cost_mode: ServiceCostMode::Fixed,
            estimated_cost_usd: Some(1.0),
        });
        assert!(!managed_service_request_can_stream(&ctx));

        ctx.service_pricing_rules.clear();
        ctx.guardrail_policy.mandatory_guardrails =
            vec![gateway_core::PII_REDACT_GUARDRAIL.to_owned()];
        assert!(!managed_service_request_can_stream(&ctx));

        ctx.guardrail_policy.mandatory_guardrails.clear();
        ctx.request_content_type = Some("application/json".to_owned());
        assert!(!managed_service_request_can_stream(&ctx));

        ctx.request_content_type = Some("application/octet-stream".to_owned());
        ctx.policy = None;
        assert!(!managed_service_request_can_stream(&ctx));
    }

    #[test]
    fn proxy_config_rejects_zero_body_admission_limits() {
        let config = PingoraLiteLlmConfig::from_base_url("http://127.0.0.1:4000", "service-key")
            .expect("base config");
        assert_eq!(
            config
                .clone()
                .with_body_admission_limits(0, 1024)
                .unwrap_err(),
            GatewayError::InvalidConfiguration
        );
        assert_eq!(
            config.with_body_admission_limits(1, 0).unwrap_err(),
            GatewayError::InvalidConfiguration
        );
    }

    #[test]
    fn response_buffer_reservation_uses_content_length_or_full_unknown_budget() {
        let mut known = ResponseHeader::build(200, Some(1)).expect("known response");
        known
            .insert_header(header::CONTENT_LENGTH, "128")
            .expect("content length");
        let unknown = ResponseHeader::build(200, None).expect("unknown response");

        assert_eq!(response_buffer_reservation_bytes(&known, 512), 128);
        assert_eq!(response_buffer_reservation_bytes(&unknown, 512), 512);
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

        let timeout = PingoraError::new_up(ErrorType::ReadTimedout);
        assert!(is_retry_safe_proxy_error(&timeout));
        assert!(is_upstream_timeout_error(&timeout));
        assert!(!proxy.activate_provider_fallback(&mut ctx));
    }

    #[tokio::test]
    async fn route_setting_blocks_disabled_canonical_litellm_routes_only() {
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
                .ensure_litellm_canonical_route_enabled(Route::ChatCompletions)
                .await
                .unwrap_err(),
            GatewayError::DisabledRoute
        );
        assert_eq!(
            proxy
                .ensure_litellm_canonical_route_enabled(Route::LiteLlmEmbeddings)
                .await
                .unwrap_err(),
            GatewayError::DisabledRoute
        );
        assert_eq!(
            proxy
                .ensure_litellm_canonical_route_enabled(Route::AnthropicMessages)
                .await
                .unwrap_err(),
            GatewayError::DisabledRoute
        );
        proxy
            .ensure_litellm_canonical_route_enabled(Route::ServiceWildcard)
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
    fn configured_litellm_limits_apply_to_openai_route_match() {
        let mut matched =
            Route::resolve_match(&http::Method::POST, "/v1/responses").expect("route");

        apply_litellm_limits_to_match(
            &mut matched,
            gateway_core::LiteLlmRouteLimits {
                timeout_ms: 240_000,
                max_request_body_bytes: 8_388_608,
                max_response_body_bytes: 4_194_304,
            },
        )
        .expect("limits apply");

        assert_eq!(matched.route, Route::Responses);
        assert_eq!(matched.timeout_ms, 240_000);
        assert_eq!(matched.max_body_bytes, 8_388_608);
        assert_eq!(matched.max_response_body_bytes, 4_194_304);
    }

    #[test]
    fn configured_litellm_limits_apply_to_anthropic_route_match() {
        let mut matched = Route::resolve_match(&http::Method::POST, "/v1/messages").expect("route");

        apply_litellm_limits_to_match(
            &mut matched,
            gateway_core::LiteLlmRouteLimits {
                timeout_ms: 180_000,
                max_request_body_bytes: 6_291_456,
                max_response_body_bytes: 3_145_728,
            },
        )
        .expect("limits apply");

        assert_eq!(matched.route, Route::AnthropicMessages);
        assert_eq!(matched.timeout_ms, 180_000);
        assert_eq!(matched.max_body_bytes, 6_291_456);
        assert_eq!(matched.max_response_body_bytes, 3_145_728);
    }

    #[test]
    fn passthrough_governance_bypass_helper_only_covers_wildcard_passthrough() {
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
        assert!(!sensitive_litellm_passthrough_authorized(
            Some(LiteLlmSensitiveRouteExposure::Disabled),
            None
        ));
        assert!(sensitive_litellm_passthrough_authorized(
            Some(LiteLlmSensitiveRouteExposure::ExplicitlyExposed),
            None
        ));
        assert!(sensitive_litellm_passthrough_authorized(
            Some(LiteLlmSensitiveRouteExposure::TrustedIngress),
            None
        ));

        let identity = EntraIdentityContext {
            tenant_id: "tenant".to_owned(),
            subject: Some("operator".to_owned()),
            object_id: Some("object".to_owned()),
            app_id: None,
            authorized_party: None,
            email: None,
            display_name: None,
            nonce: None,
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
        assert!(!sensitive_litellm_passthrough_authorized(
            Some(LiteLlmSensitiveRouteExposure::Disabled),
            Some(&identity)
        ));
    }

    #[test]
    fn trusted_ingress_rewrites_ui_redirects_to_public_origin() {
        assert_eq!(
            trusted_ingress_location("/ui/login", "http://gateway.test"),
            Some("http://gateway.test/ui/login".to_owned())
        );
        assert_eq!(
            trusted_ingress_location(
                "http://litellm:4000/ui/login?redirect_to=http%3A%2F%2Fgateway.test%2Fui%2F",
                "http://gateway.test"
            ),
            Some(
                "http://gateway.test/ui/login?redirect_to=http%3A%2F%2Fgateway.test%2Fui%2F"
                    .to_owned()
            )
        );
        assert_eq!(
            trusted_ingress_location("http://litellm:4000/key/list", "http://gateway.test"),
            None
        );
    }

    #[tokio::test]
    async fn trusted_ingress_litellm_upstream_uses_active_config_without_key() {
        let store = Arc::new(MemoryUsageStore::default());
        *store
            .active_litellm_config
            .lock()
            .expect("active config lock") = Some(gateway_core::ProviderRuntimeConfig {
            provider: Provider::LiteLlm,
            base_url: "http://litellm-config.internal:4010".to_owned(),
            credential: Some("provider-litellm-key".to_owned()),
            credential_header_mode: CredentialHeaderMode::CustomHeader,
            credential_header_name: Some("x-litellm-api-key".to_owned()),
            credential_header_value_format: CredentialHeaderValueFormat::Raw,
        });
        let proxy = RelaynaPingoraProxy {
            store,
            control_state: Arc::new(MemoryControlState::default()),
            config: PingoraLiteLlmConfig::from_base_url("http://127.0.0.1:4000", "fallback-key")
                .expect("config"),
            auth_runtime: default_auth_runtime_for_tests(),
        };
        let mut ctx = new_pingora_context_for_tests();

        proxy
            .configure_litellm_upstream(&mut ctx, None)
            .await
            .expect("configure upstream");

        let upstream = ctx.litellm_upstream.expect("active upstream");
        assert_eq!(upstream.host, "litellm-config.internal");
        assert_eq!(upstream.port, 4010);
        assert_eq!(upstream.service_key, "provider-litellm-key");
        assert_eq!(
            upstream.credential_header_mode,
            CredentialHeaderMode::CustomHeader
        );
        assert_eq!(
            upstream.credential_header_name.as_deref(),
            Some("x-litellm-api-key")
        );
    }

    #[tokio::test]
    async fn key_litellm_upstream_prefers_mapped_credential() {
        let store = Arc::new(MemoryUsageStore::default());
        *store
            .active_litellm_config
            .lock()
            .expect("active config lock") = Some(gateway_core::ProviderRuntimeConfig {
            provider: Provider::LiteLlm,
            base_url: "http://litellm-config.internal:4010".to_owned(),
            credential: Some("provider-litellm-key".to_owned()),
            credential_header_mode: CredentialHeaderMode::AuthorizationBearer,
            credential_header_name: None,
            credential_header_value_format: CredentialHeaderValueFormat::Raw,
        });
        *store
            .litellm_credential_mapping
            .lock()
            .expect("mapping lock") = Some(gateway_core::LiteLlmCredentialMappingRuntime {
            credential: "mapped-litellm-key".to_owned(),
        });
        let proxy = RelaynaPingoraProxy {
            store,
            control_state: Arc::new(MemoryControlState::default()),
            config: PingoraLiteLlmConfig::from_base_url("http://127.0.0.1:4000", "fallback-key")
                .expect("config"),
            auth_runtime: default_auth_runtime_for_tests(),
        };
        let key = AuthenticatedKey {
            key_id: Uuid::new_v4(),
            project_id: Some(Uuid::new_v4()),
            key_prefix: "rk_live_test_key".to_owned(),
        };
        let mut ctx = new_pingora_context_for_tests();

        proxy
            .configure_litellm_upstream(&mut ctx, Some(&key))
            .await
            .expect("configure upstream");

        let upstream = ctx.litellm_upstream.expect("active upstream");
        assert_eq!(upstream.service_key, "mapped-litellm-key");
        assert_eq!(
            upstream.credential_header_mode,
            CredentialHeaderMode::AuthorizationBearer
        );
    }

    #[tokio::test]
    async fn direct_litellm_upstream_uses_client_bearer_with_active_header_config() {
        let store = Arc::new(MemoryUsageStore::default());
        *store
            .active_litellm_config
            .lock()
            .expect("active config lock") = Some(gateway_core::ProviderRuntimeConfig {
            provider: Provider::LiteLlm,
            base_url: "http://litellm-config.internal:4010".to_owned(),
            credential: Some("provider-litellm-key".to_owned()),
            credential_header_mode: CredentialHeaderMode::CustomHeader,
            credential_header_name: Some("x-litellm-api-key".to_owned()),
            credential_header_value_format: CredentialHeaderValueFormat::Bearer,
        });
        let proxy = RelaynaPingoraProxy {
            store,
            control_state: Arc::new(MemoryControlState::default()),
            config: PingoraLiteLlmConfig::from_base_url("http://127.0.0.1:4000", "fallback-key")
                .expect("config"),
            auth_runtime: default_auth_runtime_for_tests(),
        };
        let mut ctx = new_pingora_context_for_tests();

        proxy
            .configure_litellm_upstream_with_credential(&mut ctx, "client-litellm-key".to_owned())
            .await
            .expect("configure upstream");

        let upstream = ctx.litellm_upstream.expect("active upstream");
        assert_eq!(upstream.host, "litellm-config.internal");
        assert_eq!(upstream.service_key, "client-litellm-key");
        assert_eq!(
            upstream.credential_header_mode,
            CredentialHeaderMode::CustomHeader
        );
        assert_eq!(
            upstream.credential_header_name.as_deref(),
            Some("x-litellm-api-key")
        );
        assert_eq!(
            upstream.credential_header_value_format,
            CredentialHeaderValueFormat::Bearer
        );

        let mut request = RequestHeader::build("POST", b"/v1/responses", None).expect("request");
        request
            .insert_header("authorization", "Bearer client-litellm-key")
            .expect("client authorization");
        prepare_upstream_authority_and_credentials(&mut request, &upstream, Some("x-relayna-key"))
            .expect("prepared upstream headers");
        assert!(!request.headers.contains_key("authorization"));
        assert_eq!(
            request
                .headers
                .get("x-litellm-api-key")
                .and_then(|value| value.to_str().ok()),
            Some("Bearer client-litellm-key")
        );
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
        active_litellm_config: Mutex<Option<gateway_core::ProviderRuntimeConfig>>,
        litellm_credential_mapping: Mutex<Option<gateway_core::LiteLlmCredentialMappingRuntime>>,
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
                active_litellm_config: Mutex::new(None),
                litellm_credential_mapping: Mutex::new(None),
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

        async fn openai_route_limits(
            &self,
            _route: Route,
        ) -> GatewayResult<gateway_core::LiteLlmRouteLimits> {
            Ok(gateway_core::LiteLlmRouteLimits::default())
        }

        async fn anthropic_route_enabled(&self, route: Route) -> GatewayResult<bool> {
            if gateway_core::anthropic_route_id(route).is_some() {
                Ok(*self.openai_routes_enabled.lock().expect("routes lock"))
            } else {
                Ok(true)
            }
        }

        async fn anthropic_route_mode(&self, route: Route) -> GatewayResult<OpenAiRouteMode> {
            if gateway_core::anthropic_route_id(route).is_some() {
                Ok(*self.openai_route_mode.lock().expect("route mode lock"))
            } else {
                Ok(OpenAiRouteMode::ManagedByGateway)
            }
        }

        async fn anthropic_route_limits(
            &self,
            _route: Route,
        ) -> GatewayResult<gateway_core::LiteLlmRouteLimits> {
            Ok(gateway_core::LiteLlmRouteLimits::default())
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
    impl ProviderConfigLookup for MemoryUsageStore {
        async fn active_litellm_config(
            &self,
        ) -> GatewayResult<Option<gateway_core::ProviderRuntimeConfig>> {
            Ok(self
                .active_litellm_config
                .lock()
                .expect("active litellm config lock")
                .clone())
        }

        async fn litellm_credential_mapping_for_context(
            &self,
            _key_id: Uuid,
            _project_id: Option<Uuid>,
        ) -> GatewayResult<Option<gateway_core::LiteLlmCredentialMappingRuntime>> {
            Ok(self
                .litellm_credential_mapping
                .lock()
                .expect("litellm mapping lock")
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
    async fn in_memory_proxy_dependencies_cover_their_complete_trait_contracts() {
        let store = MemoryUsageStore::default();
        assert!(store
            .list_provider_health_states()
            .await
            .unwrap()
            .is_empty());
        assert!(store
            .provider_health_check_targets()
            .await
            .unwrap()
            .is_empty());
        assert!(store
            .list_service_registry_snapshots()
            .await
            .unwrap()
            .is_empty());
        assert!(store.service_registry_snapshot(1).await.unwrap().is_none());
        assert!(store.get_debug_bundle("missing").await.unwrap().is_none());

        let mut ctx = new_pingora_context_for_tests();
        ctx.request_id = "dependency-contract".to_owned();
        let bundle = debug_bundle_for_ctx(&ctx, 500);
        store.insert_debug_bundle(bundle).await.unwrap();
        assert!(store
            .get_debug_bundle("dependency-contract")
            .await
            .unwrap()
            .is_some());

        assert!(store
            .openai_route_enabled(Route::ChatCompletions)
            .await
            .unwrap());
        assert!(store.openai_route_enabled(Route::Summary).await.unwrap());
        assert_eq!(
            store
                .openai_route_mode(Route::ChatCompletions)
                .await
                .unwrap(),
            OpenAiRouteMode::ManagedByGateway
        );
        assert_eq!(
            store.openai_route_mode(Route::Summary).await.unwrap(),
            OpenAiRouteMode::ManagedByGateway
        );
        store.openai_route_limits(Route::Responses).await.unwrap();
        assert!(store
            .anthropic_route_enabled(Route::AnthropicMessages)
            .await
            .unwrap());
        assert!(store.anthropic_route_enabled(Route::Summary).await.unwrap());
        assert_eq!(
            store
                .anthropic_route_mode(Route::AnthropicMessages)
                .await
                .unwrap(),
            OpenAiRouteMode::ManagedByGateway
        );
        assert_eq!(
            store.anthropic_route_mode(Route::Summary).await.unwrap(),
            OpenAiRouteMode::ManagedByGateway
        );
        store
            .anthropic_route_limits(Route::AnthropicMessages)
            .await
            .unwrap();
        store.litellm_passthrough_settings().await.unwrap();
        assert!(store.active_litellm_config().await.unwrap().is_none());
        assert!(store
            .litellm_credential_mapping_for_context(Uuid::new_v4(), None)
            .await
            .unwrap()
            .is_none());
        assert_eq!(store.list_guardrail_definitions().await.unwrap().len(), 1);
        store
            .guardrail_policy_for_key(Uuid::new_v4())
            .await
            .unwrap();
        store
            .upsert_guardrail_policy_for_key(Uuid::new_v4(), &GuardrailPolicy::default())
            .await
            .unwrap();

        let control = MemoryControlState::default();
        let key_id = Uuid::new_v4();
        let now = Utc::now();
        control
            .check_budget(key_id, Some(1.0), Some(10.0), now)
            .await
            .unwrap();
        control.add_budget_spend(key_id, 0.1, now).await.unwrap();
        control
            .reserve_budget(key_id, "request", 0.1, now)
            .await
            .unwrap();
        control
            .reconcile_budget_reservation(key_id, "request", 0.2, now)
            .await
            .unwrap();
        control
            .release_budget_reservation(key_id, "request")
            .await
            .unwrap();
        control
            .check_request_rate_limit(key_id, Some(10), now)
            .await
            .unwrap();
        control
            .check_token_rate_limit(key_id, Some(100), 10, now)
            .await
            .unwrap();
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
