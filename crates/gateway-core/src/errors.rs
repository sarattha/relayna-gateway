use http::StatusCode;
use serde::Serialize;
use thiserror::Error;

pub type GatewayResult<T> = Result<T, GatewayError>;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum GatewayError {
    #[error("missing authorization header")]
    MissingAuthorization,
    #[error("malformed authorization header")]
    MalformedAuthorization,
    #[error("invalid virtual key")]
    InvalidVirtualKey,
    #[error("virtual key is disabled")]
    DisabledVirtualKey,
    #[error("virtual key is revoked")]
    RevokedVirtualKey,
    #[error("virtual key has expired")]
    ExpiredVirtualKey,
    #[error("invalid operator token")]
    InvalidOperatorToken,
    #[error("operator token is disabled")]
    DisabledOperatorToken,
    #[error("operator token lacks required scope")]
    InsufficientOperatorScope,
    #[error("unsupported route")]
    UnsupportedRoute,
    #[error("route is disabled")]
    DisabledRoute,
    #[error("request body too large")]
    RequestBodyTooLarge,
    #[error("upstream timed out")]
    UpstreamTimeout,
    #[error("upstream connection failed")]
    UpstreamConnection,
    #[error("request denied by policy")]
    PolicyDenied,
    #[error("request rate limit exceeded")]
    RateLimitExceeded { retry_after_seconds: Option<u64> },
    #[error("token rate limit exceeded")]
    TokenRateLimitExceeded { retry_after_seconds: Option<u64> },
    #[error("budget exceeded")]
    BudgetExceeded,
    #[error("request blocked by guardrail")]
    GuardrailBlocked,
    #[error("guardrail is not allowed by policy")]
    GuardrailForbidden,
    #[error("guardrail is unavailable")]
    GuardrailUnavailable,
    #[error("guardrail request is invalid")]
    InvalidGuardrailRequest,
    #[error("project already exists")]
    DuplicateProject,
    #[error("project was not found")]
    MissingProject,
    #[error("project is still referenced")]
    ProjectInUse,
    #[error("project payload is invalid")]
    InvalidProjectPayload,
    #[error("provider configuration already exists")]
    DuplicateProviderConfig,
    #[error("provider configuration was not found")]
    MissingProviderConfig,
    #[error("provider configuration payload is invalid")]
    InvalidProviderConfigPayload,
    #[error("service registration already exists")]
    DuplicateService,
    #[error("service registration was not found")]
    MissingService,
    #[error("service registration is disabled")]
    DisabledService,
    #[error("service registration is incomplete")]
    IncompleteService,
    #[error("service registration payload is invalid")]
    InvalidServicePayload,
    #[error("service upstream configuration is invalid")]
    InvalidServiceUpstream,
    #[error("studio catalog is unavailable")]
    StudioUnavailable,
    #[error("studio connection payload is invalid")]
    InvalidStudioConnectionPayload,
    #[error("gateway control state is unavailable")]
    ControlStateUnavailable,
    #[error("store unavailable")]
    StoreUnavailable,
    #[error("gateway configuration is invalid")]
    InvalidConfiguration,
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
pub struct ErrorBody {
    pub error: ErrorEnvelope,
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
pub struct ErrorEnvelope {
    pub code: &'static str,
    pub message: &'static str,
    pub request_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry_after_seconds: Option<u64>,
}

impl GatewayError {
    pub fn status_code(&self) -> StatusCode {
        match self {
            Self::MissingAuthorization | Self::MalformedAuthorization => StatusCode::UNAUTHORIZED,
            Self::InvalidVirtualKey
            | Self::DisabledVirtualKey
            | Self::RevokedVirtualKey
            | Self::ExpiredVirtualKey => StatusCode::UNAUTHORIZED,
            Self::InvalidOperatorToken | Self::DisabledOperatorToken => StatusCode::UNAUTHORIZED,
            Self::InsufficientOperatorScope => StatusCode::FORBIDDEN,
            Self::UnsupportedRoute => StatusCode::NOT_FOUND,
            Self::DisabledRoute => StatusCode::FORBIDDEN,
            Self::RequestBodyTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            Self::PolicyDenied => StatusCode::FORBIDDEN,
            Self::RateLimitExceeded { .. } | Self::TokenRateLimitExceeded { .. } => {
                StatusCode::TOO_MANY_REQUESTS
            }
            Self::BudgetExceeded => StatusCode::PAYMENT_REQUIRED,
            Self::GuardrailBlocked | Self::GuardrailForbidden => StatusCode::FORBIDDEN,
            Self::GuardrailUnavailable => StatusCode::BAD_GATEWAY,
            Self::InvalidGuardrailRequest => StatusCode::BAD_REQUEST,
            Self::DuplicateProject | Self::DuplicateProviderConfig | Self::DuplicateService => {
                StatusCode::CONFLICT
            }
            Self::ProjectInUse => StatusCode::CONFLICT,
            Self::MissingProject | Self::MissingProviderConfig => StatusCode::NOT_FOUND,
            Self::InvalidProjectPayload | Self::InvalidProviderConfigPayload => {
                StatusCode::BAD_REQUEST
            }
            Self::MissingService => StatusCode::NOT_FOUND,
            Self::DisabledService => StatusCode::FORBIDDEN,
            Self::IncompleteService => StatusCode::CONFLICT,
            Self::InvalidServicePayload
            | Self::InvalidServiceUpstream
            | Self::InvalidStudioConnectionPayload => StatusCode::BAD_REQUEST,
            Self::StudioUnavailable => StatusCode::BAD_GATEWAY,
            Self::UpstreamTimeout => StatusCode::GATEWAY_TIMEOUT,
            Self::UpstreamConnection | Self::StoreUnavailable => StatusCode::BAD_GATEWAY,
            Self::ControlStateUnavailable => StatusCode::SERVICE_UNAVAILABLE,
            Self::InvalidConfiguration => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    pub fn code(&self) -> &'static str {
        match self {
            Self::MissingAuthorization => "missing_authorization",
            Self::MalformedAuthorization => "malformed_authorization",
            Self::InvalidVirtualKey => "invalid_virtual_key",
            Self::DisabledVirtualKey => "disabled_virtual_key",
            Self::RevokedVirtualKey => "revoked_virtual_key",
            Self::ExpiredVirtualKey => "expired_virtual_key",
            Self::InvalidOperatorToken => "invalid_operator_token",
            Self::DisabledOperatorToken => "disabled_operator_token",
            Self::InsufficientOperatorScope => "insufficient_operator_scope",
            Self::UnsupportedRoute => "unsupported_route",
            Self::DisabledRoute => "disabled_route",
            Self::RequestBodyTooLarge => "request_body_too_large",
            Self::UpstreamTimeout => "upstream_timeout",
            Self::UpstreamConnection => "upstream_connection",
            Self::PolicyDenied => "policy_denied",
            Self::RateLimitExceeded { .. } => "rate_limit_exceeded",
            Self::TokenRateLimitExceeded { .. } => "token_rate_limit_exceeded",
            Self::BudgetExceeded => "budget_exceeded",
            Self::GuardrailBlocked => "guardrail_blocked",
            Self::GuardrailForbidden => "guardrail_forbidden",
            Self::GuardrailUnavailable => "guardrail_unavailable",
            Self::InvalidGuardrailRequest => "invalid_guardrail_request",
            Self::DuplicateProject => "duplicate_project",
            Self::MissingProject => "missing_project",
            Self::ProjectInUse => "project_in_use",
            Self::InvalidProjectPayload => "invalid_project_payload",
            Self::DuplicateProviderConfig => "duplicate_provider_config",
            Self::MissingProviderConfig => "missing_provider_config",
            Self::InvalidProviderConfigPayload => "invalid_provider_config_payload",
            Self::DuplicateService => "duplicate_service",
            Self::MissingService => "missing_service",
            Self::DisabledService => "disabled_service",
            Self::IncompleteService => "incomplete_service",
            Self::InvalidServicePayload => "invalid_service_payload",
            Self::InvalidServiceUpstream => "invalid_service_upstream",
            Self::StudioUnavailable => "studio_unavailable",
            Self::InvalidStudioConnectionPayload => "invalid_studio_connection_payload",
            Self::ControlStateUnavailable => "control_state_unavailable",
            Self::StoreUnavailable => "store_unavailable",
            Self::InvalidConfiguration => "invalid_configuration",
        }
    }

    pub fn public_message(&self) -> &'static str {
        match self {
            Self::MissingAuthorization => "Authorization header is required.",
            Self::MalformedAuthorization => "Authorization header must be a Bearer Relayna key.",
            Self::InvalidVirtualKey => "Virtual key is invalid.",
            Self::DisabledVirtualKey => "Virtual key is disabled.",
            Self::RevokedVirtualKey => "Virtual key is revoked.",
            Self::ExpiredVirtualKey => "Virtual key has expired.",
            Self::InvalidOperatorToken => "Operator token is invalid.",
            Self::DisabledOperatorToken => "Operator token is disabled.",
            Self::InsufficientOperatorScope => "Operator token lacks the required scope.",
            Self::UnsupportedRoute => "Route is not supported by this gateway.",
            Self::DisabledRoute => "Route is disabled by gateway policy.",
            Self::RequestBodyTooLarge => "Request body exceeds the route limit.",
            Self::PolicyDenied => "Request is denied by key policy.",
            Self::RateLimitExceeded { .. } => "Rate limit exceeded.",
            Self::TokenRateLimitExceeded { .. } => "Token rate limit exceeded.",
            Self::BudgetExceeded => "Budget limit exceeded.",
            Self::GuardrailBlocked => "Request was blocked by a gateway guardrail.",
            Self::GuardrailForbidden => "Guardrail is not allowed by policy.",
            Self::GuardrailUnavailable => "Gateway guardrail is unavailable.",
            Self::InvalidGuardrailRequest => "Guardrail request is invalid.",
            Self::DuplicateProject => "Project already exists.",
            Self::MissingProject => "Project was not found.",
            Self::ProjectInUse => "Project is still referenced.",
            Self::InvalidProjectPayload => "Project payload is invalid.",
            Self::DuplicateProviderConfig => "Provider configuration already exists.",
            Self::MissingProviderConfig => "Provider configuration was not found.",
            Self::InvalidProviderConfigPayload => "Provider configuration payload is invalid.",
            Self::DuplicateService => "Service registration already exists.",
            Self::MissingService => "Service registration was not found.",
            Self::DisabledService => "Service registration is disabled.",
            Self::IncompleteService => "Service registration is incomplete.",
            Self::InvalidServicePayload => "Service registration payload is invalid.",
            Self::InvalidServiceUpstream => "Service upstream configuration is invalid.",
            Self::StudioUnavailable => "Relayna Studio catalog is unavailable.",
            Self::InvalidStudioConnectionPayload => "Studio connection payload is invalid.",
            Self::UpstreamTimeout => "Upstream provider timed out.",
            Self::UpstreamConnection => "Upstream provider is unavailable.",
            Self::ControlStateUnavailable => "Gateway control state is unavailable.",
            Self::StoreUnavailable => "Gateway store is unavailable.",
            Self::InvalidConfiguration => "Gateway configuration is invalid.",
        }
    }

    pub fn body(&self, request_id: impl Into<String>) -> ErrorBody {
        ErrorBody {
            error: ErrorEnvelope {
                code: self.code(),
                message: self.public_message(),
                request_id: request_id.into(),
                retry_after_seconds: match self {
                    Self::RateLimitExceeded {
                        retry_after_seconds,
                    }
                    | Self::TokenRateLimitExceeded {
                        retry_after_seconds,
                    } => *retry_after_seconds,
                    _ => None,
                },
            },
        }
    }
}
