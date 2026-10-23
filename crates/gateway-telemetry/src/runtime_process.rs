use crate::runtime::ProcessSnapshot;

#[cfg(target_os = "linux")]
pub(crate) fn sample_process() -> ProcessSnapshot {
    let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    let mut snapshot = ProcessSnapshot {
        rss_bytes: parse_rss(&status),
        ..ProcessSnapshot::default()
    };
    // Resolve this process's unified cgroup rather than using host-root counters.
    let cgroup = std::fs::read_to_string("/proc/self/cgroup").unwrap_or_default();
    let Some(relative) = cgroup.lines().find_map(|line| line.strip_prefix("0::")) else {
        return snapshot;
    };
    let root = std::path::Path::new("/sys/fs/cgroup");
    let relative = relative.trim_start_matches('/');
    if std::path::Path::new(relative)
        .components()
        .any(|component| {
            !matches!(
                component,
                std::path::Component::Normal(_) | std::path::Component::CurDir
            )
        })
    {
        return snapshot;
    }
    let directory = root.join(relative);
    let read = |name: &str| std::fs::read_to_string(directory.join(name)).ok();
    snapshot.cgroup_memory_current_bytes =
        read("memory.current").and_then(|value| value.trim().parse().ok());
    snapshot.cgroup_memory_limit_bytes =
        read("memory.max").and_then(|value| value.trim().parse().ok());
    let cpu = read("cpu.stat").unwrap_or_default();
    snapshot.cgroup_cpu_usage_usec = key_value(&cpu, "usage_usec");
    snapshot.cgroup_cpu_periods = key_value(&cpu, "nr_periods");
    snapshot.cgroup_cpu_throttled_periods = key_value(&cpu, "nr_throttled");
    snapshot.cgroup_cpu_throttled_usec = key_value(&cpu, "throttled_usec");
    snapshot.cgroup_cpu_limit_cores = read("cpu.max").and_then(|value| parse_cpu_limit(&value));
    let events = read("memory.events").unwrap_or_default();
    snapshot.cgroup_oom_events = key_value(&events, "oom");
    snapshot.cgroup_oom_kills = key_value(&events, "oom_kill");
    snapshot
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn sample_process() -> ProcessSnapshot {
    ProcessSnapshot::default()
}

#[cfg(any(target_os = "linux", test))]
fn parse_rss(status: &str) -> Option<u64> {
    let mut fields = status
        .lines()
        .find_map(|line| line.strip_prefix("VmRSS:"))?
        .split_whitespace();
    let value: u64 = fields.next()?.parse().ok()?;
    (fields.next()? == "kB")
        .then(|| value.checked_mul(1_024))
        .flatten()
}

#[cfg(any(target_os = "linux", test))]
fn key_value(content: &str, key: &str) -> Option<u64> {
    content.lines().find_map(|line| {
        let mut fields = line.split_whitespace();
        (fields.next()? == key)
            .then(|| fields.next()?.parse().ok())
            .flatten()
    })
}

#[cfg(any(target_os = "linux", test))]
fn parse_cpu_limit(content: &str) -> Option<f64> {
    let mut fields = content.split_whitespace();
    let quota: u64 = fields.next()?.parse().ok()?;
    let period: u64 = fields.next()?.parse().ok()?;
    (period != 0).then_some(quota as f64 / period as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proc_and_cgroup_values_keep_unknown_distinct_from_zero() {
        assert_eq!(parse_rss("Name: gateway\nVmRSS:\t123 kB\n"), Some(125_952));
        assert_eq!(parse_rss("VmSize: 999 kB"), None);
        assert_eq!(parse_rss("VmRSS: 123 bytes"), None);
        assert_eq!(key_value("oom 0\noom_kill 2\n", "oom"), Some(0));
        assert_eq!(key_value("oom 0\n", "oom_kill"), None);
        assert_eq!(parse_cpu_limit("200000 100000"), Some(2.0));
        assert_eq!(parse_cpu_limit("max 100000"), None);
        assert_eq!(parse_cpu_limit("200000 0"), None);
    }
}
