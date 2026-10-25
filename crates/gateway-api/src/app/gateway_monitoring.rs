//! Cluster observations are best effort and never consulted by the proxy.
use super::*;
use gateway_core::traffic::{TrafficActiveSnapshot, TrafficSummarySnapshot};
use gateway_telemetry::{merge_runtime_snapshots, runtime_snapshot, RuntimeSnapshot};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;
use uuid::Uuid;

const PUBLISH_INTERVAL: Duration = Duration::from_secs(1);
const IO_TIMEOUT: Duration = Duration::from_millis(750);
const STALE_AFTER_MS: i64 = 5_000;
const WINDOWS: [u64; 3] = [60, 300, 900];

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct DependencySnapshot {
    postgres_ready: Option<bool>,
    postgres_readiness_latency_ms: Option<u64>,
    postgres_pool_connections: u32,
    postgres_pool_idle: usize,
    redis_ready: Option<bool>,
    redis_readiness_latency_ms: Option<u64>,
    publication_failed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PublishedStats {
    instance_id: String,
    sampled_at: chrono::DateTime<Utc>,
    revision: u64,
    windows: Vec<RuntimeSnapshot>,
    traffic: TrafficSummarySnapshot,
    dependencies: DependencySnapshot,
}

#[derive(Clone)]
struct PublishedInstance {
    stats: PublishedStats,
    traffic: Option<TrafficActiveSnapshot>,
}

#[derive(Clone)]
struct Fleet {
    instances: Vec<PublishedInstance>,
    partial: bool,
    unavailable: bool,
}

struct CachedFleet {
    source: Uuid,
    instance_id: Option<Uuid>,
    attempted_at: Instant,
    fleet: Fleet,
}

static LOCAL: OnceLock<Mutex<Option<PublishedStats>>> = OnceLock::new();
static STATUS_CACHE: OnceLock<tokio::sync::Mutex<Option<CachedFleet>>> = OnceLock::new();
static TRAFFIC_CACHE: OnceLock<tokio::sync::Mutex<Option<CachedFleet>>> = OnceLock::new();

fn local_stats(dependencies: DependencySnapshot, revision: u64) -> PublishedStats {
    PublishedStats {
        instance_id: traffic::monitor().instance_id.clone(),
        sampled_at: Utc::now(),
        revision,
        windows: WINDOWS.into_iter().map(runtime_snapshot).collect(),
        traffic: traffic::monitor().summary_snapshot(),
        dependencies,
    }
}

/// One publisher per process. Snapshot construction, dependency probes, and
/// Redis writes happen on the control runtime, outside request forwarding.
pub(super) async fn run(redis: RedisReadiness, store: PostgresStore) {
    let publisher = redis.monitoring();
    let mut interval = tokio::time::interval(PUBLISH_INTERVAL);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut dependencies = DependencySnapshot::default();
    let mut revision = 0_u64;
    loop {
        interval.tick().await;
        revision = revision.wrapping_add(1);
        if revision % 5 == 1 {
            let pg_started = Instant::now();
            dependencies.postgres_ready = Some(matches!(
                tokio::time::timeout(IO_TIMEOUT, store.ready()).await,
                Ok(Ok(()))
            ));
            dependencies.postgres_readiness_latency_ms =
                Some(pg_started.elapsed().as_millis() as u64);
            let redis_started = Instant::now();
            dependencies.redis_ready = Some(matches!(
                tokio::time::timeout(IO_TIMEOUT, redis.ready()).await,
                Ok(Ok(()))
            ));
            dependencies.redis_readiness_latency_ms =
                Some(redis_started.elapsed().as_millis() as u64);
        }
        dependencies.postgres_pool_connections = store.pool().size();
        dependencies.postgres_pool_idle = store.pool().num_idle();
        let stats = local_stats(dependencies.clone(), revision);
        *LOCAL
            .get_or_init(|| Mutex::new(None))
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = Some(stats.clone());
        let mut active = traffic::monitor().publication_snapshot();
        // The registry has its own byte budget. The transport envelope also
        // needs space, so keep its serialized total below the Redis write cap.
        let traffic = loop {
            let value = serde_json::to_value(&active).expect("serializable active snapshot");
            if serde_json::to_vec(&value)
                .expect("serializable snapshot")
                .len()
                <= gateway_store::monitoring::MAX_MONITORING_TRAFFIC_BYTES
            {
                break value;
            }
            active.rows.truncate(active.rows.len() / 2);
            active.truncated = true;
            active.has_more = true;
        };
        let stats_value = serde_json::to_value(&stats).expect("serializable monitoring snapshot");
        let result = tokio::time::timeout(
            IO_TIMEOUT,
            publisher.publish(
                Uuid::parse_str(&stats.instance_id).expect("process UUID"),
                &stats_value,
                &traffic,
                stats.sampled_at,
            ),
        )
        .await;
        dependencies.publication_failed = !matches!(result, Ok(Ok(())));
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub(super) struct StatusQuery {
    instance_id: Option<String>,
    window_seconds: Option<u64>,
}

fn selection(instance: Option<&str>) -> GatewayResult<Option<Uuid>> {
    match instance {
        None | Some("all") | Some("") => Ok(None),
        Some(value) => Uuid::parse_str(value)
            .map(Some)
            .map_err(|_| GatewayError::InvalidUsageQuery),
    }
}

fn stale(sampled_at: chrono::DateTime<Utc>, now: chrono::DateTime<Utc>) -> bool {
    let age = (now - sampled_at).num_milliseconds();
    !(-STALE_AFTER_MS..=STALE_AFTER_MS).contains(&age)
}

async fn read_fleet(
    redis: &RedisReadiness,
    include_traffic: bool,
    instance_id: Option<Uuid>,
) -> Fleet {
    if instance_id.is_some_and(|id| traffic::monitor().instance_id == id.to_string()) {
        return with_local(
            Fleet {
                instances: Vec::new(),
                partial: false,
                unavailable: false,
            },
            include_traffic,
            instance_id,
        );
    }
    let cache = if include_traffic {
        &TRAFFIC_CACHE
    } else {
        &STATUS_CACHE
    };
    let mut cached = cache
        .get_or_init(|| tokio::sync::Mutex::new(None))
        .lock()
        .await;
    let source = redis.monitoring_identity();
    if let Some(value) = cached.as_ref().filter(|value| {
        value.source == source
            && value.instance_id == instance_id
            && value.attempted_at.elapsed() < PUBLISH_INTERVAL
    }) {
        return with_local(value.fleet.clone(), include_traffic, instance_id);
    }
    let result = tokio::time::timeout(
        IO_TIMEOUT,
        redis
            .monitoring()
            .snapshots(include_traffic, instance_id, Utc::now()),
    )
    .await;
    let fleet = match result {
        Ok(Ok(records)) => {
            let mut partial = records.truncated;
            let instances = records
                .records
                .into_iter()
                .filter_map(|record| {
                    let stats: PublishedStats =
                        match serde_json::from_value::<PublishedStats>(record.stats) {
                            Ok(value) if record.instance_id == value.instance_id => value,
                            _ => {
                                partial = true;
                                return None;
                            }
                        };
                    let traffic = match record
                        .traffic
                        .map(serde_json::from_value::<TrafficActiveSnapshot>)
                        .transpose()
                    {
                        Ok(value) => value,
                        Err(_) => {
                            partial = true;
                            None
                        }
                    };
                    Some(PublishedInstance { stats, traffic })
                })
                .collect();
            Fleet {
                instances,
                partial,
                unavailable: false,
            }
        }
        _ => {
            let mut previous = cached
                .as_ref()
                .filter(|value| value.source == source && value.instance_id == instance_id)
                .map(|value| value.fleet.clone())
                .unwrap_or(Fleet {
                    instances: Vec::new(),
                    partial: true,
                    unavailable: true,
                });
            previous.instances.retain(|instance| {
                (Utc::now() - instance.stats.sampled_at).num_milliseconds()
                    <= gateway_store::monitoring::MONITORING_LEASE_MS
            });
            previous.partial = true;
            previous.unavailable = true;
            previous
        }
    };
    *cached = Some(CachedFleet {
        source,
        instance_id,
        attempted_at: Instant::now(),
        fleet: fleet.clone(),
    });
    with_local(fleet, include_traffic, instance_id)
}

fn with_local(mut fleet: Fleet, include_traffic: bool, instance_id: Option<Uuid>) -> Fleet {
    if instance_id.is_some_and(|id| traffic::monitor().instance_id != id.to_string()) {
        return fleet;
    }
    let local = LOCAL
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
        .unwrap_or_else(|| local_stats(DependencySnapshot::default(), 0));
    fleet
        .instances
        .retain(|instance| instance.stats.instance_id != local.instance_id);
    fleet.instances.push(PublishedInstance {
        stats: local,
        traffic: include_traffic.then(|| traffic::monitor().publication_snapshot()),
    });
    fleet
}

fn private_json(value: impl Serialize) -> Response {
    let mut response = Json(value).into_response();
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-store"),
    );
    response
}

fn reader_permit() -> Result<tokio::sync::OwnedSemaphorePermit, GatewayError> {
    static READERS: OnceLock<Arc<tokio::sync::Semaphore>> = OnceLock::new();
    READERS
        .get_or_init(|| Arc::new(tokio::sync::Semaphore::new(4)))
        .clone()
        .try_acquire_owned()
        .map_err(|_| GatewayError::GatewayOverloaded)
}

pub(super) async fn status(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<StatusQuery>,
) -> Response {
    if let Err(response) = require_admin_scope(&state, &headers, SCOPE_USAGE_READ).await {
        return response;
    }
    let instance = match selection(query.instance_id.as_deref()) {
        Ok(value) => value,
        Err(error) => return error_response(&headers, error),
    };
    let window = query.window_seconds.unwrap_or(60);
    if !WINDOWS.contains(&window) {
        return error_response(&headers, GatewayError::InvalidUsageQuery);
    }
    let _permit = match reader_permit() {
        Ok(value) => value,
        Err(error) => return error_response(&headers, error),
    };
    let fleet = read_fleet(&state.redis, false, instance).await;
    let now = Utc::now();
    private_json(status_response(fleet, window, instance, now))
}

fn status_response(
    fleet: Fleet,
    window: u64,
    instance: Option<Uuid>,
    now: chrono::DateTime<Utc>,
) -> Value {
    let instances = fleet
        .instances
        .iter()
        .filter(|value| instance.is_none_or(|id| value.stats.instance_id == id.to_string()));
    let mut selected = Vec::new();
    let mut fresh = Vec::new();
    let mut partial = fleet.partial;
    for value in instances {
        let is_stale = stale(value.stats.sampled_at, now);
        partial |= is_stale || value.stats.dependencies.publication_failed;
        let runtime = value
            .stats
            .windows
            .iter()
            .find(|value| value.window_seconds == window)
            .cloned();
        partial |= runtime.is_none();
        if !is_stale {
            fresh.extend(runtime.clone());
        }
        selected.push(serde_json::json!({
            "instance_id": value.stats.instance_id, "sampled_at": value.stats.sampled_at,
            "revision": value.stats.revision, "stale": is_stale, "runtime": runtime,
            "dependencies": value.stats.dependencies,
        }));
    }
    partial |= selected.is_empty();
    let merged = merge_runtime_snapshots(&fresh);
    partial |= merged.incompatible_instances > 0;
    let aggregate = (merged.instance_count > 0).then_some(merged);
    serde_json::json!({
        "scope": if instance.is_some() { "instance" } else { "cluster" },
        "sampled_at": now, "partial": partial, "unavailable": fleet.unavailable,
        "window_seconds": window, "instances": selected,
        "aggregate": aggregate,
    })
}

#[derive(Debug, Default)]
struct ClusterTrafficQuery {
    traffic: TrafficQuery,
    instance_id: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
pub(super) struct InstanceQuery {
    instance_id: Option<String>,
}

pub(super) async fn cluster_traffic(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(traffic): Query<TrafficQuery>,
    Query(scope): Query<InstanceQuery>,
) -> Response {
    // Parse each flat URL query directly. serde(flatten) buffers values as
    // strings and prevents serde_urlencoded's numeric/bool conversion.
    let query = ClusterTrafficQuery {
        traffic,
        instance_id: scope.instance_id,
    };
    if let Err(response) = require_admin_scope(&state, &headers, SCOPE_USAGE_READ).await {
        return response;
    }
    if let Err(error) = query.traffic.validate() {
        return error_response(&headers, error);
    }
    let instance = match selection(query.instance_id.as_deref()) {
        Ok(value) => value,
        Err(error) => return error_response(&headers, error),
    };
    let _permit = match reader_permit() {
        Ok(value) => value,
        Err(error) => return error_response(&headers, error),
    };
    let fleet = read_fleet(&state.redis, true, instance).await;
    private_json(traffic_response(fleet, &query, instance, Utc::now()))
}

fn traffic_response(
    fleet: Fleet,
    query: &ClusterTrafficQuery,
    instance: Option<Uuid>,
    now: chrono::DateTime<Utc>,
) -> Value {
    let mut rows = Vec::new();
    let mut instances = Vec::new();
    let mut http = 0_u64;
    let mut websocket = 0_u64;
    let mut fresh_instances = 0_u64;
    let mut partial = fleet.partial;
    let mut truncated = false;
    for value in fleet
        .instances
        .into_iter()
        .filter(|value| instance.is_none_or(|id| value.stats.instance_id == id.to_string()))
    {
        let is_stale = stale(value.stats.sampled_at, now);
        partial |= is_stale || value.stats.dependencies.publication_failed;
        if !is_stale {
            fresh_instances += 1;
            http = http.saturating_add(value.stats.traffic.counts.http);
            websocket = websocket.saturating_add(value.stats.traffic.counts.websocket);
        }
        instances.push(serde_json::json!({
            "instance_id": value.stats.instance_id, "sampled_at": value.stats.sampled_at,
            "stale": is_stale, "revision": value.stats.revision, "summary": value.stats.traffic,
        }));
        if let Some(snapshot) = value.traffic {
            truncated |= snapshot.truncated || snapshot.has_more;
            rows.extend(
                snapshot
                    .rows
                    .into_iter()
                    .filter(|row| query.traffic.matches_active(row)),
            );
        } else {
            partial = true;
        }
    }
    partial |= instances.is_empty();
    rows.sort_unstable_by_key(|row| std::cmp::Reverse((row.started_at, row.id)));
    rows.dedup_by_key(|row| row.id);
    let matching = (!partial && !truncated).then_some(rows.len());
    let limit = query.traffic.limit.unwrap_or(100).clamp(1, 200) as usize;
    let has_more = rows.len() > limit;
    rows.truncate(limit);
    let before = rows.last().map(|row| row.started_at);
    let before_id = rows.last().map(|row| row.id);
    serde_json::json!({
        "scope": if instance.is_some() { "instance" } else { "cluster" },
        "sampled_at": now, "partial": partial, "unavailable": fleet.unavailable,
        "instances": instances, "rows": rows,
        "counts": {"http": (fresh_instances > 0).then_some(http), "websocket": (fresh_instances > 0).then_some(websocket)},
        "observed_instances": fresh_instances,
        "matching": matching, "has_more": has_more, "truncated": truncated,
        "before": before, "before_id": before_id,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use gateway_core::traffic::TrafficRequest;

    #[test]
    fn instance_selection_and_freshness_reject_ambiguity() {
        assert!(selection(Some("all")).unwrap().is_none());
        assert!(selection(Some("not-a-process-id")).is_err());
        let now = Utc::now();
        assert!(!stale(now, now));
        assert!(stale(now - ChronoDuration::seconds(6), now));
        assert!(stale(now + ChronoDuration::seconds(6), now));
    }

    #[test]
    fn cluster_filters_precede_limits_and_stale_pods_are_explicit() {
        let now = Utc::now();
        let project = Uuid::new_v4();
        let mut stats = local_stats(DependencySnapshot::default(), 1);
        stats.sampled_at = now - ChronoDuration::seconds(6);
        stats.traffic.counts.http = 2;
        let matching = TrafficRequest {
            project_id: Some(project),
            started_at: now - ChronoDuration::seconds(2),
            ..Default::default()
        };
        let unrelated = TrafficRequest {
            started_at: now,
            ..Default::default()
        };
        let snapshot = TrafficActiveSnapshot {
            summary: stats.traffic.clone(),
            rows: vec![unrelated, matching],
            has_more: false,
            truncated: false,
        };
        let fleet = Fleet {
            instances: vec![PublishedInstance {
                stats,
                traffic: Some(snapshot),
            }],
            partial: false,
            unavailable: false,
        };
        let query = ClusterTrafficQuery {
            traffic: TrafficQuery {
                project_id: Some(project),
                limit: Some(1),
                ..Default::default()
            },
            ..Default::default()
        };
        let result = traffic_response(fleet, &query, None, now);
        assert_eq!(result["rows"].as_array().unwrap().len(), 1);
        assert_eq!(result["rows"][0]["project_id"], project.to_string());
        assert_eq!(result["partial"], true);
        assert_eq!(result["instances"][0]["stale"], true);
        assert!(result["matching"].is_null());
        assert_eq!(result["has_more"], false);
        assert!(result["counts"]["http"].is_null());
        assert!(result["counts"]["websocket"].is_null());
    }

    #[test]
    fn incompatible_histograms_never_become_a_zero_fleet_aggregate() {
        let now = Utc::now();
        let mut stats = local_stats(DependencySnapshot::default(), 1);
        stats.windows[0].schema_version = 99;
        let fleet = Fleet {
            instances: vec![PublishedInstance {
                stats,
                traffic: None,
            }],
            partial: false,
            unavailable: false,
        };
        let result = status_response(fleet, 60, None, now);
        assert_eq!(result["partial"], true);
        assert!(result["aggregate"].is_null());
    }
}
