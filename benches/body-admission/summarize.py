#!/usr/bin/env python3
"""Summarize benchmark artifacts without third-party dependencies."""

import argparse
import json
import math
from pathlib import Path


MIB = 1024 * 1024


def percentile(values, p):
    ordered = sorted(values)
    return ordered[max(0, math.ceil(len(ordered) * p) - 1)] if ordered else None


def summarize(directory):
    directory = Path(directory)
    environment = json.loads((directory / "environment.json").read_text())
    groups = {}
    for path in sorted(directory.glob("limit-??-rep-?.json")):
        run = json.loads(path.read_text())
        memory = []
        for sample_path in directory.glob(path.stem + "-*.memory.txt"):
            rows = [[float(value) for value in line.split()] for line in sample_path.read_text().splitlines() if len(line.split()) == 5]
            if not rows:
                raise ValueError(f"no memory samples: {sample_path}")
            memory.append({
                "pod": sample_path.stem.removeprefix(path.stem + "-").removesuffix(".memory"),
                "sample_count": len(rows), "rss_peak_bytes": max(row[2] for row in rows),
                "cgroup_sampled_peak_bytes": max(row[1] for row in rows),
                "cgroup_kernel_peak_bytes": max(row[3] for row in rows),
                "cpu_throttled_usec": rows[-1][4] - rows[0][4],
            })
        if len(memory) != 2:
            raise ValueError(f"expected two memory streams: {path}")
        pods = json.loads(path.with_suffix(".pods.json").read_text())["items"]
        restarts = sum(container["restartCount"] for pod in pods for container in pod["status"]["containerStatuses"])
        oom = any(container.get("lastState", {}).get("terminated", {}).get("reason") == "OOMKilled" for pod in pods for container in pod["status"]["containerStatuses"])
        samples = [sample for sample in run["samples"] if "error" not in sample]
        if len(pods) != 2 or any(sample.get("error") for sample in run["samples"]):
            raise ValueError(f"invalid pod count or failed metrics scrape: {path}")
        run["memory"] = memory
        run["restarts"] = restarts
        run["oom"] = oom
        run["buffered_requests_peak"] = max(sample["buffered_requests"] for sample in samples)
        run["buffered_bytes_peak"] = max(sample["buffered_bytes"] for sample in samples)
        run["concurrency_denials"] = sum(max(sample["concurrency_denials"] for sample in samples if sample["pod"] == pod["name"]) - min(sample["concurrency_denials"] for sample in samples if sample["pod"] == pod["name"]) for pod in run["pods"])
        run["byte_denials"] = sum(max(sample["byte_denials"] for sample in samples if sample["pod"] == pod["name"]) - min(sample["byte_denials"] for sample in samples if sample["pod"] == pod["name"]) for pod in run["pods"])
        if run["body_bytes"] != 5 * MIB:
            raise ValueError(f"unexpected request size: {path}")
        if run["buffered_requests_peak"] > run["limit"] or run["buffered_bytes_peak"] > 512 * MIB:
            raise ValueError(f"admission bound exceeded: {path}")
        if run["status_counts"].get("503", 0) != run["concurrency_denials"] + run["byte_denials"]:
            raise ValueError(f"HTTP rejections do not match admission counters: {path}")
        for pod in run["pods"]:
            if sum(result["pod"] == pod["name"] for result in run["results"]) != run["scheduled_requests"] / 2:
                raise ValueError(f"uneven offered pod traffic: {path}")
        if any(result["status"] == 503 and result.get("error_code") != "gateway_overloaded" for result in run["results"]):
            raise ValueError(f"unexpected 503 source: {path}")
        groups.setdefault(run["limit"], []).append(run)
    if sorted(groups) != sorted(environment["limits"]):
        raise ValueError("incomplete limit matrix")
    conditions = []
    for limit, runs in sorted(groups.items()):
        if len(runs) != environment["repetitions"]:
            raise ValueError(f"incomplete repetitions for {limit}")
        results = [result for run in runs for result in run["results"]]
        successes = [result for result in results if result["status"] == 200]
        rejects = [result for result in results if result["status"] == 503]
        unexpected = [result for result in results if result["status"] not in (200, 503)]
        condition = {
            "label": environment["label"], "limit": limit, "repetitions": len(runs),
            "requests": len(results), "successes": len(successes), "rejections": len(rejects),
            "unexpected_results": len(unexpected),
            "success_percent": 100 * len(successes) / len(results),
            "success_rps_offered_window": len(successes) / sum(run["duration_seconds"] for run in runs),
            "success_rps_including_drain": len(successes) / sum(run["elapsed_ms"] / 1000 for run in runs),
            "success_p50_ms": percentile([result["ms"] for result in successes], 0.5),
            "success_p95_ms": percentile([result["ms"] for result in successes], 0.95),
            "success_p99_ms": percentile([result["ms"] for result in successes], 0.99),
            "success_ttfb_p95_ms": percentile([result["ttfb_ms"] for result in successes], 0.95),
            "rejection_p95_ms": percentile([result["ms"] for result in rejects], 0.95),
            "dispatch_lag_p99_ms": percentile([result["dispatch_lag_ms"] for result in results], 0.99),
            "rss_peak_mib": max(sample["rss_peak_bytes"] for run in runs for sample in run["memory"]) / MIB,
            "cgroup_peak_mib": max(sample["cgroup_kernel_peak_bytes"] for run in runs for sample in run["memory"]) / MIB,
            "cgroup_sampled_peak_mib": max(sample["cgroup_sampled_peak_bytes"] for run in runs for sample in run["memory"]) / MIB,
            "buffered_requests_peak": max(run["buffered_requests_peak"] for run in runs),
            "buffered_bytes_peak_mib": max(run["buffered_bytes_peak"] for run in runs) / MIB,
            "concurrency_denials": sum(run["concurrency_denials"] for run in runs),
            "byte_denials": sum(run["byte_denials"] for run in runs),
            "restarts": sum(run["restarts"] for run in runs), "oom": any(run["oom"] for run in runs),
            "max_pod_throttled_ms": max(sample["cpu_throttled_usec"] for run in runs for sample in run["memory"]) / 1000,
            "successes_per_repetition": [run["successful_requests"] for run in runs],
            "verified_successes": sum(run["verified_requests"] for run in runs),
        }
        if condition["verified_successes"] != condition["successes"]:
            raise ValueError(f"unverified response: {limit}")
        conditions.append(condition)
    return {"environment": environment, "conditions": conditions}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directories", nargs="+")
    parser.add_argument("--output", required=True)
    args = parser.parse_args()
    summaries = [summarize(directory) for directory in args.directories]
    if len(summaries) > 1:
        for key in ["limits", "repetitions", "rps", "duration_seconds", "delay_ms", "gateway_resources"]:
            if any(summary["environment"][key] != summaries[0]["environment"][key] for summary in summaries):
                raise ValueError(f"comparison settings differ: {key}")
    output = Path(args.output)
    output.write_text(json.dumps(summaries, indent=2) + "\n")
    print("| Version | Limit/pod | Success | Success RPS¹ | p50 ms | p95 ms | RSS peak MiB/pod | Cgroup peak MiB/pod | Peak slots/pod |")
    print("|---|---:|---:|---:|---:|---:|---:|---:|---:|")
    for summary in summaries:
        for row in summary["conditions"]:
            print(f"| {row['label']} | {row['limit']} | {row['success_percent']:.1f}% | {row['success_rps_offered_window']:.1f} | {row['success_p50_ms']:.1f} | {row['success_p95_ms']:.1f} | {row['rss_peak_mib']:.1f} | {row['cgroup_peak_mib']:.1f} | {row['buffered_requests_peak']:.0f} |")
    print("\n¹ Successes divided by offered-load duration; drain-inclusive throughput is retained in summary.json. Memory is the worst observed individual pod across all repetitions; latency pools successful requests only.")


if __name__ == "__main__":
    main()
