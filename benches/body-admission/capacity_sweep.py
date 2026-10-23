#!/usr/bin/env python3
"""Find and repeat retained-body resource boundaries; preserve failed trials."""

import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import time


HERE = Path(__file__).resolve().parent
MIB = 1024 * 1024
GRIDS = {
    5: [32, 64, 96, 102, 128, 192, 256],
    10: [16, 32, 48, 51, 64, 96, 128],
    20: [8, 16, 24, 25, 32, 48, 64],
    40: [1, 2, 4, 8, 11, 12, 13, 16, 24, 32],
    80: [1, 2, 3, 4, 5, 6, 7, 8, 12, 16],
    100: [1, 2, 3, 4, 5, 6, 8, 10, 16],
}


def analyze(folder, slots, attempt, expected_body_bytes, cohort):
    folder = Path(folder)
    raw = folder / f"limit-{slots:02}-rep-1.json"
    row = {"slots": slots, "attempt": attempt, "directory": folder.name,
           "body_bytes": expected_body_bytes, "cohort_per_pod": cohort,
           "expected_requests": 2 * cohort}
    pods = json.loads(raw.with_suffix(".pods.json").read_text())["items"]
    assert len(pods) == 2, "expected exactly two Gateway pods"
    states = [state for pod in pods for state in pod["status"]["containerStatuses"]]
    row["restarts"] = sum(state["restartCount"] for state in states)
    row["oom_pods"] = sum(any(state.get(phase, {}).get("terminated", {}).get("reason") == "OOMKilled" for phase in ("state", "lastState")) for state in states)
    row["terminated_pods"] = sum(any(state.get(phase, {}).get("terminated") for phase in ("state", "lastState")) for state in states)
    row["pod_names"] = [pod["metadata"]["name"] for pod in pods]
    previous = [pod for pod in pods if (folder / f"{raw.stem}-{pod['metadata']['name']}-previous.log").exists()]
    row["previous_container_logs_pods"] = len(previous)
    row["snapshot_missed_restarts"] = sum(pod["status"]["containerStatuses"][0]["restartCount"] == 0 for pod in previous)
    for pod in pods:
        container = pod["spec"]["containers"][0]
        assert container["resources"]["limits"] == {"cpu": "2", "memory": "1Gi"}
        env = {item["name"]: item.get("value") for item in container["env"]}
        assert env["GATEWAY_MAX_BUFFERED_REQUESTS"] == str(slots)
        assert env["GATEWAY_MAX_INFLIGHT_BUFFER_BYTES"] == str(512 * MIB)
    memory = []
    for path in folder.glob(raw.stem + "-*.memory.txt"):
        samples = [[float(value) for value in line.split()] for line in path.read_text().splitlines() if len(line.split()) == 5]
        if samples:
            memory.append(samples)
    row["memory_streams"] = len(memory)
    row["rss_peak_mib"] = max((sample[2] for stream in memory for sample in stream), default=0) / MIB
    row["cgroup_peak_mib"] = max((sample[3] for stream in memory for sample in stream), default=0) / MIB
    row["max_pod_throttled_ms"] = max((max(0, stream[-1][4] - stream[0][4]) for stream in memory), default=0) / 1000
    row["resource_failure"] = bool(row["terminated_pods"] or row["restarts"] or previous)
    try:
        run = json.loads(raw.read_text())
    except (ValueError, OSError):
        run = {}
    row["warmup_failed"] = bool(run.get("warmup_failed"))
    row["incomplete"] = not run or raw.with_suffix(".failure.json").exists()
    row["memory_censored"] = row["resource_failure"] or row["memory_streams"] != 2
    row["clean"] = False
    row["resource_stable"] = False
    if row["incomplete"] or row["warmup_failed"]:
        row["warmup_results"] = run.get("warmup_results", [])
        return row
    assert run["body_bytes"] == expected_body_bytes
    assert run["scheduled_requests"] == run["completed_requests"] == len(run["results"]) == 2 * cohort
    assert run["verified_requests"] == run["successful_requests"]
    assert run["http_max_sockets_per_host"] >= cohort
    assert all(result["verified"] for result in run["results"] if result["status"] == 200)
    assert all(result.get("error_code") == "gateway_overloaded" for result in run["results"] if result["status"] == 503)
    for pod in run["pods"]:
        assert sum(result["pod"] == pod["name"] for result in run["results"]) == cohort
    samples = [sample for sample in run["samples"] if "error" not in sample]
    assert all(0 <= sample["buffered_requests"] <= slots and 0 <= sample["buffered_bytes"] <= 512 * MIB for sample in samples)
    row.update({"requests": run["scheduled_requests"], "successes": run["successful_requests"],
                "statuses": run["status_counts"], "body_sha256": run["body_sha256"],
                "elapsed_ms": run["elapsed_ms"], "success_latency_ms": run["success_latency_ms"],
                "rejection_latency_ms": run["rejection_latency_ms"], "dispatch_lag_ms": run["dispatch_lag_ms"],
                "metric_scrape_errors": len(run["samples"]) - len(samples),
                "buffered_requests_peak": max((sample["buffered_requests"] for sample in samples), default=0),
                "buffered_bytes_peak_mib": max((sample["buffered_bytes"] for sample in samples), default=0) / MIB,
                "transport_errors": sum(result["status"] == 0 for result in run["results"]),
                "other_http_statuses": sum(result["status"] not in (0, 200, 503) for result in run["results"]),
                "unexpected_results": [result for result in run["results"] if result["status"] not in (200, 503)]})
    row["concurrency_denials"] = row["byte_denials"] = None
    row["admission_counter_gap"] = None
    row["terminal_zero"] = False
    if not row["resource_failure"] and row["metric_scrape_errors"] == 0:
        for key in ("concurrency_denials", "byte_denials"):
            row[key] = sum(max(sample[key] for sample in samples if sample["pod"] == pod["name"]) - min(sample[key] for sample in samples if sample["pod"] == pod["name"]) for pod in run["pods"])
        # A reset can lose the rejection response while the server has already
        # recorded its denial. Keep that failed observation and its discrepancy
        # instead of aborting the resource search. Clean trials must reconcile.
        row["admission_counter_gap"] = row["concurrency_denials"] + row["byte_denials"] - run["status_counts"].get("503", 0)
        final = {pod["name"]: [sample for sample in samples if sample["pod"] == pod["name"]][-1] for pod in run["pods"]}
        row["terminal_zero"] = all(sample["buffered_requests"] == sample["buffered_bytes"] == 0 for sample in final.values())
    # Every offered request must reconcile to either a verified success or a
    # server admission denial. A lost denial response is reported separately
    # from resource stability; a missing admitted response remains invalid.
    row["resource_stable"] = not any((row["resource_failure"], row["metric_scrape_errors"], row["other_http_statuses"], row["memory_censored"])) and row["terminal_zero"] and row["admission_counter_gap"] == row["transport_errors"]
    row["clean"] = row["resource_stable"] and row["transport_errors"] == 0
    return row


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--kubeconfig", required=True)
    parser.add_argument("--image", required=True)
    parser.add_argument("--source-commit", required=True)
    parser.add_argument("--body-mib", type=int, choices=GRIDS, required=True)
    parser.add_argument("--output", required=True)
    parser.add_argument("--pilot-limits", nargs="+", type=int)
    parser.add_argument("--confirmation-repetitions", type=int, default=3)
    parser.add_argument("--max-search-slots", type=int, default=4096,
                        help="finite exploration bound; report a lower bound if no resource cliff appears")
    parser.add_argument("--fixed-limits", nargs="+", type=int, help="repeat exactly these slots without adaptive search")
    args = parser.parse_args(argv)
    root = Path(args.output)
    root.mkdir(parents=True, exist_ok=True)
    if (root / "summary.json").exists():
        raise RuntimeError("refusing to overwrite a completed sweep")
    capacity = 512 // args.body_mib
    grid = args.fixed_limits or args.pilot_limits or GRIDS[args.body_mib]
    if args.confirmation_repetitions < 1 or min(grid) < 1 or args.max_search_slots < max(grid):
        parser.error("positive repetitions/slots and a search bound at least as large as the grid are required")
    cohort = 2 * max(grid)
    rows = []
    summary = {"body_mib": args.body_mib, "body_bytes": args.body_mib * MIB,
               "full_body_arithmetic_bound_per_pod": capacity, "cohort_per_pod": cohort,
               "upstream_read_delay_ms": 10000, "post_upload_delay_ms": 2000,
               "image": args.image, "source_commit": args.source_commit,
               "confirmation_repetitions": args.confirmation_repetitions,
               "criterion": "resource_stability",
               "search_mode": "fixed" if args.fixed_limits else "adaptive",
               "max_search_slots": args.max_search_slots,
               "harness_sha256": {path.name: hashlib.sha256(path.read_bytes()).hexdigest() for path in (HERE / "capacity_sweep.py", HERE / "run.py", HERE / "mock.mjs", HERE / "workload.mjs")},
               "trials": rows, "complete": False}

    def save():
        temporary = root / "summary.json.tmp"
        temporary.write_text(json.dumps(summary, indent=2) + "\n")
        temporary.replace(root / "summary.json")

    def trial(slots, phase):
        attempt = 1 + sum(row["slots"] == slots for row in rows)
        folder = root / f"slots-{slots:04}-attempt-{attempt}"
        if folder.exists():
            raise RuntimeError(f"refusing to overwrite {folder}")
        print(f"{args.body_mib} MiB: {phase}: slots={slots}, attempt={attempt}", flush=True)
        command = [sys.executable, str(HERE / "run.py"), "--kubeconfig", args.kubeconfig,
                   "--image", args.image, "--source-commit", args.source_commit,
                   "--label", f"capacity-{args.body_mib}mib-slots{slots}-attempt{attempt}",
                   "--output", str(folder), "--body-mib", str(args.body_mib),
                   "--limits", str(slots), "--repetitions", "1", "--memory-limit-mib", "1024",
                   "--concurrent-per-pod", str(cohort), "--read-delay-ms", "10000",
                   "--delay-ms", "2000", "--client-deadline-ms", "60000",
                   "--condition-timeout", "150", "--retain-failures", "--host-metadata"]
        row = {"slots": slots, "attempt": attempt, "directory": folder.name,
               "body_bytes": args.body_mib * MIB, "cohort_per_pod": cohort,
               "phase": phase, "status": "running", "incomplete": True,
               "resource_failure": False, "resource_stable": False, "clean": False,
               "memory_censored": True}
        rows.append(row)
        save()
        try:
            with (root / f"{folder.name}.runner.log").open("w") as log:
                result = subprocess.run(command, stdout=log, stderr=subprocess.STDOUT, timeout=300)
            analyzed = analyze(folder, slots, attempt, args.body_mib * MIB, cohort)
            row.clear()
            row.update(analyzed, phase=phase, runner_exit_code=result.returncode, status="completed")
            if result.returncode:
                row["resource_stable"] = row["clean"] = False
        except BaseException as error:
            row.update(status="interrupted" if isinstance(error, KeyboardInterrupt) else "invalid",
                       analysis_error=f"{type(error).__name__}: {error}")
            raise
        finally:
            save()
        print(json.dumps({key: row.get(key) for key in ("slots", "attempt", "successes", "statuses", "rss_peak_mib", "cgroup_peak_mib", "buffered_requests_peak", "resource_failure", "resource_stable", "clean")}), flush=True)
        return row

    save()
    try:
        if args.fixed_limits:
            for slots in grid:
                for _ in range(args.confirmation_repetitions):
                    trial(slots, "fixed")
            summary.update(tested_limits=sorted(set(grid)), final_cohort_per_pod=cohort,
                           complete=True, completed_at_unix=time.time())
            return
        for slots in grid:
            trial(slots, "discovery")
            if len(rows) >= 2 and all(row["resource_failure"] for row in rows[-2:]):
                summary["discovery_stop"] = "two consecutive resource failures; higher counts untested"
                break
        # Do not mistake the offered cohort for an algorithmic limit. If the
        # entire grid survives, double both the tested ceiling and its offered
        # cohort. Refine and confirm within the final cohort, never across loads.
        while not args.pilot_limits and all(row["resource_stable"] for row in rows):
            ceiling = max(row["slots"] for row in rows)
            if ceiling >= args.max_search_slots:
                summary["expansion_stop"] = "finite search bound reached without a resource failure"
                break
            ceiling = min(2 * ceiling, args.max_search_slots)
            cohort = 2 * ceiling
            trial(ceiling, "expansion")
        current = lambda: [row for row in rows if row["cohort_per_pod"] == cohort]
        failures = sorted({row["slots"] for row in current() if row["resource_failure"]})
        if failures:
            high = failures[0]
            low = max((row["slots"] for row in current() if row["resource_stable"] and row["slots"] < high), default=0)
            while high - low > 1:
                middle = (high + low) // 2
                row = trial(middle, "refinement")
                if row["resource_stable"]:
                    low = middle
                elif row["resource_failure"]:
                    high = middle
                else:
                    summary["refinement_stop"] = "non-resource failure prevents monotonic refinement"
                    break
            selected = {value for value in (low, high) if value > 0}
        else:
            tested = sorted({row["slots"] for row in current()})
            selected = {min(tested, key=lambda value: abs(value - capacity)), tested[-1]}
            clean = [row["slots"] for row in current() if row["resource_stable"]]
            if clean:
                selected.add(max(clean))
        def confirm(slots):
            while sum(row["slots"] == slots for row in current()) < args.confirmation_repetitions:
                trial(slots, "confirmation")
            return all(row["resource_stable"] for row in current() if row["slots"] == slots)

        for slots in sorted(selected):
            confirm(slots)
        # A one-run candidate can fail confirmation, including a new failure at
        # an otherwise clean search ceiling. Refine with the repeated criterion
        # rather than a slow linear descent or accepting that single success.
        failures = sorted({row["slots"] for row in current() if row["resource_failure"]})
        if failures:
            high = failures[0]
            stable = [slots for slots in {row["slots"] for row in current()}
                      if slots < high
                      and sum(row["slots"] == slots for row in current()) >= args.confirmation_repetitions
                      and all(row["resource_stable"] for row in current() if row["slots"] == slots)]
            low = max(stable, default=0)
            while high - low > 1:
                middle = (high + low) // 2
                selected.add(middle)
                if confirm(middle):
                    low = middle
                elif any(row["resource_failure"] for row in current() if row["slots"] == middle):
                    high = middle
                else:
                    summary["confirmation_stop"] = "non-resource failure prevents repeated refinement"
                    break
            selected.add(high)
            confirm(high)
        summary["confirmed_limits"] = sorted(selected)
        summary["final_cohort_per_pod"] = cohort
        summary["complete"] = True
        summary["completed_at_unix"] = time.time()
        save()
    except BaseException as error:
        summary["error"] = f"{type(error).__name__}: {error}"
        raise
    finally:
        save()


if __name__ == "__main__":
    main()
