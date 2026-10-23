#!/usr/bin/env python3
"""Disposable, production-shaped Usage read benchmark; standard library only.

Requires an already migrated, explicitly named local monitoring test database.
Credentials are read from a private file and never included in artifacts.
"""
import argparse
import concurrent.futures
import datetime as dt
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import re
import subprocess
import time
import urllib.error
import urllib.parse
import urllib.request


def sql(args, statement):
    result = subprocess.run(
        ["docker", "exec", "--interactive", args.container, "psql", "--username",
         "relayna_test", "--dbname", "relayna_monitoring", "-v", "ON_ERROR_STOP=1", "-Atq"],
        input=statement, capture_output=True, text=True, check=True,
    )
    return result.stdout.strip()


def seed(args, count):
    sql(args, """
    CREATE TABLE IF NOT EXISTS monitoring_scale_state (
        singleton boolean PRIMARY KEY DEFAULT true CHECK(singleton),
        event_count bigint NOT NULL, anchor timestamptz NOT NULL
    );
    INSERT INTO monitoring_scale_state VALUES (true, 0, now()) ON CONFLICT DO NOTHING;
    INSERT INTO projects(id, name)
    SELECT md5('monitoring-project-' || n)::uuid, 'Monitoring project ' || n
    FROM generate_series(1,30) n ON CONFLICT DO NOTHING;
    INSERT INTO api_keys(id, project_id, key_prefix, key_hash)
    SELECT md5('monitoring-key-' || n)::uuid, md5('monitoring-project-' || n)::uuid,
           'scale-history-' || n, 'non-authenticating-history-fixture'
    FROM generate_series(1,30) n ON CONFLICT DO NOTHING;
    INSERT INTO service_registrations(name, route_pattern)
    SELECT 'monitoring-service-' || n, '/services/monitoring-service-' || n || '/*'
    FROM generate_series(1,20) n ON CONFLICT DO NOTHING;
    INSERT INTO project_service_links(project_id,service_name)
    SELECT md5('monitoring-project-' || p)::uuid,
           'monitoring-service-' || ((p + s) % 20 + 1)
    FROM generate_series(1,30) p CROSS JOIN generate_series(0,4) s
    ON CONFLICT DO NOTHING;
    """)
    current = int(sql(args, "SELECT event_count FROM monitoring_scale_state"))
    if current > count:
        raise ValueError("fixture already exceeds cohort; use a fresh disposable database")
    for start in range(current + 1, count + 1, 100_000):
        end = min(start + 99_999, count)
        sql(args, f"""
        BEGIN;
        INSERT INTO usage_events (
            id,request_id,key_id,project_id,route,provider,status,status_code,latency_ms,
            input_tokens,output_tokens,total_tokens,estimated_cost,service_name,
            http_method,endpoint_template,created_at
        )
        SELECT md5('monitoring-event-' || n)::uuid, 'monitoring-scale-' || n,
               md5('monitoring-key-' || (((n-1) / 5) % 30 + 1))::uuid,
               md5('monitoring-project-' || (((n-1) / 5) % 30 + 1))::uuid,
               CASE n % 5 WHEN 0 THEN '/v1/chat/completions' WHEN 1 THEN '/v1/messages' ELSE '/services/*' END,
               CASE WHEN n % 5 < 2 THEN 'litellm' ELSE 'internal-service' END,
               CASE WHEN n % 100 = 0 THEN 'failure' ELSE 'success' END,
               CASE WHEN n % 100 = 0 THEN 503 ELSE 200 END,
               5 + n % 2000, 100, 50, 150, 0.001,
               CASE WHEN n % 5 < 2 THEN NULL ELSE 'monitoring-service-' || (((n-1) / 5 % 30 + 1 + n % 5) % 20 + 1) END,
               'POST', '/run', anchor - (n % 2592000) * interval '1 second'
        FROM generate_series({start},{end}) n CROSS JOIN monitoring_scale_state;
        UPDATE monitoring_scale_state SET event_count={end};
        COMMIT;
        """)
        print(f"seeded {end:,} historical events", flush=True)
    sql(args, """
    INSERT INTO guardrail_execution_events(request_id,key_id,project_id,guardrail_name,mode,action,failure_policy,latency_ms,created_at)
    SELECT u.request_id,u.key_id,u.project_id,'fixture-observer','pre_call','allow','fail_open',1,u.created_at
    FROM usage_events u WHERE u.request_id LIKE 'monitoring-scale-%'
    AND substring(u.request_id from 'monitoring-scale-([0-9]+)')::bigint % 100 = 0
    AND NOT EXISTS (SELECT 1 FROM guardrail_execution_events g WHERE g.request_id=u.request_id);
    ANALYZE usage_events;
    ANALYZE guardrail_execution_events;
    """)
    return sql(args, "SELECT anchor FROM monitoring_scale_state")


def fetch(base, path, token):
    started = time.monotonic()
    status, size, error = None, 0, None
    try:
        request = urllib.request.Request(base + path, headers={"Authorization": "Bearer " + token})
        with urllib.request.urlopen(request, timeout=35) as response:
            status = response.status
            body = response.read(4 * 1024 * 1024)
            size = len(body)
            json.loads(body)
    except urllib.error.HTTPError as failure:
        status = failure.code
        body = failure.read(4096)
        size = len(body)
        try:
            code = json.loads(body).get("error", {}).get("code", "http_error")
            error = code if re.fullmatch(r"[a-z_]{1,80}", str(code)) else "http_error"
        except (ValueError, AttributeError):
            error = "http_error"
    except (OSError, ValueError):
        error = "transport_or_response_error"
    return {"status": status, "elapsed_ms": round((time.monotonic() - started) * 1000, 3),
            "response_bytes": size, "error": error}


def percentile(values, fraction):
    values = sorted(values)
    return values[max(0, math.ceil(len(values) * fraction) - 1)] if values else None


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--container", default="relayna-monitoring-postgres")
    parser.add_argument("--base", default="http://127.0.0.1:58081")
    parser.add_argument("--token-file", required=True)
    parser.add_argument("--rows", type=int, nargs="+", default=[1_000_000, 10_000_000])
    parser.add_argument("--viewers", type=int, nargs="+", default=[1, 5, 10])
    parser.add_argument("--rounds", type=int, default=3)
    parser.add_argument("--output", default="output/monitoring/scale")
    parser.add_argument("--allow-seed", action="store_true")
    args = parser.parse_args()
    if not args.allow_seed or not re.fullmatch(r"relayna-monitoring-[a-z0-9-]+", args.container):
        parser.error("--allow-seed and a disposable relayna-monitoring-* container are required")
    parsed = urllib.parse.urlparse(args.base)
    if parsed.hostname not in {"127.0.0.1", "localhost", "::1"}:
        parser.error("benchmark must target a loopback test gateway")
    if any(n < 1 or n > 10_000_000 for n in args.rows) or any(n < 1 or n > 50 for n in args.viewers) or not 1 <= args.rounds <= 20:
        parser.error("cohorts, viewers, or rounds outside supported test bounds")
    token_path = Path(args.token_file)
    if os.stat(token_path).st_mode & 0o077:
        parser.error("token file must be private (chmod 600)")
    token = token_path.read_text().strip()
    output = Path(args.output)
    if not any(part in {"output", "target"} for part in output.parts):
        parser.error("evidence must be saved under output/ or target/")
    output.mkdir(parents=True, exist_ok=True)
    report = {"created_at": dt.datetime.now(dt.timezone.utc).isoformat(),
              "head": subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip(),
              "working_tree_dirty": bool(subprocess.check_output(["git", "status", "--porcelain"], text=True).strip()),
              "candidate_source_sha256": hashlib.sha256(subprocess.check_output(["git", "diff", "HEAD"]) + b"".join(
                  path.encode() + b"\0" + Path(path).read_bytes()
                  for path in subprocess.check_output(["git", "ls-files", "--others", "--exclude-standard"], text=True).splitlines()
                  if Path(path).is_file()
              )).hexdigest(),
              "binary_sha256": hashlib.sha256(Path("target/debug/gateway-api").read_bytes()).hexdigest(),
              "host_architecture": platform.machine(), "host": platform.platform(),
              "projects": 30, "services": 20, "services_per_project": 5, "cases": []}
    unexpected = 0
    for count in sorted(set(args.rows)):
        anchor = dt.datetime.fromisoformat(seed(args, count))
        plans = {}
        for name, project in [("all_projects", ""), ("one_project", " AND project_id=md5('monitoring-project-1')::uuid")]:
            plans[name] = json.loads(sql(args, f"""
                EXPLAIN (ANALYZE, BUFFERS, FORMAT JSON)
                SELECT count(*),sum(total_tokens),sum(estimated_cost),percentile_cont(0.95) WITHIN GROUP(ORDER BY latency_ms)
                FROM usage_events WHERE created_at >= '{(anchor - dt.timedelta(days=1)).isoformat()}'{project};
            """))
        (output / f"plans-{count}.json").write_text(json.dumps(plans, indent=2) + "\n")
        for days in [1, 7, 30]:
            filters = {"from": (anchor - dt.timedelta(days=days)).isoformat(), "to": anchor.isoformat(), "limit": 50}
            for scope in ["all_projects", "one_project"]:
                query = dict(filters)
                if scope == "one_project":
                    query["project_id"] = sql(args, "SELECT md5('monitoring-project-1')::uuid")
                for endpoint in ["summary", "events"]:
                    if endpoint == "events":
                        query["cursor"] = "start"
                    for viewers in args.viewers:
                        samples = []
                        with concurrent.futures.ThreadPoolExecutor(max_workers=viewers) as pool:
                            for _ in range(args.rounds):
                                path = "/admin-ui/admin/usage/" + endpoint + "?" + urllib.parse.urlencode(query)
                                samples.extend(pool.map(lambda _: fetch(args.base, path, token), range(viewers)))
                        successes = [row["elapsed_ms"] for row in samples if row["status"] == 200]
                        invalid = [row for row in samples if row["status"] != 200 and row["error"] != "gateway_overloaded"]
                        unexpected += len(invalid)
                        case = {"rows": count, "days": days, "scope": scope, "endpoint": endpoint,
                                "viewers": viewers, "successes": len(successes), "unexpected": len(invalid),
                                "p50_ms": percentile(successes, .50), "p95_ms": percentile(successes, .95),
                                "p99_ms": percentile(successes, .99), "samples": samples}
                        report["cases"].append(case)
                        (output / "report.json").write_text(json.dumps(report, indent=2) + "\n")
                        print(f"{count:,} rows {days}d {scope} {endpoint} viewers={viewers}: "
                              f"{len(successes)}/{len(samples)} admitted, p95={case['p95_ms']} ms, unexpected={len(invalid)}", flush=True)
    report["unexpected_responses"] = unexpected
    (output / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    raise SystemExit(1 if unexpected else 0)


if __name__ == "__main__":
    main()
