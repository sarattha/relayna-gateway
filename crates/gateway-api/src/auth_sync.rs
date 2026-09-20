//! Reconcile persisted front-door authentication without restarting replicas.
use gateway_core::{AdminGatewayAuthSettingsStore, GatewayAuthEnv, SharedGatewayAuthRuntime};
use std::time::Duration;
use tokio::time::{interval, timeout, MissedTickBehavior};

const REFRESH_INTERVAL: Duration = Duration::from_secs(5);
const REFRESH_TIMEOUT: Duration = Duration::from_secs(3);

pub async fn run_gateway_auth_refresh(
    store: impl AdminGatewayAuthSettingsStore,
    env: GatewayAuthEnv,
    runtime: SharedGatewayAuthRuntime,
) {
    let mut ticks = interval(REFRESH_INTERVAL);
    ticks.set_missed_tick_behavior(MissedTickBehavior::Skip);
    loop {
        ticks.tick().await;
        match timeout(REFRESH_TIMEOUT, runtime.refresh_from_store(&store, &env)).await {
            Ok(Ok(_)) => {}
            Ok(Err(error)) => {
                let error_code = error.code();
                tracing::warn!(error_code, "auth refresh failed; keeping config");
            }
            Err(_) => tracing::warn!(
                "gateway authentication refresh timed out; retaining last valid configuration"
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use gateway_core::{
        GatewayAuthSettingsPatchRequest, GatewayError, GatewayResult, StoredGatewayAuthSettings,
    };
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    struct RecoveringStore(AtomicUsize);
    #[async_trait]
    impl AdminGatewayAuthSettingsStore for RecoveringStore {
        async fn gateway_auth_settings(&self) -> GatewayResult<Option<StoredGatewayAuthSettings>> {
            match self.0.fetch_add(1, Ordering::SeqCst) {
                0 => Err(GatewayError::StoreUnavailable),
                1 => std::future::pending().await,
                _ => Ok(Some(StoredGatewayAuthSettings {
                    relayna_key_header: Some("x-recovered-key".into()),
                    ..Default::default()
                })),
            }
        }
        async fn patch_gateway_auth_settings(
            &self,
            _: GatewayAuthSettingsPatchRequest,
        ) -> GatewayResult<StoredGatewayAuthSettings> {
            unreachable!()
        }
    }
    #[tokio::test]
    async fn worker_retries_errors_and_timeouts_without_clearing_runtime() {
        gateway_telemetry::init("warn", false);
        let runtime = SharedGatewayAuthRuntime::new(gateway_core::GatewayAuthRuntimeConfig {
            relayna_key_header: "x-existing-key".into(),
            ..Default::default()
        })
        .unwrap();
        let store = Arc::new(RecoveringStore(AtomicUsize::new(0)));
        let worker = tokio::spawn(run_gateway_auth_refresh(
            store.clone(),
            GatewayAuthEnv::default(),
            runtime.clone(),
        ));
        timeout(Duration::from_secs(15), async {
            loop {
                let header = runtime.snapshot().unwrap().config.relayna_key_header;
                if header == "x-recovered-key" {
                    break;
                }
                assert_eq!(header, "x-existing-key");
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        assert!(store.0.load(Ordering::SeqCst) >= 3);
        worker.abort();
        assert!(worker.await.unwrap_err().is_cancelled());
    }
}
