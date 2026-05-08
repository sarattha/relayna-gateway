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
}

impl GatewayError {
    pub fn status_code(&self) -> StatusCode {
        match self {
            Self::MissingAuthorization | Self::MalformedAuthorization => StatusCode::UNAUTHORIZED,
            Self::InvalidVirtualKey | Self::DisabledVirtualKey | Self::ExpiredVirtualKey => {
                StatusCode::UNAUTHORIZED
            }
            Self::UnsupportedRoute => StatusCode::NOT_FOUND,
            Self::UpstreamTimeout => StatusCode::GATEWAY_TIMEOUT,
            Self::UpstreamConnection | Self::StoreUnavailable => StatusCode::BAD_GATEWAY,
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
            Self::UpstreamTimeout => "Upstream provider timed out.",
            Self::UpstreamConnection => "Upstream provider is unavailable.",
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
            },
        }
    }
}
