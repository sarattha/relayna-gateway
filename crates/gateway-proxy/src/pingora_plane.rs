use async_trait::async_trait;
use bytes::Bytes;
use chrono::Utc;
use gateway_core::{
    auth::{Authenticator, VirtualKeyLookup},
    evaluate_policy, extract_estimated_cost_usd, extract_generation_features, extract_model,
    extract_usage_tokens, AuthenticatedKey, BudgetDecision, BudgetStore, GatewayError,
    PolicyLookup, Provider, RateLimitDecision, RateLimitStore, Route, RouteMatch, UsageEvent,
    UsageRecorder,
};
use pingora_core::{upstreams::peer::HttpPeer, Result as PingoraResult};
use pingora_http::{RequestHeader, ResponseHeader};
use pingora_proxy::{ProxyHttp, Session};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PingoraLiteLlmConfig {
    pub litellm: PingoraUpstreamConfig,
    pub direct_openai: Option<PingoraUpstreamConfig>,
    pub internal_service: Option<PingoraUpstreamConfig>,
    pub worker_token: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PingoraUpstreamConfig {
    pub host: String,
    pub port: u16,
    pub tls: bool,
    pub sni: String,
    pub service_key: String,
}

impl PingoraLiteLlmConfig {
    pub fn from_base_url(
        base_url: impl AsRef<str>,
        service_key: impl Into<String>,
    ) -> gateway_core::GatewayResult<Self> {
        Ok(Self {
            litellm: PingoraUpstreamConfig::from_base_url(base_url, service_key)?,
            direct_openai: None,
            internal_service: None,
            worker_token: None,
        })
    }

    pub fn with_direct_openai(mut self, upstream: Option<PingoraUpstreamConfig>) -> Self {
        self.direct_openai = upstream;
        self
    }

    pub fn with_internal_service(mut self, upstream: Option<PingoraUpstreamConfig>) -> Self {
        self.internal_service = upstream;
        self
    }

    pub fn with_worker_token(mut self, worker_token: Option<String>) -> Self {
        self.worker_token = worker_token;
        self
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
        })
    }
}

pub struct RelaynaPingoraProxy<S, R> {
    store: Arc<S>,
    control_state: Arc<R>,
    config: PingoraLiteLlmConfig,
}

impl<S, R> RelaynaPingoraProxy<S, R>
where
    S: VirtualKeyLookup + UsageRecorder + PolicyLookup,
    R: RateLimitStore + BudgetStore,
{
    pub fn new(store: Arc<S>, control_state: Arc<R>, config: PingoraLiteLlmConfig) -> Self {
        Self {
            store,
            control_state,
            config,
        }
    }
}

#[derive(Debug)]
pub struct PingoraContext {
    started: Instant,
    request_id: String,
    route: Option<Route>,
    route_match: Option<RouteMatch>,
    key: Option<AuthenticatedKey>,
    body_prefix: Vec<u8>,
    body_bytes_seen: usize,
    response_body_prefix: Vec<u8>,
    is_streaming: bool,
    first_chunk_recorded: bool,
    budget_reserved: bool,
    task_id: Option<String>,
    run_id: Option<String>,
    traceparent: Option<String>,
    terminal_usage_recorded: bool,
}

#[async_trait]
impl<S, R> ProxyHttp for RelaynaPingoraProxy<S, R>
where
    S: VirtualKeyLookup + UsageRecorder + PolicyLookup + Send + Sync + 'static,
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
            body_prefix: Vec::new(),
            body_bytes_seen: 0,
            response_body_prefix: Vec::new(),
            is_streaming: false,
            first_chunk_recorded: false,
            budget_reserved: false,
            task_id: None,
            run_id: None,
            traceparent: None,
            terminal_usage_recorded: false,
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

        let matched = match Route::resolve_match(&req.method, req.uri.path()) {
            Ok(matched) => matched,
            Err(error) => {
                respond_error(session, error, &ctx.request_id).await?;
                return Ok(true);
            }
        };
        ctx.route = Some(matched.route);
        ctx.route_match = Some(matched);
        ctx.traceparent = header_value(req, "traceparent")
            .filter(|value| is_valid_traceparent(value))
            .map(ToOwned::to_owned);
        if self.trusted_worker(req) {
            ctx.task_id = header_value(req, "x-relayna-task-id").map(ToOwned::to_owned);
            ctx.run_id = header_value(req, "x-relayna-run-id").map(ToOwned::to_owned);
        }

        let authorization = req
            .headers
            .get("authorization")
            .and_then(|value| value.to_str().ok());
        match Authenticator::new(self.store.clone())
            .authenticate_authorization(authorization, Utc::now())
            .await
        {
            Ok(key) => {
                ctx.key = Some(key);
                Ok(false)
            }
            Err(error) => {
                respond_error(session, error, &ctx.request_id).await?;
                Ok(true)
            }
        }
    }

    async fn request_body_filter(
        &self,
        session: &mut Session,
        body: &mut Option<Bytes>,
        _end_of_stream: bool,
        ctx: &mut Self::CTX,
    ) -> PingoraResult<()>
    where
        Self::CTX: Send + Sync,
    {
        if let Some(body) = body {
            ctx.body_bytes_seen = ctx.body_bytes_seen.saturating_add(body.len());
            if let Some(matched) = &ctx.route_match {
                if ctx.body_bytes_seen > matched.max_body_bytes {
                    respond_error(session, GatewayError::RequestBodyTooLarge, &ctx.request_id)
                        .await?;
                }
            }
            if ctx.body_prefix.len() < 65_536 {
                let remaining = 65_536 - ctx.body_prefix.len();
                ctx.body_prefix
                    .extend_from_slice(&body[..body.len().min(remaining)]);
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

        let now = Utc::now();
        let mut features = extract_generation_features(&ctx.body_prefix);
        if features.service_name.is_none() {
            features.service_name = matched.service_name.clone();
        }
        ctx.is_streaming = features.stream;
        let policy = match self.store.policy_for_key(key.key_id).await {
            Ok(policy) => policy,
            Err(error) => {
                self.record_terminal_usage(ctx, &key, route, error.status_code().as_u16(), now)
                    .await;
                respond_error(session, error, &ctx.request_id).await?;
                return Ok(false);
            }
        };

        if let Err(error) = evaluate_policy(&policy, route, matched.provider, &features) {
            self.record_terminal_usage(ctx, &key, route, error.status_code().as_u16(), now)
                .await;
            respond_error(session, error, &ctx.request_id).await?;
            return Ok(false);
        }

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
                gateway_telemetry::record_rate_limit_rejection();
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
            .check_budget(
                key.key_id,
                policy.daily_budget_usd,
                policy.monthly_budget_usd,
                now,
            )
            .await
        {
            Ok(BudgetDecision::Allowed(_)) => {
                if ctx.is_streaming {
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
                        gateway_telemetry::stream_started();
                    }
                }
                Ok(true)
            }
            Ok(BudgetDecision::Exceeded(_)) => {
                gateway_telemetry::record_budget_rejection();
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
        upstream_request.remove_header("authorization");
        upstream_request.remove_header("host");
        upstream_request.remove_header("proxy-authorization");
        upstream_request.remove_header("x-api-key");
        upstream_request.remove_header("x-relayna-worker-token");
        let upstream = self.upstream_for(ctx).unwrap_or(&self.config.litellm);
        upstream_request
            .insert_header("authorization", format!("Bearer {}", upstream.service_key))?;
        upstream_request.insert_header("x-relayna-request-id", &ctx.request_id)?;
        if let Some(traceparent) = &ctx.traceparent {
            upstream_request.insert_header("traceparent", traceparent)?;
        }

        if let Some(key) = &ctx.key {
            upstream_request.insert_header("x-relayna-key-id", key.key_id.to_string())?;
            upstream_request.insert_header("x-relayna-project-id", key.project_id.to_string())?;
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

        Ok(())
    }

    async fn response_filter(
        &self,
        _session: &mut Session,
        upstream_response: &mut ResponseHeader,
        _ctx: &mut Self::CTX,
    ) -> PingoraResult<()>
    where
        Self::CTX: Send + Sync,
    {
        upstream_response.remove_header("alt-svc");
        Ok(())
    }

    fn response_body_filter(
        &self,
        _session: &mut Session,
        body: &mut Option<Bytes>,
        _end_of_stream: bool,
        ctx: &mut Self::CTX,
    ) -> PingoraResult<Option<std::time::Duration>>
    where
        Self::CTX: Send + Sync,
    {
        if let Some(body) = body {
            if ctx.is_streaming && !ctx.first_chunk_recorded {
                ctx.first_chunk_recorded = true;
                let latency_ms =
                    u64::try_from(ctx.started.elapsed().as_millis()).unwrap_or(u64::MAX);
                gateway_telemetry::record_first_token_latency_ms(latency_ms);
            }
            if ctx.response_body_prefix.len() < 65_536 {
                let remaining = 65_536 - ctx.response_body_prefix.len();
                ctx.response_body_prefix
                    .extend_from_slice(&body[..body.len().min(remaining)]);
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
        let estimated_cost_usd = extract_estimated_cost_usd(&ctx.response_body_prefix);
        let (input_tokens, output_tokens, total_tokens) =
            extract_usage_tokens(&ctx.response_body_prefix);
        let latency_ms = i64::try_from(ctx.started.elapsed().as_millis()).unwrap_or(i64::MAX);
        let provider = ctx
            .route_match
            .as_ref()
            .map(|matched| matched.provider)
            .unwrap_or(Provider::LiteLlm);
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
        .with_task_context(ctx.task_id.clone(), ctx.run_id.clone());
        let _ = self.store.insert_usage_event(&event).await;
        gateway_telemetry::record_request(status_code);
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
        if ctx.is_streaming {
            gateway_telemetry::stream_finished(error.is_some() || status_code >= 500);
        }
    }
}

impl<S, R> RelaynaPingoraProxy<S, R>
where
    S: UsageRecorder,
    R: BudgetStore,
{
    fn upstream_for<'a>(&'a self, ctx: &PingoraContext) -> Option<&'a PingoraUpstreamConfig> {
        let Some(matched) = &ctx.route_match else {
            return Some(&self.config.litellm);
        };
        match matched.provider {
            Provider::LiteLlm => Some(&self.config.litellm),
            Provider::OpenAiCompatible => self.config.direct_openai.as_ref(),
            Provider::InternalService => self.config.internal_service.as_ref(),
        }
    }

    fn trusted_worker(&self, req: &RequestHeader) -> bool {
        let Some(expected) = self.config.worker_token.as_deref() else {
            return false;
        };
        header_value(req, "x-relayna-worker-token").is_some_and(|actual| actual == expected)
    }

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
        let provider = ctx
            .route_match
            .as_ref()
            .map(|matched| matched.provider)
            .unwrap_or(Provider::LiteLlm);
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
        .with_service_name(
            ctx.route_match
                .as_ref()
                .and_then(|matched| matched.service_name.clone()),
        )
        .with_task_context(ctx.task_id.clone(), ctx.run_id.clone());
        let _ = self.store.insert_usage_event(&event).await;
        gateway_telemetry::record_request(status_code);
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
}
