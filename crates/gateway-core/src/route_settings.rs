use crate::{GatewayResult, Route};
use async_trait::async_trait;
use chrono::{DateTime, Utc};

pub const CHAT_COMPLETIONS_ROUTE_ID: &str = "chat-completions";
pub const RESPONSES_ROUTE_ID: &str = "responses";

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct OpenAiRouteSetting {
    pub route_id: String,
    pub route: String,
    pub enabled: bool,
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
}

#[async_trait]
pub trait OpenAiRouteSettingsLookup: Send + Sync {
    async fn openai_route_enabled(&self, route: Route) -> GatewayResult<bool>;
}

#[async_trait]
impl<T> OpenAiRouteSettingsLookup for std::sync::Arc<T>
where
    T: OpenAiRouteSettingsLookup + ?Sized,
{
    async fn openai_route_enabled(&self, route: Route) -> GatewayResult<bool> {
        (**self).openai_route_enabled(route).await
    }
}

pub fn openai_route_id(route: Route) -> Option<&'static str> {
    match route {
        Route::ChatCompletions => Some(CHAT_COMPLETIONS_ROUTE_ID),
        Route::Responses => Some(RESPONSES_ROUTE_ID),
        _ => None,
    }
}

pub fn openai_route_from_id(route_id: &str) -> Option<Route> {
    match route_id {
        CHAT_COMPLETIONS_ROUTE_ID => Some(Route::ChatCompletions),
        RESPONSES_ROUTE_ID => Some(Route::Responses),
        _ => None,
    }
}
