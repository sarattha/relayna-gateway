//! General transport admission. Application events and LiteLLM billing stay upstream.
use super::*;
use base64::{engine::general_purpose::STANDARD, Engine};
use sha2::{Digest, Sha256};

pub(super) fn upgrade_requested(req: &RequestHeader) -> bool {
    req.headers.contains_key("upgrade") || req.headers.contains_key("sec-websocket-key")
}

pub(super) fn valid_handshake(req: &RequestHeader) -> bool {
    req.method == http::Method::GET
        && req.version == http::Version::HTTP_11
        && [
            "upgrade",
            "sec-websocket-key",
            "sec-websocket-version",
            "authorization",
        ]
        .iter()
        .all(|name| req.headers.get_all(*name).iter().count() <= 1)
        && header_value(req, "upgrade").is_some_and(|v| v.eq_ignore_ascii_case("websocket"))
        && req.headers.get_all("connection").iter().any(|value| {
            value.to_str().is_ok_and(|v| {
                v.split(',')
                    .any(|v| v.trim().eq_ignore_ascii_case("upgrade"))
            })
        })
        && header_value(req, "sec-websocket-version") == Some("13")
        && header_value(req, "sec-websocket-key")
            .and_then(|v| STANDARD.decode(v).ok())
            .is_some_and(|v| v.len() == 16)
        && !req.headers.contains_key("transfer-encoding")
        && req
            .headers
            .get_all("content-length")
            .iter()
            .all(|v| v == "0")
}

impl<S, R> RelaynaPingoraProxy<S, R>
where
    S: VirtualKeyLookup
        + PolicyLookup
        + UsageRecorder
        + GuardrailStore
        + ProviderIntelligenceStore
        + OpenAiRouteSettingsLookup,
    R: RateLimitStore + BudgetStore,
{
    pub(super) async fn admit_websocket(
        &self,
        session: &mut Session,
        ctx: &mut PingoraContext,
    ) -> GatewayResult<()> {
        let settings = ctx
            .access
            .websocket
            .as_ref()
            .ok_or(GatewayError::UnsupportedRoute)?
            .clone();
        settings.validate()?;
        let route = ctx
            .route_match
            .as_ref()
            .ok_or(GatewayError::UnsupportedRoute)?
            .clone();
        let now = Utc::now();
        let mut expires_at = now.timestamp() + settings.max_session_duration_seconds as i64;
        let (scope, identity) = if ctx.direct_litellm_passthrough {
            self.ensure_litellm_canonical_route_enabled(route.route)
                .await?;
            // A bounded, process-local bucket identity, never a Relayna key ID or
            // persisted credential. Do not retain raw caller keys in diagnostics.
            let credential = &ctx
                .litellm_upstream
                .as_ref()
                .ok_or(GatewayError::InvalidConfiguration)?
                .service_key;
            let digest = Sha256::digest(credential.as_bytes());
            let identity = uuid::Uuid::from_slice(&digest[..16])
                .map_err(|_| GatewayError::InvalidConfiguration)?;
            (format!("litellm:{}", route.route.as_str()), identity)
        } else {
            if route.provider != Provider::InternalService {
                return Err(GatewayError::UnsupportedRoute);
            }
            let key = ctx.key.clone().ok_or(GatewayError::MissingAuthorization)?;
            let service = route
                .service_name
                .as_ref()
                .ok_or(GatewayError::MissingService)?;
            let effective = self
                .store
                .effective_policy_for_context(
                    key.key_id,
                    key.project_id,
                    None,
                    Some(route.route),
                    None,
                )
                .await?;
            let policy = effective.policy;
            evaluate_policy(
                &policy,
                route.route,
                route.provider,
                &gateway_core::GenerationFeatures {
                    service_name: Some(service.clone()),
                    stream: true,
                    ..Default::default()
                },
            )?;
            evaluate_policy_limits(&policy, now, None, None, None, None, Some(0.0))?;
            // HTTP content guardrails cannot inspect opaque socket messages.
            for mode in [
                GuardrailMode::PreCall,
                GuardrailMode::PostCall,
                GuardrailMode::DuringCall,
            ] {
                let plan = resolve_guardrail_plan(GuardrailPlanRequest {
                    mode,
                    definitions: ctx.guardrail_definitions.clone(),
                    policies: GuardrailPolicySet {
                        key_policy: effective.guardrail_policy.clone(),
                        ..Default::default()
                    },
                    client_requested_guardrails: Vec::new(),
                })?;
                if !plan.entries.is_empty() {
                    return Err(GatewayError::GuardrailUnavailable);
                }
            }
            if let Some(seconds) = policy.max_stream_duration_seconds {
                expires_at = expires_at.min(now.timestamp() + i64::from(seconds));
            }
            if let Some(expiry) = ctx
                .entra_identity
                .as_ref()
                .and_then(|identity| identity.expires_at)
            {
                expires_at = expires_at.min(expiry);
            }
            if let Some(expiry) = self
                .store
                .find_by_prefix(&key.key_prefix)
                .await?
                .and_then(|stored| stored.expires_at)
            {
                expires_at = expires_at.min(expiry.timestamp());
            }
            match self
                .control_state
                .check_request_rate_limit(key.key_id, policy.rpm_limit, now)
                .await?
            {
                RateLimitDecision::Allowed { count } => ctx.request_rate_count = Some(count),
                RateLimitDecision::Exceeded {
                    retry_after_seconds,
                    ..
                } => {
                    return Err(GatewayError::RateLimitExceeded {
                        retry_after_seconds,
                    })
                }
            }
            match self
                .admit_request_budget(key.key_id, &ctx.reservation_id, 0.0, &policy, now)
                .await?
            {
                BudgetDecision::Allowed(_) => {
                    ctx.budget_reserved = true;
                    ctx.budget_reserved_at = Some(now);
                }
                BudgetDecision::Exceeded(_) => return Err(GatewayError::BudgetExceeded),
            }
            ctx.policy = Some(policy);
            (format!("service:{service}"), key.key_id)
        };
        if expires_at <= now.timestamp() {
            return Err(GatewayError::PolicyDenied);
        }
        ctx.socket_lease = Some(self.connection_limits.acquire(
            &scope,
            identity,
            settings.max_connections,
            settings.max_connections_per_key,
        )?);
        ctx.socket_request_frames = Some(FrameLimit::new(settings.max_frame_bytes, true));
        ctx.socket_response_frames = Some(FrameLimit::new(settings.max_frame_bytes, false));
        ctx.socket_expires_at = expires_at;
        ctx.is_streaming = true;
        if let Some(socket) = &mut ctx.traffic.diagnostics.websocket {
            socket.session_expires_at = chrono::DateTime::from_timestamp(expires_at, 0);
        }
        let idle = Some(Duration::from_millis(settings.idle_timeout_ms));
        session.as_downstream_mut().set_read_timeout(idle);
        session.as_downstream_mut().set_write_timeout(idle);
        gateway_telemetry::record_provider_selection();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn handshake() -> RequestHeader {
        let mut req = RequestHeader::build("GET", b"/socket", None).unwrap();
        for (name, value) in [
            ("connection", "keep-alive, Upgrade"),
            ("upgrade", "WebSocket"),
            ("sec-websocket-version", "13"),
            ("sec-websocket-key", "AAAAAAAAAAAAAAAAAAAAAA=="),
        ] {
            req.insert_header(name, value).unwrap();
        }
        req
    }

    #[test]
    fn handshake_rejects_ambiguous_or_body_carrying_upgrades() {
        assert!(valid_handshake(&handshake()));
        for (name, value) in [
            ("connection", "keep-alive"),
            ("upgrade", "h2c"),
            ("sec-websocket-version", "12"),
            ("sec-websocket-key", "not-a-key"),
            ("content-length", "1"),
            ("transfer-encoding", "chunked"),
        ] {
            let mut req = handshake();
            req.insert_header(name, value).unwrap();
            assert!(!valid_handshake(&req), "{name}");
        }
        let mut req = handshake();
        req.method = http::Method::POST;
        assert!(!valid_handshake(&req));
        req = handshake();
        req.version = http::Version::HTTP_2;
        assert!(!valid_handshake(&req));
        for name in [
            "upgrade",
            "sec-websocket-key",
            "sec-websocket-version",
            "authorization",
        ] {
            let mut req = handshake();
            req.append_header(name, "duplicate").unwrap();
            if name == "authorization" {
                req.append_header(name, "duplicate").unwrap();
            }
            assert!(!valid_handshake(&req));
        }
        let mut req = handshake();
        req.insert_header("sec-websocket-extensions", "permessage-deflate")
            .unwrap();
        assert!(valid_handshake(&req), "compression offers may be declined");
    }
}
