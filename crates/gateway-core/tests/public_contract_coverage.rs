use gateway_core::GatewayError;

#[test]
fn every_public_gateway_error_has_a_complete_http_contract() {
    let errors = vec![
        GatewayError::MissingAuthorization,
        GatewayError::MalformedAuthorization,
        GatewayError::InvalidVirtualKey,
        GatewayError::DisabledVirtualKey,
        GatewayError::RevokedVirtualKey,
        GatewayError::ExpiredVirtualKey,
        GatewayError::MissingEntraAuthorization,
        GatewayError::MalformedEntraAuthorization,
        GatewayError::InvalidEntraToken,
        GatewayError::ExpiredEntraToken,
        GatewayError::InvalidEntraAudience,
        GatewayError::InvalidEntraIssuer,
        GatewayError::InsufficientEntraAuthorization,
        GatewayError::UntrustedApigeeIdentity,
        GatewayError::InvalidOperatorToken,
        GatewayError::DisabledOperatorToken,
        GatewayError::InsufficientOperatorScope,
        GatewayError::UnsupportedRoute,
        GatewayError::DisabledRoute,
        GatewayError::RequestBodyTooLarge,
        GatewayError::ResponseBodyTooLarge,
        GatewayError::GatewayOverloaded,
        GatewayError::UpstreamTimeout,
        GatewayError::UpstreamConnection,
        GatewayError::PolicyDenied,
        GatewayError::RateLimitExceeded {
            retry_after_seconds: Some(7),
        },
        GatewayError::TokenRateLimitExceeded {
            retry_after_seconds: Some(9),
        },
        GatewayError::BudgetExceeded,
        GatewayError::GuardrailBlocked,
        GatewayError::GuardrailForbidden,
        GatewayError::GuardrailUnavailable,
        GatewayError::InvalidGuardrailRequest,
        GatewayError::DuplicateProject,
        GatewayError::MissingProject,
        GatewayError::ProjectInUse,
        GatewayError::InvalidProjectPayload,
        GatewayError::DuplicateProviderConfig,
        GatewayError::MissingProviderConfig,
        GatewayError::InvalidProviderConfigPayload,
        GatewayError::DuplicateService,
        GatewayError::MissingService,
        GatewayError::DisabledService,
        GatewayError::IncompleteService,
        GatewayError::InvalidServicePayload,
        GatewayError::InvalidServiceUpstream,
        GatewayError::ServiceOpenApiUnavailable,
        GatewayError::InvalidServiceOpenApi,
        GatewayError::ServiceOpenApiChanged,
        GatewayError::InvalidUsageQuery,
        GatewayError::MissingDebugBundle,
        GatewayError::StudioUnavailable,
        GatewayError::InvalidStudioConnectionPayload,
        GatewayError::ControlStateUnavailable,
        GatewayError::StoreUnavailable,
        GatewayError::InvalidConfiguration,
    ];

    for error in errors {
        assert!(error.status_code().is_client_error() || error.status_code().is_server_error());
        assert!(!error.code().is_empty());
        assert!(!error.public_message().is_empty());
        let body = error.body("coverage-request");
        assert_eq!(body.error.code, error.code());
        assert_eq!(body.error.message, error.public_message());
        assert_eq!(body.error.request_id, "coverage-request");
        assert!(!error.to_string().is_empty());
    }
}

#[test]
fn rate_limit_retry_hint_is_optional_and_preserved() {
    assert_eq!(
        GatewayError::RateLimitExceeded {
            retry_after_seconds: None,
        }
        .body("request")
        .error
        .retry_after_seconds,
        None
    );
    assert_eq!(
        GatewayError::TokenRateLimitExceeded {
            retry_after_seconds: Some(13),
        }
        .body("request")
        .error
        .retry_after_seconds,
        Some(13)
    );
    let overloaded = GatewayError::GatewayOverloaded;
    assert_eq!(
        overloaded.status_code(),
        http::StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(overloaded.code(), "gateway_overloaded");
    assert_eq!(
        overloaded.body("request").error.retry_after_seconds,
        Some(1)
    );
}
