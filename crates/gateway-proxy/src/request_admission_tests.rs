use super::tests::{MemoryControlState, MemoryUsageStore};
use super::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

struct Harness {
    proxy: RelaynaPingoraProxy<MemoryUsageStore, MemoryControlState>,
    session: Session,
    client: tokio::net::TcpStream,
    ctx: PingoraContext,
}

impl Harness {
    async fn new(policy: KeyPolicy, direct: bool) -> Self {
        let store = Arc::new(MemoryUsageStore::default());
        *store.policy.lock().unwrap() = policy.clone();
        let proxy = RelaynaPingoraProxy::new(
            store,
            Arc::new(MemoryControlState::default()),
            PingoraLiteLlmConfig::from_base_url("http://127.0.0.1:4000", "test-key").unwrap(),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut client = tokio::net::TcpStream::connect(listener.local_addr().unwrap())
            .await
            .unwrap();
        let (server, _) = listener.accept().await.unwrap();
        client.write_all(b"POST /v1/chat/completions HTTP/1.1\r\nHost: localhost\r\nTransfer-Encoding: chunked\r\n\r\n").await.unwrap();
        let mut session = Session::new_h1(Box::new(
            pingora_core::protocols::l4::stream::Stream::from(server),
        ));
        session.read_request().await.unwrap();
        let mut ctx = new_pingora_context_for_tests();
        ctx.route = Some(Route::ChatCompletions);
        ctx.route_match =
            Some(Route::resolve_match(&http::Method::POST, "/v1/chat/completions").unwrap());
        ctx.key = Some(AuthenticatedKey {
            key_id: uuid::Uuid::new_v4(),
            project_id: None,
            key_prefix: "test".into(),
        });
        ctx.policy = Some(policy);
        ctx.request_content_type = Some("application/json".into());
        ctx.request_rewriter = Some(BoundedBodyRewriter::new(1024 * 1024));
        ctx.request_rate_count = Some(1);
        // Relayna-key canonical direct mode is buffered; client-LiteLLM-key
        // delegation (direct_litellm_passthrough) is a different contract.
        ctx.litellm_passthrough = direct;
        Self {
            proxy,
            session,
            client,
            ctx,
        }
    }

    async fn send(&mut self, bytes: Bytes, final_chunk: bool) -> (bool, Bytes) {
        let mut body = Some(bytes);
        let result = self
            .proxy
            .request_body_filter(&mut self.session, &mut body, final_chunk, &mut self.ctx)
            .await;
        (result.is_ok(), body.unwrap_or_default())
    }

    async fn response(&mut self) -> String {
        let response = tokio::time::timeout(Duration::from_secs(2), async {
            let mut response = Vec::new();
            while !response.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                let mut chunk = [0; 4096];
                let count = self.client.read(&mut chunk).await.unwrap();
                assert!(count > 0, "response headers must complete before EOF");
                response.extend_from_slice(&chunk[..count]);
                assert!(response.len() <= 16_384);
            }
            response
        })
        .await
        .unwrap();
        String::from_utf8(response).unwrap()
    }
}

fn size_policy(reserved_small_requests: usize) -> crate::SizeAwareBodyPolicy {
    crate::SizeAwareBodyPolicy {
        max_working_bytes: 32 * 1024 * 1024,
        reserved_small_requests,
        workspace_multiplier: 2,
        upload_timeout: Duration::from_secs(30),
    }
}

#[tokio::test]
async fn small_body_growth_cannot_bypass_reserved_capacity_and_releases_partial_body() {
    let mut h = Harness::new(KeyPolicy::default(), false).await;
    h.proxy.config = h
        .proxy
        .config
        .clone()
        .with_body_admission_limits(2, 8 * 1024 * 1024)
        .unwrap()
        .with_size_aware_body_policy(Some(size_policy(1)))
        .unwrap();
    let large = h
        .proxy
        .config
        .body_admission
        .try_acquire_sized(Some(1024 * 1024))
        .unwrap();
    h.ctx.request_rewriter =
        Some(BoundedBodyRewriter::new(1024 * 1024).with_expected_length(Some(128)));
    assert!(
        h.send(Bytes::from_static(b"{\"payload\":\""), false)
            .await
            .0
    );
    let (ok, forwarded) = h.send(Bytes::from(vec![b'x'; 300 * 1024]), false).await;
    assert!(!ok && forwarded.is_empty());
    assert!(h.response().await.starts_with("HTTP/1.1 503"));
    assert_eq!(h.proxy.config.body_admission.active_requests(), 1);
    assert_eq!(h.proxy.config.body_admission.active_bytes(), 0);
    drop(large);
    assert_eq!(h.proxy.config.body_admission.active_requests(), 0);
}

#[tokio::test]
async fn size_aware_body_keeps_policy_and_restores_read_timeout_after_upload() {
    let mut h = Harness::new(KeyPolicy::default(), false).await;
    h.proxy.config = h
        .proxy
        .config
        .clone()
        .with_size_aware_body_policy(Some(size_policy(0)))
        .unwrap();
    let raw = Bytes::from_static(br#"{"model":"gpt-test","input":"test"}"#);
    h.ctx.request_rewriter =
        Some(BoundedBodyRewriter::new(1024).with_expected_length(Some(raw.len())));
    h.session.set_read_timeout(Some(Duration::from_secs(90)));
    assert!(h.send(raw.slice(..10), false).await.0);
    assert!(h.session.get_read_timeout().unwrap() <= Duration::from_secs(30));
    let (ok, body) = h.send(raw.slice(10..), true).await;
    assert!(ok);
    assert_eq!(body, raw);
    assert_eq!(h.session.get_read_timeout(), Some(Duration::from_secs(90)));
    assert_eq!(h.proxy.config.body_admission.active_requests(), 1);
    drop(body);
    assert_eq!(h.proxy.config.body_admission.active_requests(), 0);
}

#[tokio::test]
async fn upstream_limit_spans_body_release_and_is_reused_on_retry() {
    let mut h = Harness::new(KeyPolicy::default(), false).await;
    h.proxy.config = h
        .proxy
        .config
        .clone()
        .with_upstream_request_limit(Some(1))
        .unwrap();
    h.proxy
        .upstream_peer(&mut h.session, &mut h.ctx)
        .await
        .unwrap();
    h.proxy
        .upstream_peer(&mut h.session, &mut h.ctx)
        .await
        .unwrap();
    assert_eq!(h.proxy.config.body_admission.active_requests(), 0);
    let mut other = new_pingora_context_for_tests();
    assert!(h
        .proxy
        .upstream_peer(&mut h.session, &mut other)
        .await
        .is_err());
    assert_eq!(other.guardrail_error, Some(GatewayError::GatewayOverloaded));
    // Cancellation drops the request context even when logging is not reached.
    drop(std::mem::replace(
        &mut h.ctx,
        new_pingora_context_for_tests(),
    ));
    h.proxy
        .upstream_peer(&mut h.session, &mut other)
        .await
        .unwrap();
}

#[tokio::test]
async fn expired_upload_releases_its_partial_buffer() {
    let mut h = Harness::new(KeyPolicy::default(), false).await;
    let mut policy = size_policy(0);
    policy.upload_timeout = Duration::from_millis(10);
    h.proxy.config = h
        .proxy
        .config
        .clone()
        .with_size_aware_body_policy(Some(policy))
        .unwrap();
    assert!(h.send(Bytes::from_static(b"{\"input\":\""), false).await.0);
    tokio::time::sleep(Duration::from_millis(20)).await;
    let (ok, body) = h.send(Bytes::from_static(b"late\"}"), true).await;
    assert!(!ok && body.is_empty());
    assert_eq!(h.proxy.config.body_admission.active_requests(), 0);
    assert_eq!(h.proxy.config.body_admission.active_bytes(), 0);
    assert!(h.response().await.starts_with("HTTP/1.1 503"));
}

#[tokio::test]
async fn complete_body_limits_cover_none_input_tpm_and_both_modes() {
    let raw = Bytes::from_static(br#"{"model":"gpt-test","input":"abcdefgh","max_tokens":12}"#);
    for direct in [false, true] {
        for (input_limit, tpm_limit, allowed) in [
            (None, None, true),
            (Some(16), None, true),
            (None, Some(16), true),
            (Some(16), Some(16), true),
            (Some(15), None, false),
            (None, Some(15), false),
            (None, Some(0), false),
            (None, Some(-1), false),
        ] {
            let mut h = Harness::new(
                KeyPolicy {
                    max_input_tokens_per_request: input_limit,
                    tpm_limit,
                    ..KeyPolicy::default()
                },
                direct,
            )
            .await;
            let (ok, body) = h.send(raw.clone(), true).await;
            assert_eq!(
                ok, allowed,
                "direct={direct}, input={input_limit:?}, tpm={tpm_limit:?}"
            );
            if allowed {
                assert_eq!(body, raw);
                assert!(h.ctx.budget_reserved);
                assert_eq!(
                    *h.proxy.control_state.token_calls.lock().unwrap(),
                    vec![(tpm_limit, if tpm_limit.is_some() { 16 } else { 0 })]
                );
                // Rechecking an already admitted body must not debit TPM twice.
                let mut emitted = None;
                h.proxy
                    .admit_complete_body(
                        &mut h.session,
                        &mut emitted,
                        &mut h.ctx,
                        CompleteRequestBody::new(body.clone()),
                    )
                    .await
                    .unwrap();
                assert_eq!(h.proxy.control_state.token_calls.lock().unwrap().len(), 1);
            } else {
                assert!(
                    body.is_empty(),
                    "a denied body must never be emitted upstream"
                );
                assert!(!h.ctx.budget_reserved);
                let response = h.response().await;
                if input_limit.is_some() {
                    assert!(h.proxy.control_state.token_calls.lock().unwrap().is_empty());
                    assert!(response.starts_with("HTTP/1.1 403"));
                    assert_eq!(h.ctx.guardrail_error, Some(GatewayError::PolicyDenied));
                } else {
                    assert_eq!(
                        *h.proxy.control_state.token_calls.lock().unwrap(),
                        vec![(tpm_limit, 16)]
                    );
                    assert!(response.starts_with("HTTP/1.1 429"));
                    assert_eq!(
                        h.ctx.guardrail_error,
                        Some(GatewayError::TokenRateLimitExceeded {
                            retry_after_seconds: Some(60)
                        })
                    );
                }
                assert!(response
                    .to_ascii_lowercase()
                    .contains("x-relayna-request-id:"));
            }
            drop(body);
            drop(h.ctx);
            assert_eq!(h.proxy.config.body_admission.active_requests(), 0);
            assert_eq!(h.proxy.config.body_admission.active_bytes(), 0);
        }
    }
}

#[tokio::test]
async fn model_tpm_uses_complete_body_beyond_prefix_and_accumulates() {
    for direct in [false, true] {
        let mut h = Harness::new(KeyPolicy::default(), direct).await;
        *h.proxy.store.model_policy.lock().unwrap() = Some((
            "late".into(),
            KeyPolicy {
                tpm_limit: Some(20_001),
                ..KeyPolicy::default()
            },
        ));
        let raw = Bytes::from(format!(
            "{{\"input\":\"{}\",\"model\":\"late\"}}",
            "x".repeat(80_000)
        ));
        let (ok, suppressed) = h.send(raw.slice(..70_000), false).await;
        assert!(ok && suppressed.is_empty());
        assert!(h.proxy.control_state.token_calls.lock().unwrap().is_empty());
        let (ok, body) = h.send(raw.slice(70_000..), true).await;
        assert!(ok);
        assert_eq!(body, raw);
        assert_eq!(
            *h.proxy.control_state.token_calls.lock().unwrap(),
            vec![(Some(20_001), 20_001)]
        );
        // A second reservation for the same key exceeds the cumulative bucket.
        h.ctx.budget_reserved = false;
        let mut emitted = None;
        assert!(h
            .proxy
            .admit_complete_body(
                &mut h.session,
                &mut emitted,
                &mut h.ctx,
                CompleteRequestBody::new(body)
            )
            .await
            .is_err());
        assert!(emitted.unwrap().is_empty());
        assert_eq!(h.proxy.control_state.token_calls.lock().unwrap().len(), 2);
        assert!(h.response().await.starts_with("HTTP/1.1 429"));
    }
}

#[tokio::test]
async fn rewritten_body_gets_new_estimate_and_keeps_original_policy_checks() {
    let raw = Bytes::from_static(
        br#"{"model":"gpt-test","input":"synthetic.long.address@example.test","max_tokens":12}"#,
    );
    for limit in [100, 16] {
        let mut h = Harness::new(
            KeyPolicy {
                max_input_tokens_per_request: Some(limit),
                tpm_limit: Some(100),
                ..KeyPolicy::default()
            },
            false,
        )
        .await;
        h.ctx.guardrail_definitions = vec![gateway_core::pii_redact_definition()];
        h.proxy
            .store
            .guardrail_policy
            .lock()
            .unwrap()
            .mandatory_guardrails = vec!["pii-redact".into()];
        let (ok, body) = h.send(raw.clone(), true).await;
        if limit == 100 {
            assert!(ok);
            assert_ne!(body, raw);
            let estimate = estimate_generation_tokens(&body);
            assert_ne!(estimate, estimate_generation_tokens(&raw));
            assert_eq!(
                *h.proxy.control_state.token_calls.lock().unwrap(),
                vec![(Some(100), estimate)]
            );
        } else {
            assert!(!ok && body.is_empty());
            assert_eq!(h.ctx.guardrail_error, Some(GatewayError::PolicyDenied));
            assert!(h.proxy.control_state.token_calls.lock().unwrap().is_empty());
        }
    }
}

#[test]
fn cached_analysis_keeps_malformed_selectors_and_non_json_service_semantics() {
    let matched = Route::resolve_match(&http::Method::POST, "/v1/chat/completions").unwrap();
    for raw in [
        b"{\"model\":\"a\",\"model\":\"b\"}".as_slice(),
        b"{\"stream\":null}",
        b"{",
        b"[]",
    ] {
        let analyzed = CompleteRequestBody::new(Bytes::copy_from_slice(raw));
        assert_eq!(
            check_complete_request_policy(
                &KeyPolicy::default(),
                &matched,
                &analyzed,
                &analyzed.features()
            ),
            Err(GatewayError::PolicyDenied)
        );
    }
    let service = service_route_match_for_persisted_registration(
        &http::Method::POST,
        "/services/test/run",
        "test",
    );
    let policy = KeyPolicy {
        allowed_routes: vec![service.route],
        allowed_providers: vec![service.provider],
        ..KeyPolicy::default()
    };
    let raw = CompleteRequestBody::new(Bytes::from_static(b"plain text"));
    assert!(check_complete_request_policy(&policy, &service, &raw, &raw.features()).is_ok());
    assert!(
        raw.tokens.get().is_none(),
        "uncapped service bodies need no token estimate"
    );
}

#[tokio::test]
async fn guardrail_expansion_is_rechecked_before_tpm_or_forwarding() {
    // The email has two estimated tokens; its [EMAIL_1] replacement has three.
    let raw = Bytes::from_static(br#"{"model":"gpt-test","input":"a@b.co","max_tokens":12}"#);
    assert_eq!(estimate_generation_tokens(&raw), 16);
    let mut h = Harness::new(
        KeyPolicy {
            max_input_tokens_per_request: Some(16),
            tpm_limit: Some(100),
            ..KeyPolicy::default()
        },
        false,
    )
    .await;
    h.ctx.guardrail_definitions = vec![gateway_core::pii_redact_definition()];
    h.proxy
        .store
        .guardrail_policy
        .lock()
        .unwrap()
        .mandatory_guardrails = vec!["pii-redact".into()];
    let (ok, body) = h.send(raw, true).await;
    assert!(!ok && body.is_empty());
    assert!(
        h.ctx.pre_guardrail_plan.is_some(),
        "original body passed policy and was rewritten"
    );
    assert_eq!(h.ctx.guardrail_error, Some(GatewayError::PolicyDenied));
    assert!(h.proxy.control_state.token_calls.lock().unwrap().is_empty());
    assert!(h.response().await.starts_with("HTTP/1.1 403"));
}
