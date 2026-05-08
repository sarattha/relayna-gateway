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
    #[error("virtual key has expired")]
    ExpiredVirtualKey,
    #[error("unsupported route")]
    UnsupportedRoute,
    #[error("upstream timed out")]
    UpstreamTimeout,
    #[error("upstream connection failed")]
    UpstreamConnection,
    #[error("request denied by policy")]
    PolicyDenied,
    #[error("request rate limit exceeded")]
    RateLimitExceeded { retry_after_seconds: Option<u64> },
    #[error("budget exceeded")]
    BudgetExceeded,
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
            Self::InvalidVirtualKey | Self::DisabledVirtualKey | Self::ExpiredVirtualKey => {
                StatusCode::UNAUTHORIZED
            }
            Self::UnsupportedRoute => StatusCode::NOT_FOUND,
            Self::PolicyDenied => StatusCode::FORBIDDEN,
            Self::RateLimitExceeded { .. } => StatusCode::TOO_MANY_REQUESTS,
            Self::BudgetExceeded => StatusCode::PAYMENT_REQUIRED,
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
            Self::ExpiredVirtualKey => "expired_virtual_key",
            Self::UnsupportedRoute => "unsupported_route",
            Self::UpstreamTimeout => "upstream_timeout",
            Self::UpstreamConnection => "upstream_connection",
            Self::PolicyDenied => "policy_denied",
            Self::RateLimitExceeded { .. } => "rate_limit_exceeded",
            Self::BudgetExceeded => "budget_exceeded",
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
            Self::ExpiredVirtualKey => "Virtual key has expired.",
            Self::UnsupportedRoute => "Route is not supported by this gateway.",
            Self::PolicyDenied => "Request is denied by key policy.",
            Self::RateLimitExceeded { .. } => "Rate limit exceeded.",
            Self::BudgetExceeded => "Budget limit exceeded.",
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
                    } => *retry_after_seconds,
                    _ => None,
                },
            },
        }
    }
}
