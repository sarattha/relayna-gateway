use crate::errors::{GatewayError, GatewayResult};
use http::Method;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Provider {
    LiteLlm,
    OpenAiCompatible,
    InternalService,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Route {
    ChatCompletions,
    Responses,
    DirectOpenAi,
    Summary,
    Translation,
    Ocr,
    Embeddings,
    ServiceWildcard,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendType {
    LiteLlm,
    DirectProvider,
    InternalService,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RouteMatch {
    pub route: Route,
    pub backend: BackendType,
    pub provider: Provider,
    pub service_name: Option<String>,
    pub timeout_ms: u64,
    pub max_body_bytes: usize,
    pub estimated_cost_usd: Option<f64>,
}

impl Route {
    pub fn resolve(method: &Method, path: &str) -> GatewayResult<Self> {
        Self::resolve_match(method, path).map(|matched| matched.route)
    }

    pub fn resolve_match(method: &Method, path: &str) -> GatewayResult<RouteMatch> {
        if method != Method::POST {
            return Err(GatewayError::UnsupportedRoute);
        }

        match path {
            "/v1/chat/completions" => Ok(RouteMatch::litellm(Self::ChatCompletions)),
            "/v1/responses" => Ok(RouteMatch::litellm(Self::Responses)),
            "/summary" => Ok(RouteMatch::service(Self::Summary, "summary")),
            "/translation" => Ok(RouteMatch::service(Self::Translation, "translation")),
            "/ocr" => Ok(RouteMatch::service(Self::Ocr, "ocr")),
            "/embeddings" => Ok(RouteMatch::service(Self::Embeddings, "embeddings")),
            _ if path.starts_with("/services/") => {
                let service_name = path
                    .trim_start_matches("/services/")
                    .split('/')
                    .next()
                    .filter(|value| !value.is_empty())
                    .ok_or(GatewayError::UnsupportedRoute)?;
                Ok(RouteMatch::service(Self::ServiceWildcard, service_name))
            }
            _ if path.starts_with("/providers/openai/") => Ok(RouteMatch {
                route: Self::DirectOpenAi,
                backend: BackendType::DirectProvider,
                provider: Provider::OpenAiCompatible,
                service_name: None,
                timeout_ms: 60_000,
                max_body_bytes: 1_048_576,
                estimated_cost_usd: Some(0.01),
            }),
            _ => Err(GatewayError::UnsupportedRoute),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::ChatCompletions => "/v1/chat/completions",
            Self::Responses => "/v1/responses",
            Self::DirectOpenAi => "/providers/openai/*",
            Self::Summary => "/summary",
            Self::Translation => "/translation",
            Self::Ocr => "/ocr",
            Self::Embeddings => "/embeddings",
            Self::ServiceWildcard => "/services/*",
        }
    }
}

impl Provider {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::LiteLlm => "litellm",
            Self::OpenAiCompatible => "openai-compatible",
            Self::InternalService => "internal-service",
        }
    }
}

impl RouteMatch {
    fn litellm(route: Route) -> Self {
        Self {
            route,
            backend: BackendType::LiteLlm,
            provider: Provider::LiteLlm,
            service_name: None,
            timeout_ms: 120_000,
            max_body_bytes: 1_048_576,
            estimated_cost_usd: Some(0.01),
        }
    }

    fn service(route: Route, service_name: &str) -> Self {
        Self {
            route,
            backend: BackendType::InternalService,
            provider: Provider::InternalService,
            service_name: Some(service_name.to_owned()),
            timeout_ms: 60_000,
            max_body_bytes: 2_097_152,
            estimated_cost_usd: Some(0.01),
        }
    }
}

pub fn is_retry_safe_status(status_code: u16) -> bool {
    matches!(status_code, 429 | 500 | 502 | 503 | 504)
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

    #[test]
    fn resolves_direct_provider_and_internal_service_routes() {
        let direct = Route::resolve_match(&Method::POST, "/providers/openai/v1/chat/completions")
            .expect("direct provider");
        assert_eq!(direct.route, Route::DirectOpenAi);
        assert_eq!(direct.backend, BackendType::DirectProvider);
        assert_eq!(direct.provider, Provider::OpenAiCompatible);

        let summary = Route::resolve_match(&Method::POST, "/summary").expect("summary");
        assert_eq!(summary.route, Route::Summary);
        assert_eq!(summary.backend, BackendType::InternalService);
        assert_eq!(summary.service_name.as_deref(), Some("summary"));

        let wildcard =
            Route::resolve_match(&Method::POST, "/services/custom-ai/run").expect("service");
        assert_eq!(wildcard.route, Route::ServiceWildcard);
        assert_eq!(wildcard.service_name.as_deref(), Some("custom-ai"));
    }

    #[test]
    fn classifies_retry_safe_status_codes() {
        assert!(is_retry_safe_status(429));
        assert!(is_retry_safe_status(503));
        assert!(!is_retry_safe_status(400));
        assert!(!is_retry_safe_status(401));
    }
}
