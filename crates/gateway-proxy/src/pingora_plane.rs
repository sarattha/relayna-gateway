use async_trait::async_trait;
use bytes::Bytes;
use chrono::Utc;
use gateway_core::{
    auth::{Authenticator, VirtualKeyLookup},
    evaluate_policy, extract_generation_features, extract_model, AuthenticatedKey, BudgetDecision,
    BudgetStore, GatewayError, PolicyLookup, Provider, RateLimitDecision, RateLimitStore, Route,
    UsageEvent, UsageRecorder,
};
use pingora_core::{upstreams::peer::HttpPeer, Result as PingoraResult};
use pingora_http::{RequestHeader, ResponseHeader};
use pingora_proxy::{ProxyHttp, Session};
use std::{sync::Arc, time::Instant};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PingoraLiteLlmConfig {
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
    key: Option<AuthenticatedKey>,
    body_prefix: Vec<u8>,
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
            key: None,
            body_prefix: Vec::new(),
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

        let route = match Route::resolve(&req.method, req.uri.path()) {
            Ok(route) => route,
            Err(error) => {
                respond_error(session, error, &ctx.request_id).await?;
                return Ok(true);
            }
        };
        ctx.route = Some(route);

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
        _session: &mut Session,
        body: &mut Option<Bytes>,
        _end_of_stream: bool,
        ctx: &mut Self::CTX,
    ) -> PingoraResult<()>
    where
        Self::CTX: Send + Sync,
    {
        if let Some(body) = body {
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
        let Some(route) = ctx.route else {
            respond_error(session, GatewayError::UnsupportedRoute, &ctx.request_id).await?;
            return Ok(false);
        };
        let Some(key) = ctx.key.clone() else {
            respond_error(session, GatewayError::MissingAuthorization, &ctx.request_id).await?;
            return Ok(false);
        };

        let now = Utc::now();
        let features = extract_generation_features(&ctx.body_prefix);
        let policy = match self.store.policy_for_key(key.key_id).await {
            Ok(policy) => policy,
            Err(error) => {
                self.record_terminal_usage(ctx, &key, route, error.status_code().as_u16(), now)
                    .await;
                respond_error(session, error, &ctx.request_id).await?;
                return Ok(false);
            }
        };

        if let Err(error) = evaluate_policy(&policy, route, Provider::LiteLlm, &features) {
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
            Ok(BudgetDecision::Allowed(_)) => Ok(true),
            Ok(BudgetDecision::Exceeded(_)) => {
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
        _ctx: &mut Self::CTX,
    ) -> PingoraResult<Box<HttpPeer>> {
        let addr = format!("{}:{}", self.config.host, self.config.port);
        Ok(Box::new(HttpPeer::new(
            addr,
            self.config.tls,
            self.config.sni.clone(),
        )))
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
        upstream_request.insert_header(
            "authorization",
            format!("Bearer {}", self.config.service_key),
        )?;
        upstream_request.insert_header("x-relayna-request-id", &ctx.request_id)?;

        if let Some(key) = &ctx.key {
            upstream_request.insert_header("x-relayna-key-id", key.key_id.to_string())?;
            upstream_request.insert_header("x-relayna-project-id", key.project_id.to_string())?;
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
        let latency_ms = i64::try_from(ctx.started.elapsed().as_millis()).unwrap_or(i64::MAX);
        let event = UsageEvent::new(
            &ctx.request_id,
            key,
            route,
            extract_model(&ctx.body_prefix),
            status_code,
            latency_ms,
            Utc::now(),
        );
        let _ = self.store.insert_usage_event(&event).await;
        let _ = self
            .control_state
            .add_budget_spend(key.key_id, 0.0, Utc::now())
            .await;
    }
}

impl<S, R> RelaynaPingoraProxy<S, R>
where
    S: UsageRecorder,
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
        let event = UsageEvent::new(
            &ctx.request_id,
            key,
            route,
            extract_model(&ctx.body_prefix),
            status_code,
            latency_ms,
            now,
        );
        let _ = self.store.insert_usage_event(&event).await;
        ctx.terminal_usage_recorded = true;
    }
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

        assert_eq!(config.host, "litellm.internal");
        assert_eq!(config.port, 443);
        assert!(config.tls);
        assert_eq!(config.sni, "litellm.internal");
        assert_eq!(config.service_key, "service-key");
    }

    #[test]
    fn parses_http_litellm_base_url_for_pingora_peer() {
        let config = PingoraLiteLlmConfig::from_base_url("http://127.0.0.1:4000", "service-key")
            .expect("config");

        assert_eq!(config.host, "127.0.0.1");
        assert_eq!(config.port, 4000);
        assert!(!config.tls);
        assert_eq!(config.sni, "127.0.0.1");
    }
}
