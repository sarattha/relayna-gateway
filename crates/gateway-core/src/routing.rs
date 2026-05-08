use crate::errors::{GatewayError, GatewayResult};
use http::Method;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Provider {
    LiteLlm,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Route {
    ChatCompletions,
    Responses,
}

impl Route {
    pub fn resolve(method: &Method, path: &str) -> GatewayResult<Self> {
        if method != Method::POST {
            return Err(GatewayError::UnsupportedRoute);
        }

        match path {
            "/v1/chat/completions" => Ok(Self::ChatCompletions),
            "/v1/responses" => Ok(Self::Responses),
            _ => Err(GatewayError::UnsupportedRoute),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::ChatCompletions => "/v1/chat/completions",
            Self::Responses => "/v1/responses",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_phase_one_generation_routes() {
        assert_eq!(
            Route::resolve(&Method::POST, "/v1/chat/completions").expect("route"),
            Route::ChatCompletions
        );
        assert_eq!(
            Route::resolve(&Method::POST, "/v1/responses").expect("route"),
            Route::Responses
        );
    }

    #[test]
    fn rejects_unsupported_routes() {
        assert_eq!(
            Route::resolve(&Method::GET, "/v1/responses").unwrap_err(),
            GatewayError::UnsupportedRoute
        );
        assert_eq!(
            Route::resolve(&Method::POST, "/v1/completions").unwrap_err(),
            GatewayError::UnsupportedRoute
        );
    }
}
