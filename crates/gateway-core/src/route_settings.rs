use crate::{GatewayError, GatewayResult, Route};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use http::Method;
use serde::{Deserialize, Serialize};

pub const CHAT_COMPLETIONS_ROUTE_ID: &str = "chat-completions";
pub const EMBEDDINGS_ROUTE_ID: &str = "embeddings";
pub const RESPONSES_ROUTE_ID: &str = "responses";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpenAiRouteMode {
    #[serde(rename = "managed_by_gateway")]
    #[default]
    ManagedByGateway,
    #[serde(rename = "direct_litellm_passthrough")]
    DirectLiteLlmPassthrough,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OpenAiRouteSetting {
    pub route_id: String,
    pub route: String,
    pub enabled: bool,
    pub mode: OpenAiRouteMode,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LiteLlmSensitiveRouteExposure {
    #[default]
    Disabled,
    OperatorOnly,
    ExplicitlyExposed,
    TrustedIngress,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct LiteLlmPassthroughSettingsPatchRequest {
    pub enabled: Option<bool>,
    pub allowed_paths: Option<Vec<String>>,
    pub allowed_methods: Option<Vec<String>>,
    pub ui_exposure: Option<LiteLlmSensitiveRouteExposure>,
    pub admin_api_exposure: Option<LiteLlmSensitiveRouteExposure>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LiteLlmPassthroughSettings {
    pub enabled: bool,
    pub allowed_paths: Vec<String>,
    pub allowed_methods: Vec<String>,
    pub ui_exposure: LiteLlmSensitiveRouteExposure,
    pub admin_api_exposure: LiteLlmSensitiveRouteExposure,
    pub updated_at: DateTime<Utc>,
}

#[async_trait]
pub trait AdminOpenAiRouteStore: Send + Sync {
    async fn list_openai_route_settings(&self) -> GatewayResult<Vec<OpenAiRouteSetting>>;

    async fn set_openai_route_enabled(
        &self,
        route_id: &str,
        enabled: bool,
    ) -> GatewayResult<Option<OpenAiRouteSetting>>;

    async fn set_openai_route_mode(
        &self,
        route_id: &str,
        mode: OpenAiRouteMode,
    ) -> GatewayResult<Option<OpenAiRouteSetting>>;

    async fn get_litellm_passthrough_settings(&self) -> GatewayResult<LiteLlmPassthroughSettings>;

    async fn patch_litellm_passthrough_settings(
        &self,
        patch: LiteLlmPassthroughSettingsPatchRequest,
    ) -> GatewayResult<LiteLlmPassthroughSettings>;
}

#[async_trait]
impl<T> AdminOpenAiRouteStore for std::sync::Arc<T>
where
    T: AdminOpenAiRouteStore + ?Sized,
{
    async fn list_openai_route_settings(&self) -> GatewayResult<Vec<OpenAiRouteSetting>> {
        (**self).list_openai_route_settings().await
    }

    async fn set_openai_route_enabled(
        &self,
        route_id: &str,
        enabled: bool,
    ) -> GatewayResult<Option<OpenAiRouteSetting>> {
        (**self).set_openai_route_enabled(route_id, enabled).await
    }

    async fn set_openai_route_mode(
        &self,
        route_id: &str,
        mode: OpenAiRouteMode,
    ) -> GatewayResult<Option<OpenAiRouteSetting>> {
        (**self).set_openai_route_mode(route_id, mode).await
    }

    async fn get_litellm_passthrough_settings(&self) -> GatewayResult<LiteLlmPassthroughSettings> {
        (**self).get_litellm_passthrough_settings().await
    }

    async fn patch_litellm_passthrough_settings(
        &self,
        patch: LiteLlmPassthroughSettingsPatchRequest,
    ) -> GatewayResult<LiteLlmPassthroughSettings> {
        (**self).patch_litellm_passthrough_settings(patch).await
    }
}

#[async_trait]
pub trait OpenAiRouteSettingsLookup: Send + Sync {
    async fn openai_route_enabled(&self, route: Route) -> GatewayResult<bool>;
    async fn openai_route_mode(&self, route: Route) -> GatewayResult<OpenAiRouteMode>;
    async fn litellm_passthrough_settings(&self) -> GatewayResult<LiteLlmPassthroughSettings>;
}

#[async_trait]
impl<T> OpenAiRouteSettingsLookup for std::sync::Arc<T>
where
    T: OpenAiRouteSettingsLookup + ?Sized,
{
    async fn openai_route_enabled(&self, route: Route) -> GatewayResult<bool> {
        (**self).openai_route_enabled(route).await
    }

    async fn openai_route_mode(&self, route: Route) -> GatewayResult<OpenAiRouteMode> {
        (**self).openai_route_mode(route).await
    }

    async fn litellm_passthrough_settings(&self) -> GatewayResult<LiteLlmPassthroughSettings> {
        (**self).litellm_passthrough_settings().await
    }
}

pub fn openai_route_id(route: Route) -> Option<&'static str> {
    match route {
        Route::ChatCompletions => Some(CHAT_COMPLETIONS_ROUTE_ID),
        Route::Responses => Some(RESPONSES_ROUTE_ID),
        Route::LiteLlmEmbeddings => Some(EMBEDDINGS_ROUTE_ID),
        _ => None,
    }
}

pub fn openai_route_from_id(route_id: &str) -> Option<Route> {
    match route_id {
        CHAT_COMPLETIONS_ROUTE_ID => Some(Route::ChatCompletions),
        RESPONSES_ROUTE_ID => Some(Route::Responses),
        EMBEDDINGS_ROUTE_ID => Some(Route::LiteLlmEmbeddings),
        _ => None,
    }
}

impl LiteLlmPassthroughSettingsPatchRequest {
    pub fn validate(&self) -> GatewayResult<()> {
        if let Some(paths) = &self.allowed_paths {
            validate_allowed_paths(paths)?;
        }
        if let Some(methods) = &self.allowed_methods {
            validate_allowed_methods(methods)?;
        }
        if self.admin_api_exposure == Some(LiteLlmSensitiveRouteExposure::TrustedIngress) {
            return Err(GatewayError::InvalidProviderConfigPayload);
        }
        Ok(())
    }
}

impl LiteLlmPassthroughSettings {
    pub fn default_with_updated_at(updated_at: DateTime<Utc>) -> Self {
        Self {
            enabled: false,
            allowed_paths: vec!["/v1/*".to_owned()],
            allowed_methods: vec!["GET".to_owned(), "POST".to_owned()],
            ui_exposure: LiteLlmSensitiveRouteExposure::Disabled,
            admin_api_exposure: LiteLlmSensitiveRouteExposure::Disabled,
            updated_at,
        }
    }

    pub fn allows(&self, method: &Method, path: &str) -> bool {
        if !self.enabled {
            return false;
        }
        let method = method.as_str();
        if !self
            .allowed_methods
            .iter()
            .any(|allowed| allowed.eq_ignore_ascii_case(method))
        {
            return false;
        }
        if self.sensitive_exposure_for_path(path) == Some(LiteLlmSensitiveRouteExposure::Disabled) {
            return false;
        }
        self.allowed_paths
            .iter()
            .any(|allowed| path_matches_allowed_pattern(path, allowed))
    }

    pub fn sensitive_exposure_for_path(&self, path: &str) -> Option<LiteLlmSensitiveRouteExposure> {
        if is_litellm_ui_path(path) {
            Some(self.ui_exposure)
        } else if is_litellm_admin_path(path) {
            Some(self.admin_api_exposure)
        } else {
            None
        }
    }

    pub fn trusted_ingress_ui_path_allowed(&self, method: &Method, path: &str) -> bool {
        if self.ui_exposure != LiteLlmSensitiveRouteExposure::TrustedIngress || !self.enabled {
            return false;
        }
        let method = method.as_str();
        if !self
            .allowed_methods
            .iter()
            .any(|allowed| allowed.eq_ignore_ascii_case(method))
        {
            return false;
        }
        if !(is_litellm_ui_path(path) || is_litellm_ui_support_path(path)) {
            return false;
        }
        self.allowed_paths
            .iter()
            .any(|allowed| path_matches_allowed_pattern(path, allowed))
    }
}

pub fn openai_route_mode_str(mode: OpenAiRouteMode) -> &'static str {
    match mode {
        OpenAiRouteMode::ManagedByGateway => "managed_by_gateway",
        OpenAiRouteMode::DirectLiteLlmPassthrough => "direct_litellm_passthrough",
    }
}

pub fn parse_openai_route_mode(value: &str) -> GatewayResult<OpenAiRouteMode> {
    match value {
        "managed_by_gateway" => Ok(OpenAiRouteMode::ManagedByGateway),
        "direct_litellm_passthrough" => Ok(OpenAiRouteMode::DirectLiteLlmPassthrough),
        _ => Err(GatewayError::InvalidProviderConfigPayload),
    }
}

pub fn litellm_exposure_str(exposure: LiteLlmSensitiveRouteExposure) -> &'static str {
    match exposure {
        LiteLlmSensitiveRouteExposure::Disabled => "disabled",
        LiteLlmSensitiveRouteExposure::OperatorOnly => "operator_only",
        LiteLlmSensitiveRouteExposure::ExplicitlyExposed => "explicitly_exposed",
        LiteLlmSensitiveRouteExposure::TrustedIngress => "trusted_ingress",
    }
}

pub fn parse_litellm_exposure(value: &str) -> GatewayResult<LiteLlmSensitiveRouteExposure> {
    match value {
        "disabled" => Ok(LiteLlmSensitiveRouteExposure::Disabled),
        "operator_only" => Ok(LiteLlmSensitiveRouteExposure::OperatorOnly),
        "explicitly_exposed" => Ok(LiteLlmSensitiveRouteExposure::ExplicitlyExposed),
        "trusted_ingress" => Ok(LiteLlmSensitiveRouteExposure::TrustedIngress),
        _ => Err(GatewayError::InvalidProviderConfigPayload),
    }
}

fn validate_allowed_paths(paths: &[String]) -> GatewayResult<()> {
    if paths.is_empty() || paths.len() > 64 {
        return Err(GatewayError::InvalidProviderConfigPayload);
    }
    for path in paths {
        let path = path.trim();
        if !path.starts_with('/') || path.contains("..") || path.contains(char::is_whitespace) {
            return Err(GatewayError::InvalidProviderConfigPayload);
        }
    }
    Ok(())
}

fn validate_allowed_methods(methods: &[String]) -> GatewayResult<()> {
    if methods.is_empty() || methods.len() > 16 {
        return Err(GatewayError::InvalidProviderConfigPayload);
    }
    for method in methods {
        if method.trim().parse::<Method>().is_err() {
            return Err(GatewayError::InvalidProviderConfigPayload);
        }
    }
    Ok(())
}

fn path_matches_allowed_pattern(path: &str, pattern: &str) -> bool {
    let pattern = pattern.trim();
    if let Some(prefix) = pattern.strip_suffix('*') {
        path.starts_with(prefix)
    } else {
        path == pattern
    }
}

fn is_litellm_ui_path(path: &str) -> bool {
    path == "/ui" || path.starts_with("/ui/")
}

fn is_litellm_ui_support_path(path: &str) -> bool {
    const UI_SUPPORT_EXACT: &[&str] = &[
        "/get_image",
        "/litellm/.well-known/litellm-ui-config",
        "/login",
        "/logout",
        "/models",
        "/user/info",
        "/v2/login",
    ];
    const UI_SUPPORT_PREFIXES: &[&str] = &[
        "/get/",
        "/litellm-asset-prefix/",
        "/model/",
        "/model_group/",
        "/public/",
    ];
    UI_SUPPORT_EXACT.contains(&path)
        || UI_SUPPORT_PREFIXES
            .iter()
            .any(|prefix| path.starts_with(prefix))
}

fn is_litellm_admin_path(path: &str) -> bool {
    const ADMIN_PREFIXES: &[&str] = &[
        "/key/",
        "/keys/",
        "/user/",
        "/team/",
        "/config/",
        "/spend/",
        "/global/",
        "/budget/",
        "/customer/",
        "/organization/",
    ];
    ADMIN_PREFIXES
        .iter()
        .any(|prefix| path == prefix.trim_end_matches('/') || path.starts_with(prefix))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn operator_only_sensitive_paths_are_routeable_but_classified() {
        let mut settings = LiteLlmPassthroughSettings::default_with_updated_at(Utc::now());
        settings.enabled = true;
        settings.allowed_paths = vec!["/v1/*".to_owned(), "/ui".to_owned(), "/key/*".to_owned()];
        settings.ui_exposure = LiteLlmSensitiveRouteExposure::OperatorOnly;
        settings.admin_api_exposure = LiteLlmSensitiveRouteExposure::OperatorOnly;

        assert!(settings.allows(&Method::GET, "/ui"));
        assert_eq!(
            settings.sensitive_exposure_for_path("/ui"),
            Some(LiteLlmSensitiveRouteExposure::OperatorOnly)
        );
        assert!(settings.allows(&Method::GET, "/key/list"));
        assert_eq!(
            settings.sensitive_exposure_for_path("/key/list"),
            Some(LiteLlmSensitiveRouteExposure::OperatorOnly)
        );
    }

    #[test]
    fn disabled_sensitive_paths_stay_blocked_even_when_allowlisted() {
        let mut settings = LiteLlmPassthroughSettings::default_with_updated_at(Utc::now());
        settings.enabled = true;
        settings.allowed_paths = vec!["/v1/*".to_owned(), "/ui".to_owned(), "/key/*".to_owned()];

        assert!(!settings.allows(&Method::GET, "/ui"));
        assert!(!settings.allows(&Method::GET, "/key/list"));
    }

    #[test]
    fn trusted_ingress_ui_exposure_only_bypasses_ui_browser_paths() {
        let mut settings = LiteLlmPassthroughSettings::default_with_updated_at(Utc::now());
        settings.enabled = true;
        settings.allowed_paths = vec![
            "/ui".to_owned(),
            "/ui/*".to_owned(),
            "/litellm-asset-prefix/*".to_owned(),
            "/v2/login".to_owned(),
            "/models".to_owned(),
            "/user/info".to_owned(),
            "/v1/*".to_owned(),
            "/key/*".to_owned(),
        ];
        settings.allowed_methods = vec!["GET".to_owned(), "POST".to_owned()];
        settings.ui_exposure = LiteLlmSensitiveRouteExposure::TrustedIngress;
        settings.admin_api_exposure = LiteLlmSensitiveRouteExposure::TrustedIngress;

        assert!(settings.trusted_ingress_ui_path_allowed(&Method::GET, "/ui"));
        assert!(settings
            .trusted_ingress_ui_path_allowed(&Method::GET, "/litellm-asset-prefix/index.js"));
        assert!(settings.trusted_ingress_ui_path_allowed(&Method::POST, "/v2/login"));
        assert!(settings.trusted_ingress_ui_path_allowed(&Method::GET, "/user/info"));
        assert!(!settings.trusted_ingress_ui_path_allowed(&Method::GET, "/v1/models"));
        assert!(!settings.trusted_ingress_ui_path_allowed(&Method::GET, "/key/list"));
        assert!(!settings.trusted_ingress_ui_path_allowed(&Method::DELETE, "/ui"));
    }

    #[test]
    fn trusted_ingress_is_rejected_for_admin_api_exposure() {
        let patch = LiteLlmPassthroughSettingsPatchRequest {
            enabled: None,
            allowed_paths: None,
            allowed_methods: None,
            ui_exposure: Some(LiteLlmSensitiveRouteExposure::TrustedIngress),
            admin_api_exposure: Some(LiteLlmSensitiveRouteExposure::TrustedIngress),
        };

        assert_eq!(
            patch
                .validate()
                .expect_err("admin trusted ingress rejected"),
            GatewayError::InvalidProviderConfigPayload
        );
    }
}
