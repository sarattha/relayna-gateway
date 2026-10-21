#!/usr/bin/env python3
"""Describe overload runs, explicitly retaining crashes and censored timing."""

import argparse
import json
from pathlib import Path


def summarize(directory):
    directory = Path(directory)
    environment = json.loads((directory / "environment.json").read_text())
    conditions = []
    for path in sorted(directory.glob("limit-??-rep-?.json")):
        run = json.loads(path.read_text())
        pods = json.loads(path.with_suffix(".pods.json").read_text())["items"]
        statuses = [status for pod in pods for status in pod["status"]["containerStatuses"]]
        oom_pods = sum(any(status.get(state, {}).get("terminated", {}).get("reason") == "OOMKilled" for state in ["lastState", "state"]) for status in statuses)
        memory = []
        for sample in directory.glob(path.stem + "-*.memory.txt"):
            rows = [[float(value) for value in line.split()] for line in sample.read_text().splitlines() if len(line.split()) == 5]
            if not rows:
                raise ValueError(f"no memory samples: {sample}")
            memory.extend(rows)
        if len(pods) != 2 or run["body_bytes"] != 5 * 1024 * 1024:
            raise ValueError(f"invalid experiment: {path}")
        if run["completed_requests"] != run["scheduled_requests"] or run["verified_requests"] != run["successful_requests"]:
            raise ValueError(f"incomplete or unverified client results: {path}")
        conditions.append({
            "limit": run["limit"], "repetition": run["repetition"],
            "requests": run["scheduled_requests"], "successes": run["successful_requests"],
            "overload_503s": sum(result["status"] == 503 for result in run["results"]),
            "transport_errors": sum(result["status"] == 0 for result in run["results"]),
            "other_http_statuses": sum(result["status"] not in (0, 200, 503) for result in run["results"]),
            "oom_pods": oom_pods, "restarts": sum(status["restartCount"] for status in statuses),
            "rss_observed_peak_mib": max(row[2] for row in memory) / 1024 ** 2,
            "cgroup_observed_peak_mib": max(row[3] for row in memory) / 1024 ** 2,
            "timing_censored": bool(oom_pods or any(result["status"] == 0 for result in run["results"])),
            "success_p95_ms": run["success_latency_ms"]["p95"] if run["success_latency_ms"] else None,
            "dispatch_lag_p99_ms": run["dispatch_lag_ms"]["p99"],
            "elapsed_ms_including_drain": run["elapsed_ms"],
            "metric_scrape_errors": sum("error" in sample for sample in run["samples"]),
            "body_sha256": run["body_sha256"],
        })
    if len(conditions) != len(environment["limits"]) * environment["repetitions"]:
        raise ValueError("incomplete overload sweep")
    return {"environment": environment, "conditions": conditions}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directories", nargs="+")
    parser.add_argument("--output", required=True)
    args = parser.parse_args()
    summaries = [summarize(directory) for directory in args.directories]
    for key in ["limits", "repetitions", "rps", "duration_seconds", "delay_ms", "gateway_resources", "malloc_mmap_threshold_override"]:
        if any(summary["environment"].get(key) != summaries[0]["environment"].get(key) for summary in summaries):
            raise ValueError(f"probe controls differ: {key}")
    if len({condition["body_sha256"] for summary in summaries for condition in summary["conditions"]}) != 1:
        raise ValueError("probe payloads differ")
    Path(args.output).write_text(json.dumps(summaries, indent=2) + "\n")
    print("| Version | Slots/pod | 200 / offered | 503 | Transport errors | OOM pods | RSS observed MiB/pod | Success p95 ms |")
    print("|---|---:|---:|---:|---:|---:|---:|---:|")
    for summary in summaries:
        for row in summary["conditions"]:
            p95 = "censored" if row["timing_censored"] else f"{row['success_p95_ms']:.1f}" if row["success_p95_ms"] is not None else "—"
            print(f"| {summary['environment']['label']} | {row['limit']} | {row['successes']} / {row['requests']} | {row['overload_503s']} | {row['transport_errors']} | {row['oom_pods']} | {row['rss_observed_peak_mib']:.1f} | {p95} |")


if __name__ == "__main__":
    main()
