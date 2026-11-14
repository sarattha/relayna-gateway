"""Audit original 1.3.0 checkpoints; keep observations unchanged."""
import base64
import copy
import hashlib
import json
from pathlib import Path
import subprocess
import sys

REPO = Path.cwd()
ROOT = Path(__file__).resolve().parent
sys.path.insert(0, str(REPO / "benches/body-admission"))
import capacity_report as report
import capacity_sweep as sweep

COMMIT = "4d25895cb8d45ad24a6978d234f3bb7f96b2b177"
sizes, provenance, stats = [], [], []
for size in (5, 10, 20, 40, 80, 100):
    folder = ROOT / f"{size}mib"
    original = json.loads((folder / "report.json").read_text())
    run = original["run"]
    assert original["status"] == run["status"] == "completed"
    assert run["native_verified"] and run["architecture"] == "amd64"
    assert run["harness_commit"] == run["image"]["source_commit"] == COMMIT
    assert not run["harness_dirty"] and not run["errors"]
    assert run["resources"] == dict(pods=2, cpu_per_pod=2, memory_mib_per_pod=1024, body_budget_mib_per_pod=512)
    assert all(node["architecture"] == "amd64" for node in run["nodes"])
    for filename, expected in run["harness_sha256"].items():
        source = subprocess.check_output(["git", "show", f"{COMMIT}:benches/body-admission/{filename}"])
        assert hashlib.sha256(source).hexdigest() == expected
    selected = copy.deepcopy(original["sizes"][0])
    assert selected["complete"] and selected["stable_trials"] >= 3 and not selected["error"]
    for row in selected["trials"]:
        trial = folder / f"{size}mib" / row["directory"]
        actual = sweep.analyze(trial, row["slots"], row["attempt"], size * 1024**2, row["cohort_per_pod"])
        normalized = report.normalize(actual)
        for key in set(actual) & set(row):
            assert normalized.get(key, actual[key]) == row[key], (size, row["directory"], key)
        assert actual["resource_stable"] or actual["resource_failure"], (size, row["directory"], "unresolved non-resource failure")
        env = json.loads((trial / "environment.json").read_text())
        assert env["source_commit"] == COMMIT and env["image_id"] == run["image"]["id"]
        assert env["image_architecture"] == "amd64" and env["admission_environment"] == {}
    rows = [r for r in selected["trials"] if r["slots"] == selected["slots"] and r["cohort_per_pod"] == selected["cohort_per_pod"]]
    assert len(rows) >= 3 and all(r["resource_stable"] and not r["memory_censored"] for r in rows)
    pod_names = set()
    for row in rows:
        raw = folder / f"{size}mib" / row["directory"] / f"limit-{row['slots']:02}-rep-1.pods.json"
        names = {p["metadata"]["name"] for p in json.loads(raw.read_text())["items"]}
        assert len(names) == 2 and not names & pod_names
        pod_names |= names
    lower_failure = any(r["resource_failure"] and r["slots"] <= selected["slots"] and r["cohort_per_pod"] == selected["cohort_per_pod"] for r in selected["trials"])
    edge = selected["adjacent_trials"] >= 3 and selected["adjacent_oom_trials"] == selected["adjacent_trials"] and not lower_failure
    selected["original_label"] = selected["label"]
    selected["label"] = "confirmed resource edge" if edge else "tested lower bound"
    sizes.append(selected)
    provenance.append(run)
    stats.append({
        "body_mib": size, "trials": len(selected["trials"]), "slots": selected["slots"],
        "label": selected["label"], "stable_trials": len(rows),
        "http_clean_trials": sum(bool(r["clean"]) for r in rows),
        "occupied_peak": max(r["buffered_requests_peak"] for r in rows),
        "rss_peak_mib": selected["rss_peak_mib"], "cgroup_peak_mib": selected["cgroup_peak_mib"],
        "headroom_mib": selected["headroom_mib"],
        "success_latency_ms": {p: max(r["success_latency_ms"][p] for r in rows) for p in ("p50", "p95", "p99")},
        "rejection_latency_ms": {p: max(r["rejection_latency_ms"][p] for r in rows) for p in ("p50", "p95", "p99")},
        "elapsed_ms": max(r["elapsed_ms"] for r in rows),
        "success_range": [min(r["successes"] for r in rows), max(r["successes"] for r in rows)],
        "http_503_range": [min(r["statuses"].get("503",0) for r in rows), max(r["statuses"].get("503",0) for r in rows)],
        "http_429_range": [min(r["statuses"].get("429",0) for r in rows), max(r["statuses"].get("429",0) for r in rows)],
        "transport_errors": sum(r["transport_errors"] for r in rows),
        "adjacent_oom_trials": selected["adjacent_oom_trials"],
        "adjacent_trials": selected["adjacent_trials"], "adjacent_slots": selected["adjacent_slots"],
        "all_oom_trials": sum(bool(r["oom_pods"]) for r in selected["trials"]),
        "all_restarts": sum(r["restarts"] for r in selected["trials"]),
        "all_transport_errors": sum(r["transport_errors"] for r in selected["trials"]),
        "initial_cohort": selected["trials"][0]["cohort_per_pod"],
        "final_cohort": selected["cohort_per_pod"],
    })

publication = ROOT / "publication"
publication.mkdir(exist_ok=True)
audit = {"runtime_commit": COMMIT, "trials": sum(s["trials"] for s in stats),
         "method": "Reanalyze every original trial; validate caps, provenance, full-cost body verification, admission reconciliation and fresh confirmation pods. Require OOM in every adjacent trial for an edge; otherwise retain lower-bound label. Observations unchanged.",
         "sizes": stats, "provenance": provenance}
(publication / "gateway-1.3.0-audit.json").write_text(json.dumps(audit, indent=2)+"\n")
combined = {"schema_version":1,"status":"completed","run":{**provenance[0],"run_id":"v1.3.0-six-native-amd64-runs","body_mib":[5,10,20,40,80,100],"started_at":min(r["started_at"] for r in provenance),"finished_at":max(r["finished_at"] for r in provenance),"cluster":"Six isolated clusters; see per_size_provenance","image":{"source_commit":COMMIT,"architecture":"amd64","reference":"Six source-built images; see per_size_provenance","version":"Cargo 1.3.0; image labels unknown"},"preflight":{"architecture":"amd64","host":{"cpu_model":"Per-size hosts; see per_size_provenance"}},"nodes":[],"per_size_provenance":provenance},"sizes":sizes,"warnings":["Separate native hosts per size: see audit JSON. Original labels requiring only one adjacent OOM are conservatively corrected; numeric observations unchanged."]}
(publication / "run.json").write_text(json.dumps(combined["run"])+"\n")
report.collect = lambda root: combined
report.generate(publication)
font = base64.b64encode((REPO / "docs/assets/brand/fonts/Manrope-4.505.woff2").read_bytes()).decode()
page=(publication/"report.html").read_text()
page=page.replace("<style>", "<style>@font-face{font-family:Manrope;src:url(data:font/woff2;base64,"+font+") format('woff2');font-weight:200 800}body{--font-sans:Manrope,system-ui}",1)
page=page.replace("a repeated adjacent setting with an observed OOM", "at least three adjacent trials, each with an observed OOM, with no contradictory lower-slot resource failure")
page=page.replace("Gateway body benchmark", "Gateway 1.3.0 body benchmark")
page=page.replace('href="report.json"','href="gateway-1.3.0-benchmarks.json"').replace('href="trials.csv"','href="gateway-1.3.0-trials.csv"').replace('href="run.json"','href="gateway-1.3.0-audit.json"').replace('Run checkpoint</a>','Audit and provenance</a>').replace('href="provision.log"','href="gateway-1.3.0-resource-evidence.tar.gz"').replace('Setup log</a>','Original raw evidence</a>')
(publication / "gateway-1.3.0-benchmarks.html").write_text(page)
(publication / "report.json").rename(publication / "gateway-1.3.0-benchmarks.json")
(publication / "trials.csv").rename(publication / "gateway-1.3.0-trials.csv")
(publication / "report.html").unlink()
(publication / "run.json").unlink()
print(json.dumps({"trials":audit["trials"],"sizes":stats},indent=2))
