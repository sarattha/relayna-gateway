use super::*;
use gateway_core::{EndpointEntraPolicy, GenerationFeatures};
use uuid::Uuid;

impl<S, R> RelaynaPingoraProxy<S, R> {
    pub(super) async fn verify_endpoint_identity(
        &self,
        req: &RequestHeader,
        now: chrono::DateTime<Utc>,
        auth: &GatewayAuthRuntimeSnapshot,
        request_id: &str,
        policy: &EndpointEntraPolicy,
    ) -> GatewayResult<EntraIdentityContext> {
        let context = EntraAuthDebugContext::new("endpoint", Some(request_id));
        if header_value(req, "x-apigee-entra-identity").is_some()
            || header_value(req, "x-apigee-entra-signature").is_some()
        {
            if !policy.allow_apigee {
                return Err(GatewayError::UntrustedApigeeIdentity);
            }
            let mut config = auth
                .config
                .apigee_trusted_header
                .clone()
                .ok_or(GatewayError::UntrustedApigeeIdentity)?;
            // Endpoint requirements replace global requirements, never form a union.
            config.required_scope = None;
            config.required_role = None;
            config.allowed_groups.clear();
            let identity = verify_apigee_trusted_identity_with_context(
                header_value(req, "x-apigee-entra-identity"),
                header_value(req, "x-apigee-entra-signature"),
                &config,
                context,
            )?;
            if auth
                .config
                .entra_auth
                .as_ref()
                .is_none_or(|config| config.tenant_id != identity.tenant_id)
            {
                return Err(GatewayError::InvalidEntraIssuer);
            }
            policy.authorize(&identity, now.timestamp())?;
            return Ok(identity);
        }
        let authorization =
            header_value(req, "authorization").ok_or(GatewayError::MissingEntraAuthorization)?;
        let token = authorization
            .strip_prefix("Bearer ")
            .ok_or(GatewayError::MalformedEntraAuthorization)?;
        auth.entra_verifier
            .as_ref()
            .ok_or(GatewayError::InvalidConfiguration)?
            .verify_token_for_endpoint(token.trim(), now, context, Some(policy))
            .await
    }
}

impl<S, R> RelaynaPingoraProxy<S, R>
where
    S: VirtualKeyLookup + PolicyLookup + ServiceRegistryLookup,
    R: AccessaStore + RateLimitStore + BudgetStore,
{
    async fn accessa_policy(
        &self,
        key: &AuthenticatedKey,
        service: &str,
        stream: bool,
    ) -> GatewayResult<KeyPolicy> {
        let effective = self
            .store
            .effective_policy_for_context(
                key.key_id,
                key.project_id,
                None,
                Some(Route::ServiceWildcard),
                None,
            )
            .await?;
        let policy = effective.policy;
        // An empty legacy allowlist must never grant Accessa access implicitly.
        if !policy.allowed_services.iter().any(|name| name == service) {
            return Err(GatewayError::PolicyDenied);
        }
        evaluate_policy(
            &policy,
            Route::ServiceWildcard,
            Provider::InternalService,
            &GenerationFeatures {
                service_name: Some(service.to_owned()),
                stream,
                ..Default::default()
            },
        )?;
        evaluate_policy_limits(&policy, Utc::now(), None, None, None, None, None)?;
        Ok(policy)
    }

    async fn accessa_budget(
        &self,
        key: &AuthenticatedKey,
        policy: &KeyPolicy,
    ) -> GatewayResult<()> {
        match self
            .control_state
            .check_budget(
                key.key_id,
                policy.daily_budget_usd,
                policy.monthly_budget_usd,
                Utc::now(),
            )
            .await?
        {
            BudgetDecision::Allowed(_) => Ok(()),
            BudgetDecision::Exceeded(_) => Err(GatewayError::BudgetExceeded),
        }
    }

    async fn accessa_rate(&self, key: &AuthenticatedKey, policy: &KeyPolicy) -> GatewayResult<()> {
        match self
            .control_state
            .check_request_rate_limit(key.key_id, policy.rpm_limit, Utc::now())
            .await?
        {
            RateLimitDecision::Allowed { .. } => Ok(()),
            RateLimitDecision::Exceeded {
                retry_after_seconds,
                ..
            } => Err(GatewayError::RateLimitExceeded {
                retry_after_seconds,
            }),
        }
    }

    pub(super) async fn admit_accessa_connection(
        &self,
        ctx: &mut PingoraContext,
        key: &AuthenticatedKey,
    ) -> GatewayResult<()> {
        let service = ctx
            .route_match
            .as_ref()
            .and_then(|route| route.service_name.clone())
            .ok_or(GatewayError::MissingService)?;
        let identity = ctx
            .entra_identity
            .clone()
            .ok_or(GatewayError::MissingEntraAuthorization)?;
        if identity.object_id.as_deref().is_none_or(str::is_empty) {
            return Err(GatewayError::InsufficientEntraAuthorization);
        }
        let policy = self.accessa_policy(key, &service, ctx.socket).await?;
        self.accessa_rate(key, &policy).await?;
        self.accessa_budget(key, &policy).await?;
        ctx.policy = Some(policy);
        let binding = ctx
            .access
            .accessa
            .as_ref()
            .ok_or(GatewayError::InvalidConfiguration)?;
        if ctx.socket {
            ctx.socket_lease = Some(self.connection_limits.acquire(
                &service,
                key.key_id,
                binding.max_connections,
                binding.max_connections_per_key,
            )?);
            ctx.socket_request_frames = Some(FrameLimit::new(binding.max_frame_bytes, true));
            ctx.socket_response_frames = Some(FrameLimit::new(binding.max_frame_bytes, false));
            ctx.is_streaming = true;
        }
        if binding.app.is_some() {
            let now = Utc::now().timestamp();
            let expires_at = identity
                .expires_at
                .ok_or(GatewayError::ExpiredEntraToken)?
                .min(now + 3600);
            if expires_at <= now {
                return Err(GatewayError::ExpiredEntraToken);
            }
            let snapshot = AccessaSession {
                key_id: key.key_id,
                key_prefix: key.key_prefix.clone(),
                service_name: service,
                identity,
                access: ctx.access.clone(),
                authentication_profile: ctx.traffic.diagnostics.authentication_profile.clone(),
                expires_at,
            };
            let token = Uuid::new_v4().to_string();
            let value =
                serde_json::to_string(&snapshot).map_err(|_| GatewayError::InvalidConfiguration)?;
            self.control_state
                .accessa_put(
                    &session_store_key(&token)?,
                    &value,
                    (expires_at - now) as u64,
                    false,
                )
                .await?;
            ctx.admission_token = Some(token);
            ctx.socket_expires_at = expires_at;
            if let Some(socket) = &mut ctx.traffic.diagnostics.websocket {
                socket.session_expires_at = chrono::DateTime::from_timestamp(expires_at, 0);
            }
        }
        Ok(())
    }

    async fn admit_turn(
        &self,
        req: &RequestHeader,
        ctx: &mut PingoraContext,
    ) -> GatewayResult<serde_json::Value> {
        if req.method != http::Method::POST
            || req.headers.contains_key("transfer-encoding")
            || header_value(req, "content-length").is_some_and(|v| v != "0")
        {
            return Err(GatewayError::InvalidServicePayload);
        }
        let turn = req
            .uri
            .path()
            .strip_prefix(ADMISSION_PREFIX)
            .and_then(|value| Uuid::parse_str(value).ok())
            .ok_or(GatewayError::InvalidServicePayload)?;
        let token = header_value(req, ADMISSION_CONTEXT_HEADER)
            .ok_or(GatewayError::MissingAuthorization)?;
        let session_key = session_store_key(token)?;
        let value = self
            .control_state
            .accessa_get(&session_key)
            .await?
            .ok_or(GatewayError::InvalidVirtualKey)?;
        let snapshot: AccessaSession =
            serde_json::from_str(&value).map_err(|_| GatewayError::ControlStateUnavailable)?;
        ctx.traffic.diagnostics.authentication_profile = snapshot.authentication_profile.clone();
        if let Some(profile) = &mut ctx.traffic.diagnostics.authentication_profile {
            profile.outcome = "credential_pending".into();
        }
        let now = Utc::now();
        if snapshot.expires_at <= now.timestamp() {
            return Err(GatewayError::ExpiredEntraToken);
        }
        let stored = self
            .store
            .find_by_prefix(&snapshot.key_prefix)
            .await?
            .ok_or(GatewayError::InvalidVirtualKey)?;
        if stored.id != snapshot.key_id {
            return Err(GatewayError::InvalidVirtualKey);
        }
        if stored.disabled {
            return Err(GatewayError::DisabledVirtualKey);
        }
        if stored.revoked_at.is_some() {
            return Err(GatewayError::RevokedVirtualKey);
        }
        if stored.expires_at.is_some_and(|expiry| expiry <= now) {
            return Err(GatewayError::ExpiredVirtualKey);
        }
        let key = AuthenticatedKey {
            key_id: stored.id,
            project_id: stored.project_id,
            key_prefix: stored.key_prefix,
        };
        ctx.key = Some(key.clone());
        ctx.route = Some(Route::ServiceWildcard);
        ctx.route_match = Some(RouteMatch::service(
            Route::ServiceWildcard,
            &snapshot.service_name,
        ));
        ctx.access = snapshot.access.clone();
        ctx.run_id = Some(turn.to_string());
        ctx.http_method = Some("POST".to_owned());
        ctx.endpoint_path = Some(format!("{ADMISSION_PREFIX}{turn}"));
        ctx.endpoint_template = Some(format!("{ADMISSION_PREFIX}{{turn_id}}"));
        let registration = self
            .store
            .service_registration(&snapshot.service_name)
            .await?
            .ok_or(GatewayError::MissingService)?;
        if !registration.enabled {
            return Err(GatewayError::DisabledService);
        }
        if let Some(profile) = &mut ctx.traffic.diagnostics.authentication_profile {
            profile.outcome = "selection_pending".into();
        }
        if registration.access != snapshot.access
            || registration
                .access
                .accessa
                .as_ref()
                .is_none_or(|binding| binding.app.is_none())
        {
            return Err(GatewayError::PolicyDenied);
        }
        snapshot
            .access
            .identity_for_key(snapshot.key_id)?
            .ok_or(GatewayError::InvalidConfiguration)?
            .authorize(&snapshot.identity, now.timestamp())?;
        if let Some(profile) = &mut ctx.traffic.diagnostics.authentication_profile {
            profile.outcome = "verified".into();
        }
        let policy = self
            .accessa_policy(&key, &snapshot.service_name, true)
            .await?;
        self.accessa_budget(&key, &policy).await?;
        let admission_key = format!("{session_key}:turn:{turn}");
        let ttl = (snapshot.expires_at - now.timestamp()) as u64;
        // One rate charge for repeated admission of a turn. Router owns durable run deduplication.
        if self
            .control_state
            .accessa_put(&admission_key, "pending", ttl, true)
            .await?
        {
            if let Err(error) = self.accessa_rate(&key, &policy).await {
                self.control_state.accessa_delete(&admission_key).await?;
                return Err(error);
            }
            self.control_state
                .accessa_put(&admission_key, "accepted", ttl, false)
                .await?;
        } else if self
            .control_state
            .accessa_get(&admission_key)
            .await?
            .as_deref()
            != Some("accepted")
        {
            return Err(GatewayError::RateLimitExceeded {
                retry_after_seconds: Some(1),
            });
        }
        Ok(
            serde_json::json!({"admission_id": turn, "key_id": key.key_id, "service": snapshot.service_name,
            "caller_oid": snapshot.identity.object_id, "app": snapshot.access.accessa.as_ref().and_then(|b| b.app.as_ref()),
            "channel": snapshot.access.accessa.as_ref().map(|b| &b.channel), "charged": false}),
        )
    }

    pub(super) async fn handle_accessa_admission(
        &self,
        session: &mut Session,
        ctx: &mut PingoraContext,
    ) -> PingoraResult<bool> {
        match self.admit_turn(session.req_header(), ctx).await {
            Ok(value) => {
                let body = Bytes::from(value.to_string());
                let mut response = ResponseHeader::build(200, None)?;
                response.insert_header("content-type", "application/json")?;
                response.insert_header("content-length", body.len().to_string())?;
                response.insert_header("x-request-id", &ctx.request_id)?;
                session
                    .write_response_header(Box::new(response), false)
                    .await?;
                session.write_response_body(Some(body), true).await?;
            }
            Err(error) => {
                respond_error(session, error, ctx).await?;
            }
        }
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gateway_core::{auth::StoredVirtualKey, ServiceRegistration};
    use serde_json::Value;
    use std::{collections::HashMap, sync::Mutex};

    struct Store {
        key: Mutex<Option<StoredVirtualKey>>,
        policy: Mutex<KeyPolicy>,
        registration: Mutex<Option<ServiceRegistration>>,
    }
    #[async_trait]
    impl VirtualKeyLookup for Store {
        async fn find_by_prefix(&self, _: &str) -> GatewayResult<Option<StoredVirtualKey>> {
            Ok(self.key.lock().unwrap().clone())
        }
    }
    #[async_trait]
    impl PolicyLookup for Store {
        async fn policy_for_key(&self, _: Uuid) -> GatewayResult<KeyPolicy> {
            Ok(self.policy.lock().unwrap().clone())
        }
    }
    #[async_trait]
    impl ServiceRegistryLookup for Store {
        async fn service_registration(
            &self,
            _: &str,
        ) -> GatewayResult<Option<ServiceRegistration>> {
            Ok(self.registration.lock().unwrap().clone())
        }
    }
    #[derive(Default)]
    struct Control {
        values: Mutex<HashMap<String, String>>,
        denied: Mutex<bool>,
        rate_denied: Mutex<bool>,
        rate_calls: Mutex<usize>,
    }
    #[async_trait]
    impl AccessaStore for Control {
        async fn accessa_get(&self, key: &str) -> GatewayResult<Option<String>> {
            Ok(self.values.lock().unwrap().get(key).cloned())
        }
        async fn accessa_put(
            &self,
            key: &str,
            value: &str,
            _: u64,
            nx: bool,
        ) -> GatewayResult<bool> {
            let mut values = self.values.lock().unwrap();
            if nx && values.contains_key(key) {
                return Ok(false);
            }
            values.insert(key.into(), value.into());
            Ok(true)
        }
        async fn accessa_delete(&self, key: &str) -> GatewayResult<()> {
            self.values.lock().unwrap().remove(key);
            Ok(())
        }
    }
    #[async_trait]
    impl RateLimitStore for Control {
        async fn check_request_rate_limit(
            &self,
            _: Uuid,
            _: Option<i32>,
            _: chrono::DateTime<Utc>,
        ) -> GatewayResult<RateLimitDecision> {
            *self.rate_calls.lock().unwrap() += 1;
            Ok(if *self.rate_denied.lock().unwrap() {
                RateLimitDecision::Exceeded {
                    count: 1,
                    retry_after_seconds: Some(1),
                }
            } else {
                RateLimitDecision::Allowed { count: 1 }
            })
        }
        async fn check_token_rate_limit(
            &self,
            _: Uuid,
            _: Option<i32>,
            _: i64,
            _: chrono::DateTime<Utc>,
        ) -> GatewayResult<RateLimitDecision> {
            unreachable!()
        }
    }
    #[async_trait]
    impl BudgetStore for Control {
        async fn check_budget(
            &self,
            _: Uuid,
            _: Option<f64>,
            _: Option<f64>,
            _: chrono::DateTime<Utc>,
        ) -> GatewayResult<BudgetDecision> {
            let state = gateway_core::BudgetState {
                daily_spend_usd: 0.0,
                monthly_spend_usd: 0.0,
            };
            Ok(if *self.denied.lock().unwrap() {
                BudgetDecision::Exceeded(state)
            } else {
                BudgetDecision::Allowed(state)
            })
        }
        async fn add_budget_spend(
            &self,
            _: Uuid,
            _: f64,
            _: chrono::DateTime<Utc>,
        ) -> GatewayResult<()> {
            panic!("admission must never charge")
        }
        async fn reserve_budget(
            &self,
            _: Uuid,
            _: &str,
            _: f64,
            _: chrono::DateTime<Utc>,
        ) -> GatewayResult<()> {
            panic!("admission must never reserve")
        }
        async fn reconcile_budget_reservation(
            &self,
            _: Uuid,
            _: &str,
            _: f64,
            _: chrono::DateTime<Utc>,
        ) -> GatewayResult<()> {
            panic!("admission must never reconcile")
        }
        async fn release_budget_reservation(&self, _: Uuid, _: &str) -> GatewayResult<()> {
            panic!("admission must never release")
        }
    }
    fn identity() -> EntraIdentityContext {
        serde_json::from_value(serde_json::json!({"tenant_id":"tenant","object_id":"user","scopes":["run"],"roles":[],"groups":[],"token_version":"2.0","source":"jwt","audiences":["accessa"],"expires_at":Utc::now().timestamp()+3600})).unwrap()
    }
    fn setup() -> (
        RelaynaPingoraProxy<Store, Control>,
        PingoraContext,
        AuthenticatedKey,
    ) {
        let access=serde_json::from_value(serde_json::json!({"entra":{"audience":"accessa","required_scopes":["run"]},"accessa":{"app":"tara","channel":"web","idle_timeout_ms":1000,"max_connections":2,"max_connections_per_key":1,"max_frame_bytes":1024}})).unwrap();
        let registration = ServiceRegistration {
            access,
            name: "accessa".into(),
            project_id: None,
            studio_service_id: None,
            route_pattern: "/app/tara/channel/web/v1/*".into(),
            upstream_base_url: Some("http://localhost".into()),
            health_check_path: None,
            health_check_method: "GET".into(),
            enabled: true,
            allowed_methods: vec!["GET".into(), "POST".into()],
            timeout_ms: 1000,
            max_body_bytes: 1024,
            cost_mode: ServiceCostMode::None,
            estimated_cost_usd: None,
            pricing_rules: vec![],
            openapi_source_path: None,
            openapi_schema_hash: None,
            openapi_synced_at: None,
            openapi_endpoints: vec![],
            endpoint_pricing_rules: vec![],
            credential_secret: Some("test".into()),
            fallback_services: vec![],
            source: gateway_core::ServiceSource::Gateway,
            sync_status: gateway_core::ServiceSyncStatus::Local,
            last_synced_at: None,
            disabled_at: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        let key = AuthenticatedKey {
            key_id: Uuid::new_v4(),
            project_id: None,
            key_prefix: "rk_live_test_key".into(),
        };
        let mut policy = KeyPolicy::neutral_layer(1);
        policy.allow_streaming = true;
        policy.allowed_services = vec!["accessa".into()];
        let mut ctx = new_pingora_context_for_tests();
        ctx.access = registration.access.clone();
        ctx.socket = true;
        ctx.entra_identity = Some(identity());
        ctx.route_match = Some(RouteMatch::service(Route::ServiceWildcard, "accessa"));
        let proxy = RelaynaPingoraProxy {
            store: Arc::new(Store {
                key: Mutex::new(Some(StoredVirtualKey {
                    id: key.key_id,
                    project_id: None,
                    key_prefix: key.key_prefix.clone(),
                    key_hash: String::new(),
                    disabled: false,
                    revoked_at: None,
                    expires_at: None,
                })),
                policy: Mutex::new(policy),
                registration: Mutex::new(Some(registration)),
            }),
            control_state: Arc::new(Control::default()),
            config: PingoraLiteLlmConfig::from_base_url("http://localhost", "test").unwrap(),
            auth_runtime: default_auth_runtime_for_tests(),
            connection_limits: Arc::new(ConnectionLimits::default()),
        };
        (proxy, ctx, key)
    }
    fn request(token: &str, turn: Uuid) -> RequestHeader {
        let mut req =
            RequestHeader::build("POST", format!("{ADMISSION_PREFIX}{turn}").as_bytes(), None)
                .unwrap();
        req.insert_header(ADMISSION_CONTEXT_HEADER, token).unwrap();
        req
    }
    #[tokio::test]
    async fn admission_rechecks_and_deduplicates_without_charging() {
        let (proxy, mut ctx, key) = setup();
        proxy
            .admit_accessa_connection(&mut ctx, &key)
            .await
            .unwrap();
        let token = ctx.admission_token.clone().unwrap();
        let turn = Uuid::new_v4();
        let req = request(&token, turn);
        let mut admission = new_pingora_context_for_tests();
        let first = proxy.admit_turn(&req, &mut admission).await.unwrap();
        assert_eq!(first["charged"], false);
        assert_eq!(first, proxy.admit_turn(&req, &mut admission).await.unwrap());
        assert_eq!(*proxy.control_state.rate_calls.lock().unwrap(), 2);
        type Mutation = fn(&Store);
        let attempts: Vec<(Mutation, GatewayError)> = vec![
            (
                |s| s.key.lock().unwrap().as_mut().unwrap().disabled = true,
                GatewayError::DisabledVirtualKey,
            ),
            (
                |s| s.key.lock().unwrap().as_mut().unwrap().revoked_at = Some(Utc::now()),
                GatewayError::RevokedVirtualKey,
            ),
            (
                |s| s.key.lock().unwrap().as_mut().unwrap().expires_at = Some(Utc::now()),
                GatewayError::ExpiredVirtualKey,
            ),
            (
                |s| s.key.lock().unwrap().as_mut().unwrap().id = Uuid::new_v4(),
                GatewayError::InvalidVirtualKey,
            ),
            (
                |s| *s.key.lock().unwrap() = None,
                GatewayError::InvalidVirtualKey,
            ),
            (
                |s| s.policy.lock().unwrap().deny = true,
                GatewayError::PolicyDenied,
            ),
            (
                |s| s.policy.lock().unwrap().allowed_services.clear(),
                GatewayError::PolicyDenied,
            ),
            (
                |s| {
                    s.policy.lock().unwrap().allowed_hours_utc =
                        vec![(chrono::Timelike::hour(&Utc::now()) as i32 + 1) % 24]
                },
                GatewayError::PolicyDenied,
            ),
            (
                |s| s.registration.lock().unwrap().as_mut().unwrap().enabled = false,
                GatewayError::DisabledService,
            ),
            (
                |s| {
                    s.registration
                        .lock()
                        .unwrap()
                        .as_mut()
                        .unwrap()
                        .access
                        .accessa
                        .as_mut()
                        .unwrap()
                        .channel = "other".into()
                },
                GatewayError::PolicyDenied,
            ),
            (
                |s| *s.registration.lock().unwrap() = None,
                GatewayError::MissingService,
            ),
        ];
        for (mutate, expected) in attempts {
            let (p, mut c, k) = setup();
            p.admit_accessa_connection(&mut c, &k).await.unwrap();
            let r = request(c.admission_token.as_ref().unwrap(), Uuid::new_v4());
            mutate(&p.store);
            assert_eq!(
                p.admit_turn(&r, &mut new_pingora_context_for_tests())
                    .await
                    .unwrap_err(),
                expected
            );
        }
        *proxy.control_state.denied.lock().unwrap() = true;
        assert_eq!(
            proxy.admit_turn(&req, &mut admission).await.unwrap_err(),
            GatewayError::BudgetExceeded
        );
        *proxy.control_state.denied.lock().unwrap() = false;
        *proxy.control_state.rate_denied.lock().unwrap() = true;
        let other = request(&token, Uuid::new_v4());
        assert!(matches!(
            proxy.admit_turn(&other, &mut admission).await,
            Err(GatewayError::RateLimitExceeded { .. })
        ));
        *proxy.control_state.rate_denied.lock().unwrap() = false;
        assert!(proxy.admit_turn(&other, &mut admission).await.is_ok());
        let pending = Uuid::new_v4();
        proxy
            .control_state
            .accessa_put(
                &format!("{}:turn:{pending}", session_store_key(&token).unwrap()),
                "pending",
                60,
                false,
            )
            .await
            .unwrap();
        assert!(matches!(
            proxy
                .admit_turn(&request(&token, pending), &mut admission)
                .await,
            Err(GatewayError::RateLimitExceeded { .. })
        ));
    }
    #[tokio::test]
    async fn malformed_stale_contexts_fail_closed() {
        let (proxy, mut ctx, key) = setup();
        proxy
            .admit_accessa_connection(&mut ctx, &key)
            .await
            .unwrap();
        let token = ctx.admission_token.unwrap();
        let sk = session_store_key(&token).unwrap();
        let original = proxy.control_state.accessa_get(&sk).await.unwrap().unwrap();
        for (value, error) in [
            ("broken".to_owned(), GatewayError::ControlStateUnavailable),
            (
                original.replace(
                    &format!(
                        "\"expires_at\":{}",
                        serde_json::from_str::<Value>(&original).unwrap()["expires_at"]
                    ),
                    "\"expires_at\":0",
                ),
                GatewayError::ExpiredEntraToken,
            ),
        ] {
            proxy
                .control_state
                .accessa_put(&sk, &value, 60, false)
                .await
                .unwrap();
            assert_eq!(
                proxy
                    .admit_turn(
                        &request(&token, Uuid::new_v4()),
                        &mut new_pingora_context_for_tests()
                    )
                    .await
                    .unwrap_err(),
                error
            );
        }
        proxy.control_state.accessa_delete(&sk).await.unwrap();
        assert_eq!(
            proxy
                .admit_turn(
                    &request(&token, Uuid::new_v4()),
                    &mut new_pingora_context_for_tests()
                )
                .await
                .unwrap_err(),
            GatewayError::InvalidVirtualKey
        );
        let mut req = request("bad", Uuid::new_v4());
        assert_eq!(
            proxy
                .admit_turn(&req, &mut new_pingora_context_for_tests())
                .await
                .unwrap_err(),
            GatewayError::InvalidVirtualKey
        );
        req.remove_header(ADMISSION_CONTEXT_HEADER);
        assert_eq!(
            proxy
                .admit_turn(&req, &mut new_pingora_context_for_tests())
                .await
                .unwrap_err(),
            GatewayError::MissingAuthorization
        );
        for (method, path, header) in [
            ("GET", format!("{ADMISSION_PREFIX}{}", Uuid::new_v4()), None),
            ("POST", format!("{ADMISSION_PREFIX}bad"), None),
            (
                "POST",
                format!("{ADMISSION_PREFIX}{}", Uuid::new_v4()),
                Some(("content-length", "10")),
            ),
            (
                "POST",
                format!("{ADMISSION_PREFIX}{}", Uuid::new_v4()),
                Some(("transfer-encoding", "chunked")),
            ),
        ] {
            let mut req = RequestHeader::build(method, path.as_bytes(), None).unwrap();
            if let Some((name, value)) = header {
                req.insert_header(name, value).unwrap();
            }
            assert!(proxy
                .admit_turn(&req, &mut new_pingora_context_for_tests())
                .await
                .is_err());
        }
    }
    #[tokio::test]
    async fn connection_requires_user_policy_budget_and_valid_expiry() {
        let (proxy, mut ctx, key) = setup();
        ctx.entra_identity = None;
        assert!(proxy
            .admit_accessa_connection(&mut ctx, &key)
            .await
            .is_err());
        ctx.entra_identity = Some(identity());
        ctx.entra_identity.as_mut().unwrap().object_id = None;
        assert!(proxy
            .admit_accessa_connection(&mut ctx, &key)
            .await
            .is_err());
        ctx.entra_identity = Some(identity());
        ctx.entra_identity.as_mut().unwrap().expires_at = None;
        assert!(proxy
            .admit_accessa_connection(&mut ctx, &key)
            .await
            .is_err());
        ctx.socket_lease.take();
        ctx.entra_identity = Some(identity());
        ctx.entra_identity.as_mut().unwrap().expires_at = Some(0);
        assert!(proxy
            .admit_accessa_connection(&mut ctx, &key)
            .await
            .is_err());
        ctx.socket_lease.take();
        ctx.entra_identity = Some(identity());
        ctx.socket = false;
        ctx.access.accessa.as_mut().unwrap().app = None;
        proxy
            .admit_accessa_connection(&mut ctx, &key)
            .await
            .unwrap();
        assert!(ctx.admission_token.is_none());
    }
}

#[cfg(test)]
mod endpoint_tests {
    use super::*;
    fn signed(identity: &EntraIdentityContext) -> (String, String) {
        use base64::Engine;
        let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(serde_json::to_vec(identity).unwrap());
        let config = gateway_core::ApigeeTrustedHeaderConfig {
            secret: "test-secret".into(),
            required_scope: None,
            required_role: None,
            allowed_groups: vec![],
        };
        let signature = gateway_core::sign_apigee_trusted_identity(&payload, &config).unwrap();
        (payload, signature)
    }
    #[tokio::test]
    async fn endpoint_apigee_requires_signed_audience_expiry_and_permissions() {
        let config = gateway_core::EntraAuthConfig {
            tenant_id: "tenant".into(),
            audience: "global".into(),
            issuer: "https://issuer.test".into(),
            oidc_discovery_url: "http://127.0.0.1:1".into(),
            required_scope: Some("global".into()),
            required_role: None,
            allowed_groups: vec![],
            accepted_algorithms: vec!["RS256".into()],
            relayna_key_header: "x-relayna-key".into(),
            jwks_cache_ttl_seconds: 300,
            clock_skew_seconds: 0,
        };
        let auth = SharedGatewayAuthRuntime::new(GatewayAuthRuntimeConfig {
            unverified_bearer_enabled: false,
            relayna_key_header: "x-relayna-key".into(),
            entra_auth: Some(config.clone()),
            apigee_trusted_header: Some(gateway_core::ApigeeTrustedHeaderConfig {
                secret: "test-secret".into(),
                required_scope: Some("global".into()),
                required_role: Some("global".into()),
                allowed_groups: vec!["global".into()],
            }),
        })
        .unwrap();
        let proxy = RelaynaPingoraProxy {
            store: Arc::new(()),
            control_state: Arc::new(()),
            config: PingoraLiteLlmConfig::from_base_url("http://localhost", "test").unwrap(),
            auth_runtime: auth.clone(),
            connection_limits: Arc::new(ConnectionLimits::default()),
        };
        let mut policy = EndpointEntraPolicy {
            audience: "internal".into(),
            required_scopes: vec!["internal.invoke".into()],
            required_roles: vec![],
            allowed_groups: vec![],
            allow_apigee: true,
        };
        let identity:EntraIdentityContext=serde_json::from_value(serde_json::json!({"tenant_id":"tenant","object_id":"employee","audiences":["internal"],"expires_at":Utc::now().timestamp()+60,"scopes":["internal.invoke"],"roles":[],"groups":[],"source":"jwt","token_version":"2.0"})).unwrap();
        let mut req = RequestHeader::build("GET", b"/internal-service", None).unwrap();
        let snapshot = auth.snapshot().unwrap();
        assert_eq!(
            proxy
                .verify_endpoint_identity(&req, Utc::now(), &snapshot, "request", &policy)
                .await
                .unwrap_err(),
            GatewayError::MissingEntraAuthorization
        );
        req.insert_header("authorization", "Basic invalid").unwrap();
        assert_eq!(
            proxy
                .verify_endpoint_identity(&req, Utc::now(), &snapshot, "request", &policy)
                .await
                .unwrap_err(),
            GatewayError::MalformedEntraAuthorization
        );
        req.insert_header("authorization", "Bearer malformed")
            .unwrap();
        assert!(proxy
            .verify_endpoint_identity(&req, Utc::now(), &snapshot, "request", &policy)
            .await
            .is_err());
        for (field, value, error) in [
            (
                "audiences",
                serde_json::json!(["accessa"]),
                GatewayError::InvalidEntraAudience,
            ),
            (
                "expires_at",
                serde_json::json!(null),
                GatewayError::ExpiredEntraToken,
            ),
            (
                "scopes",
                serde_json::json!([]),
                GatewayError::InsufficientEntraAuthorization,
            ),
            (
                "tenant_id",
                serde_json::json!("other"),
                GatewayError::InvalidEntraIssuer,
            ),
        ] {
            let mut value_identity = serde_json::to_value(&identity).unwrap();
            value_identity[field] = value;
            let modified = serde_json::from_value(value_identity).unwrap();
            let (payload, signature) = signed(&modified);
            req.insert_header("x-apigee-entra-identity", payload)
                .unwrap();
            req.insert_header("x-apigee-entra-signature", signature)
                .unwrap();
            assert_eq!(
                proxy
                    .verify_endpoint_identity(&req, Utc::now(), &snapshot, "request", &policy)
                    .await
                    .unwrap_err(),
                error
            );
        }
        let (payload, signature) = signed(&identity);
        req.insert_header("x-apigee-entra-identity", payload)
            .unwrap();
        req.insert_header("x-apigee-entra-signature", signature)
            .unwrap();
        assert!(proxy
            .verify_endpoint_identity(&req, Utc::now(), &snapshot, "request", &policy)
            .await
            .is_ok());
        policy.allow_apigee = false;
        assert_eq!(
            proxy
                .verify_endpoint_identity(&req, Utc::now(), &snapshot, "request", &policy)
                .await
                .unwrap_err(),
            GatewayError::UntrustedApigeeIdentity
        );
        policy.allow_apigee = true;
        req.insert_header("x-apigee-entra-signature", "forged")
            .unwrap();
        assert_eq!(
            proxy
                .verify_endpoint_identity(&req, Utc::now(), &snapshot, "request", &policy)
                .await
                .unwrap_err(),
            GatewayError::UntrustedApigeeIdentity
        );
        let missing = SharedGatewayAuthRuntime::new(Default::default())
            .unwrap()
            .snapshot()
            .unwrap();
        assert!(proxy
            .verify_endpoint_identity(&req, Utc::now(), &missing, "request", &policy)
            .await
            .is_err());
        req.remove_header("x-apigee-entra-identity");
        req.remove_header("x-apigee-entra-signature");
        assert_eq!(
            proxy
                .verify_endpoint_identity(&req, Utc::now(), &missing, "request", &policy)
                .await
                .unwrap_err(),
            GatewayError::InvalidConfiguration
        );
    }
}
