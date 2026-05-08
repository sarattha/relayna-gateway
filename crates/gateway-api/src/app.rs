use async_trait::async_trait;
use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use gateway_core::{auth::VirtualKeyLookup, GatewayError, GatewayResult, UsageEvent};
use gateway_store::{PostgresStore, RedisReadiness};
use serde::Serialize;
use std::sync::Arc;
use tower_http::{
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer},
    trace::TraceLayer,
};

#[async_trait]
pub trait GatewayData: VirtualKeyLookup + Send + Sync {
    async fn insert_usage_event(&self, event: &UsageEvent) -> GatewayResult<()>;
    async fn postgres_ready(&self) -> GatewayResult<()>;
}

#[async_trait]
impl GatewayData for PostgresStore {
    async fn insert_usage_event(&self, event: &UsageEvent) -> GatewayResult<()> {
        PostgresStore::insert_usage_event(self, event).await
    }

    async fn postgres_ready(&self) -> GatewayResult<()> {
        self.ready()
            .await
            .map_err(|_| GatewayError::StoreUnavailable)
    }
}

#[derive(Clone)]
pub struct AppState {
    store: Arc<dyn GatewayData>,
    redis: RedisReadiness,
}

pub fn router(store: PostgresStore, redis: RedisReadiness) -> Router {
    router_with_state(AppState {
        store: Arc::new(store),
        redis,
    })
}

pub fn router_with_state(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz))
        .layer(PropagateRequestIdLayer::x_request_id())
        .layer(SetRequestIdLayer::x_request_id(MakeRequestUuid))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

async fn healthz() -> impl IntoResponse {
    Json(StatusBody { status: "ok" })
}

async fn readyz(State(state): State<AppState>) -> Response {
    let postgres = state.store.postgres_ready().await;
    let redis = state
        .redis
        .ready()
        .await
        .map_err(|_| GatewayError::StoreUnavailable);

    match (postgres, redis) {
        (Ok(()), Ok(())) => Json(StatusBody { status: "ready" }).into_response(),
        _ => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(StatusBody {
                status: "not_ready",
            }),
        )
            .into_response(),
    }
}

#[derive(Debug, Serialize)]
struct StatusBody {
    status: &'static str,
}

#[cfg(test)]
mod tests {
    use super::*;
    use gateway_core::auth::StoredVirtualKey;
    use std::sync::Mutex;
    use tower::ServiceExt;
    use uuid::Uuid;

    #[derive(Clone)]
    struct MemoryStore {
        key: Arc<Mutex<Option<StoredVirtualKey>>>,
        events: Arc<Mutex<Vec<UsageEvent>>>,
        postgres_ready: bool,
    }

    #[async_trait]
    impl VirtualKeyLookup for MemoryStore {
        async fn find_by_prefix(&self, _prefix: &str) -> GatewayResult<Option<StoredVirtualKey>> {
            Ok(self.key.lock().expect("lock poisoned").clone())
        }
    }

    #[async_trait]
    impl GatewayData for MemoryStore {
        async fn insert_usage_event(&self, event: &UsageEvent) -> GatewayResult<()> {
            self.events
                .lock()
                .expect("lock poisoned")
                .push(event.clone());
            Ok(())
        }

        async fn postgres_ready(&self) -> GatewayResult<()> {
            if self.postgres_ready {
                Ok(())
            } else {
                Err(GatewayError::StoreUnavailable)
            }
        }
    }

    fn stored_key(raw: &str) -> StoredVirtualKey {
        StoredVirtualKey {
            id: Uuid::new_v4(),
            project_id: Uuid::new_v4(),
            key_prefix: raw.chars().take(16).collect(),
            key_hash: "not-used-by-control-api".to_owned(),
            disabled: false,
            expires_at: None,
        }
    }

    fn test_state(store: MemoryStore) -> AppState {
        test_state_with_redis_url(store, "redis://127.0.0.1:6379")
    }

    fn test_state_with_redis_url(store: MemoryStore, redis_url: &str) -> AppState {
        let redis = RedisReadiness::new(redis_url).expect("redis client");
        AppState {
            store: Arc::new(store),
            redis,
        }
    }

    async fn request(app: Router, route: &str) -> Response {
        app.oneshot(
            axum::http::Request::builder()
                .method(axum::http::Method::GET)
                .uri(route)
                .header("x-request-id", "req_test")
                .body(axum::body::Body::empty())
                .expect("request"),
        )
        .await
        .expect("response")
    }

    #[tokio::test]
    async fn healthz_returns_ok() {
        let store = MemoryStore {
            key: Arc::new(Mutex::new(None)),
            events: Arc::new(Mutex::new(Vec::new())),
            postgres_ready: true,
        };

        let app = router_with_state(test_state_with_redis_url(store, "redis://127.0.0.1:0"));
        let response = request(app, "/healthz").await;

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn generation_routes_are_not_served_by_axum_control_api() {
        let raw = "rk_live_1234567890abcdef";
        let store = MemoryStore {
            key: Arc::new(Mutex::new(Some(stored_key(raw)))),
            events: Arc::new(Mutex::new(Vec::new())),
            postgres_ready: true,
        };

        let app = router_with_state(test_state(store.clone()));
        let response = request(app, "/v1/chat/completions").await;

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert!(store.events.lock().expect("lock poisoned").is_empty());
    }

    #[tokio::test]
    async fn readyz_returns_unavailable_when_store_is_not_ready() {
        let raw = "rk_live_1234567890abcdef";
        let store = MemoryStore {
            key: Arc::new(Mutex::new(Some(stored_key(raw)))),
            events: Arc::new(Mutex::new(Vec::new())),
            postgres_ready: false,
        };

        let app = router_with_state(test_state(store));
        let response = request(app, "/readyz").await;

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }
}
