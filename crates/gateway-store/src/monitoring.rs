//! Expiring, bounded observation snapshots. These keys never participate in admission.
use chrono::{DateTime, Utc};
use gateway_core::{GatewayError, GatewayResult};
use serde_json::Value;
use uuid::Uuid;

const KEY_PREFIX: &str = "gateway:monitor:v1:";
pub const MONITORING_LEASE_MS: i64 = 120_000;
pub const MAX_MONITORING_INSTANCES: usize = 64;
pub const MAX_MONITORING_STATS_BYTES: usize = 256 * 1024;
pub const MAX_MONITORING_TRAFFIC_BYTES: usize = 4 * 1024 * 1024;
const MAX_MONITORING_READ_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone)]
pub struct RedisMonitoringStore {
    client: redis::Client,
    prefix: String,
}

pub struct MonitoringRecord {
    pub instance_id: String,
    pub stats: Value,
    pub traffic: Option<Value>,
}

pub struct MonitoringRecords {
    pub records: Vec<MonitoringRecord>,
    pub truncated: bool,
}

impl RedisMonitoringStore {
    pub(crate) fn from_client(client: redis::Client) -> Self {
        Self {
            client,
            prefix: KEY_PREFIX.to_owned(),
        }
    }

    pub async fn publish(
        &self,
        instance_id: Uuid,
        stats: &Value,
        traffic: &Value,
        now: DateTime<Utc>,
    ) -> GatewayResult<()> {
        let stats = bounded_json(stats, MAX_MONITORING_STATS_BYTES)?;
        let traffic = bounded_json(traffic, MAX_MONITORING_TRAFFIC_BYTES)?;
        let mut connection = self
            .client
            .get_multiplexed_async_connection()
            .await
            .map_err(|_| GatewayError::ControlStateUnavailable)?;
        let _: i64 = redis::Script::new(
            r#"
            redis.call('ZREMRANGEBYSCORE', KEYS[1], '-inf', ARGV[2] - ARGV[3])
            if not redis.call('ZSCORE', KEYS[1], ARGV[1]) and redis.call('ZCARD', KEYS[1]) >= tonumber(ARGV[4]) then
                return redis.error_reply('monitoring instance capacity reached')
            end
            redis.call('SET', KEYS[2], ARGV[5], 'PX', ARGV[3])
            redis.call('SET', KEYS[3], ARGV[6], 'PX', ARGV[3])
            redis.call('ZADD', KEYS[1], ARGV[2], ARGV[1])
            redis.call('PEXPIRE', KEYS[1], ARGV[3] * 2)
            return 1
            "#,
        )
        .key(format!("{}instances", self.prefix))
        .key(format!("{}{instance_id}:stats", self.prefix))
        .key(format!("{}{instance_id}:traffic", self.prefix))
        .arg(instance_id.to_string())
        .arg(now.timestamp_millis())
        .arg(MONITORING_LEASE_MS)
        .arg(MAX_MONITORING_INSTANCES)
        .arg(stats)
        .arg(traffic)
        .invoke_async(&mut connection)
        .await
        .map_err(|_| GatewayError::ControlStateUnavailable)?;
        Ok(())
    }

    /// Read through a Lua byte budget so a slow viewer cannot allocate an
    /// unbounded fleet of snapshots, even when many instances are registered.
    pub async fn snapshots(
        &self,
        include_traffic: bool,
        instance_id: Option<Uuid>,
        now: DateTime<Utc>,
    ) -> GatewayResult<MonitoringRecords> {
        let mut connection = self
            .client
            .get_multiplexed_async_connection()
            .await
            .map_err(|_| GatewayError::ControlStateUnavailable)?;
        let values: Vec<String> = redis::Script::new(
            r#"
            redis.call('ZREMRANGEBYSCORE', KEYS[1], '-inf', ARGV[1] - ARGV[2])
            local ids = redis.call('ZREVRANGE', KEYS[1], 0, tonumber(ARGV[3]) - 1)
            if ARGV[7] ~= '' then ids = {ARGV[7]} end
            local result = {'0'}
            local used = 0
            for _, id in ipairs(ids) do
                local sk = ARGV[4] .. id .. ':stats'
                local tk = ARGV[4] .. id .. ':traffic'
                local sl = redis.call('STRLEN', sk)
                local tl = 0
                if ARGV[5] == '1' then tl = redis.call('STRLEN', tk) end
                if sl > 0 and used + sl + tl <= tonumber(ARGV[6]) then
                    table.insert(result, id)
                    table.insert(result, redis.call('GET', sk) or '')
                    table.insert(result, ARGV[5] == '1' and (redis.call('GET', tk) or '') or '')
                    used = used + sl + tl
                else result[1] = '1' end
            end
            return result
            "#,
        )
        .key(format!("{}instances", self.prefix))
        .arg(now.timestamp_millis())
        .arg(MONITORING_LEASE_MS)
        .arg(MAX_MONITORING_INSTANCES)
        .arg(&self.prefix)
        .arg(if include_traffic { "1" } else { "0" })
        .arg(MAX_MONITORING_READ_BYTES)
        .arg(instance_id.map(|id| id.to_string()).unwrap_or_default())
        .invoke_async(&mut connection)
        .await
        .map_err(|_| GatewayError::ControlStateUnavailable)?;
        let mut truncated = values.first().is_none_or(|value| value == "1");
        let mut records = Vec::new();
        for row in values.get(1..).unwrap_or_default().as_chunks::<3>().0 {
            let stats = serde_json::from_str::<Value>(&row[1]);
            let traffic = if row[2].is_empty() {
                Ok(None)
            } else {
                serde_json::from_str::<Value>(&row[2]).map(Some)
            };
            match (Uuid::parse_str(&row[0]), stats, traffic) {
                (Ok(_), Ok(stats), Ok(traffic)) => records.push(MonitoringRecord {
                    instance_id: row[0].clone(),
                    stats,
                    traffic,
                }),
                _ => truncated = true,
            }
        }
        Ok(MonitoringRecords { records, truncated })
    }
}

fn bounded_json(value: &Value, maximum: usize) -> GatewayResult<String> {
    let encoded =
        serde_json::to_string(value).map_err(|_| GatewayError::ControlStateUnavailable)?;
    if encoded.len() > maximum {
        return Err(GatewayError::GatewayOverloaded);
    }
    Ok(encoded)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn snapshot_serialization_enforces_byte_limits() {
        assert!(bounded_json(&json!({"message": "x".repeat(40)}), 32).is_err());
        assert!(bounded_json(&json!({"ok": true}), 32).is_ok());
    }

    #[tokio::test]
    async fn redis_snapshots_are_atomic_isolated_bounded_and_expire_from_discovery() {
        let Ok(url) = std::env::var("REDIS_URL") else {
            eprintln!("skipping monitoring Redis integration: REDIS_URL is not set");
            return;
        };
        let store = RedisMonitoringStore {
            client: redis::Client::open(url).expect("test Redis client"),
            prefix: format!("gateway:monitor:test:{}:", Uuid::new_v4()),
        };
        let first = Uuid::new_v4();
        let second = Uuid::new_v4();
        let third = Uuid::new_v4();
        let now = Utc::now();
        let traffic = json!({"rows": [], "padding": "x".repeat(3 * 1024 * 1024)});
        for instance in [first, second, third] {
            store
                .publish(instance, &json!({"revision": 1}), &traffic, now)
                .await
                .unwrap();
        }
        let selected = store.snapshots(true, Some(first), now).await.unwrap();
        assert!(!selected.truncated);
        assert_eq!(selected.records.len(), 1);
        assert_eq!(selected.records[0].instance_id, first.to_string());
        assert_eq!(selected.records[0].stats["revision"], 1);
        assert!(selected.records[0].traffic.is_some());
        let fleet = store.snapshots(true, None, now).await.unwrap();
        assert!(fleet.truncated);
        assert_eq!(fleet.records.len(), 2);
        let stats = store.snapshots(false, None, now).await.unwrap();
        assert!(!stats.truncated);
        assert_eq!(stats.records.len(), 3);
        assert!(stats.records.iter().all(|record| record.traffic.is_none()));
        store
            .publish(first, &json!({"revision": 2}), &json!({"rows": []}), now)
            .await
            .unwrap();
        let updated = store.snapshots(true, Some(first), now).await.unwrap();
        assert_eq!(updated.records[0].stats["revision"], 2);
        assert_eq!(
            updated.records[0].traffic.as_ref().unwrap()["rows"],
            json!([])
        );
        let expired = store
            .snapshots(
                false,
                None,
                now + chrono::Duration::milliseconds(MONITORING_LEASE_MS + 1),
            )
            .await
            .unwrap();
        assert!(expired.records.is_empty());
        let mut keys = vec![format!("{}instances", store.prefix)];
        for instance in [first, second, third] {
            keys.push(format!("{}{instance}:stats", store.prefix));
            keys.push(format!("{}{instance}:traffic", store.prefix));
        }
        let mut connection = store
            .client
            .get_multiplexed_async_connection()
            .await
            .unwrap();
        redis::cmd("DEL")
            .arg(keys)
            .query_async::<usize>(&mut connection)
            .await
            .unwrap();
    }
}
