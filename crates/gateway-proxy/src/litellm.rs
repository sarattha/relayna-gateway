use gateway_core::{AuthenticatedKey, GatewayError, GatewayResult, Route};
use http::{header, HeaderMap, HeaderName, HeaderValue, Method, StatusCode};
use reqwest::Url;
use std::time::Duration;

const HOP_BY_HOP_HEADERS: &[&str] = &[
    "connection",
    "keep-alive",
    "proxy-authenticate",
    "proxy-authorization",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
];

#[derive(Debug, Clone)]
pub struct LiteLlmConfig {
    pub base_url: Url,
    pub service_key: String,
    pub timeout: Duration,
}

impl LiteLlmConfig {
    pub fn new(
        base_url: impl AsRef<str>,
        service_key: impl Into<String>,
        timeout: Duration,
    ) -> GatewayResult<Self> {
        let base_url =
            Url::parse(base_url.as_ref()).map_err(|_| GatewayError::InvalidConfiguration)?;
        Ok(Self {
            base_url,
            service_key: service_key.into(),
            timeout,
        })
    }
}

#[derive(Clone)]
pub struct LiteLlmProxy {
    client: reqwest::Client,
    config: LiteLlmConfig,
}

#[derive(Debug, Clone)]
pub struct UpstreamResponse {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: Vec<u8>,
}

impl LiteLlmProxy {
    pub fn new(config: LiteLlmConfig) -> GatewayResult<Self> {
        let client = reqwest::Client::builder()
            .timeout(config.timeout)
            .build()
            .map_err(|_| GatewayError::InvalidConfiguration)?;
        Ok(Self { client, config })
    }

    pub async fn forward(
        &self,
        route: Route,
        query: Option<&str>,
        inbound_headers: &HeaderMap,
        body: Vec<u8>,
        request_id: &str,
        key: &AuthenticatedKey,
    ) -> GatewayResult<UpstreamResponse> {
        let url = self.upstream_url(route, query)?;
        let headers =
            build_upstream_headers(inbound_headers, &self.config.service_key, request_id, key)?;

        let response = self
            .client
            .request(Method::POST, url)
            .headers(headers)
            .body(body)
            .send()
            .await
            .map_err(map_reqwest_error)?;

        let status = response.status();
        let headers = filter_response_headers(response.headers());
        let body = response.bytes().await.map_err(map_reqwest_error)?.to_vec();

        Ok(UpstreamResponse {
            status,
            headers,
            body,
        })
    }

    fn upstream_url(&self, route: Route, query: Option<&str>) -> GatewayResult<Url> {
        let mut url = self.config.base_url.clone();
        url.set_path(route.as_str());
        url.set_query(query);
        Ok(url)
    }
}

pub fn build_upstream_headers(
    inbound: &HeaderMap,
    service_key: &str,
    request_id: &str,
    key: &AuthenticatedKey,
) -> GatewayResult<HeaderMap> {
    let mut headers = HeaderMap::new();

    for (name, value) in inbound {
        if should_forward_header(name) {
            headers.append(name, value.clone());
        }
    }

    let authorization = HeaderValue::from_str(&format!("Bearer {service_key}"))
        .map_err(|_| GatewayError::InvalidConfiguration)?;
    headers.insert(header::AUTHORIZATION, authorization);
    headers.insert(
        HeaderName::from_static("x-relayna-request-id"),
        HeaderValue::from_str(request_id).map_err(|_| GatewayError::InvalidConfiguration)?,
    );
    headers.insert(
        HeaderName::from_static("x-relayna-key-id"),
        HeaderValue::from_str(&key.key_id.to_string())
            .map_err(|_| GatewayError::InvalidConfiguration)?,
    );
    headers.insert(
        HeaderName::from_static("x-relayna-project-id"),
        HeaderValue::from_str(&key.project_id.to_string())
            .map_err(|_| GatewayError::InvalidConfiguration)?,
    );

    Ok(headers)
}

fn should_forward_header(name: &HeaderName) -> bool {
    if name == header::AUTHORIZATION || name == header::HOST {
        return false;
    }

    !HOP_BY_HOP_HEADERS
        .iter()
        .any(|blocked| name.as_str().eq_ignore_ascii_case(blocked))
}

fn filter_response_headers(inbound: &HeaderMap) -> HeaderMap {
    let mut headers = HeaderMap::new();
    for (name, value) in inbound {
        if should_forward_response_header(name) {
            headers.append(name, value.clone());
        }
    }
    headers
}

fn should_forward_response_header(name: &HeaderName) -> bool {
    !HOP_BY_HOP_HEADERS
        .iter()
        .any(|blocked| name.as_str().eq_ignore_ascii_case(blocked))
}

fn map_reqwest_error(err: reqwest::Error) -> GatewayError {
    if err.is_timeout() {
        GatewayError::UpstreamTimeout
    } else {
        GatewayError::UpstreamConnection
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn key() -> AuthenticatedKey {
        AuthenticatedKey {
            key_id: Uuid::new_v4(),
            project_id: Uuid::new_v4(),
            key_prefix: "rk_live_12345678".to_owned(),
        }
    }

    #[test]
    fn strips_client_authorization_and_injects_litellm_key() {
        let mut inbound = HeaderMap::new();
        inbound.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer client"),
        );
        inbound.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );

        let headers = build_upstream_headers(&inbound, "litellm-service", "req_123", &key())
            .expect("headers");

        assert_eq!(
            headers.get(header::AUTHORIZATION).expect("authorization"),
            "Bearer litellm-service"
        );
        assert_eq!(
            headers.get(header::CONTENT_TYPE).expect("content-type"),
            "application/json"
        );
        assert_eq!(
            headers
                .get(HeaderName::from_static("x-relayna-request-id"))
                .expect("request id"),
            "req_123"
        );
    }

    #[test]
    fn constructs_upstream_config() {
        let config = LiteLlmConfig::new("http://127.0.0.1:4000", "service", Duration::from_secs(3))
            .expect("config");
        let proxy = LiteLlmProxy::new(config).expect("proxy");
        let url = proxy
            .upstream_url(Route::Responses, Some("trace=1"))
            .expect("url");

        assert_eq!(url.as_str(), "http://127.0.0.1:4000/v1/responses?trace=1");
    }
}
