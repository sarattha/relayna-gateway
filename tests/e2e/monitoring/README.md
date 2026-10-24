# Monitoring verification

Use an isolated, migrated PostgreSQL database named `relayna_monitoring` in a
Docker container named `relayna-monitoring-*`, with role `relayna_test`.
The harness refuses remote gateway targets and requires `--allow-seed`.
It adds deterministic synthetic history; it never truncates existing tables.
Never point the fixture at production.

Start the candidate Gateway with that database and disposable Redis. Keep the
operator credential in a local mode-0600 file under ignored `output/`.
Do not include the credential or Gateway environment in reports.

```bash
python3 tests/e2e/monitoring/scale.py \
  --allow-seed --container relayna-monitoring-postgres \
  --base http://127.0.0.1:58081 \
  --token-file output/monitoring/operator-token \
  --rows 1000000 10000000 --viewers 1 5 10 --rounds 3 \
  --output output/monitoring/scale
```

The database must already have Gateway migrations applied. The script uses the
container's `psql` and Python's standard library. It records response sizes,
successful-request p50/p95/p99, expected analytics-admission 503s separately from
unexpected errors, and `EXPLAIN (ANALYZE, BUFFERS, FORMAT JSON)` plans. Reports
contain no credentials, request bodies, or API response contents. Repeated runs
can extend the existing cohort; use a new database to return to a smaller one.
Results identify the starting Git revision; record any uncommitted candidate
changes separately and rerun after runtime changes before publishing evidence.

The fixture varies project and service attribution independently, uses shared
service links, includes canonical LiteLLM OpenAI and Anthropic routes, and adds
guardrail history for one percent of requests. It exercises 24-hour, 7-day, and
30-day reads with ongoing Gateway observation publication. It is a Usage scale
test, not a forwarding throughput or 24-hour connection soak benchmark.

For retention and lifecycle correctness, run the dependency-backed
`traffic_monitor_integration` and `websocket_transport_e2e` tests with
`DATABASE_URL` and `REDIS_URL`. The PostgreSQL test role needs `CREATEDB`.
Complete the required repository verification stack, UI build/tests, and
desktop/mobile browser checks. Keep all generated evidence under ignored
`output/` or `target/`.

## Two-instance mixed-protocol soak

`soak.py` uses Python 3.11+ and its standard library. Run two disposable native
Gateway processes against the same migrated monitoring database and Redis,
with distinct proxy/control ports. Coordinate restarts before starting the run.
The script refuses non-loopback targets and requires `--allow-fixtures`. It
starts an ephemeral-port loopback upstream and creates a uniquely named project,
neutral project policy layer, registered service, and virtual key through the
admin API. It does not edit global settings, existing policies, providers, or
LiteLLM. The service and key are disabled on exit; synthetic diagnostic history
and the project remain available for investigation in the disposable database.
If the process is killed without cleanup, disable the service/key identified by
the run's service name. Credentials stay in process memory and are never saved
in evidence. Only the operator token comes from the mode-0600 file.

After the candidate processes have restarted, resolve their **current** PIDs:

```bash
PYTHONPYCACHEPREFIX=target/python-cache python3 tests/e2e/monitoring/test_soak.py
PYTHONPYCACHEPREFIX=target/python-cache python3 -m py_compile tests/e2e/monitoring/soak.py
python3 tests/e2e/monitoring/soak.py --help
python3 tests/e2e/monitoring/soak.py \
  --allow-fixtures --token-file output/monitoring/operator-token \
  --proxy http://127.0.0.1:58080 http://127.0.0.1:58180 \
  --control http://127.0.0.1:58081 http://127.0.0.1:58181 \
  --gateway-pid "$(lsof -t -iTCP:58081 -sTCP:LISTEN)" \
                "$(lsof -t -iTCP:58181 -sTCP:LISTEN)" \
  --seconds 300 --http-rate 20 --min-http-per-instance 1000 \
  --output output/monitoring/soak-5m
```

The default five-minute workload sends 20 HTTP requests/second total through
both processes, with two bounded workers per process. JSON POSTs exercise the
managed body path. One successful/truncated SSE sequence runs per instance,
alongside a quiet WebSocket opened before HTTP churn. After 225 seconds in the
default run, a real echoed frame checks that the old socket survived newer
HTTP traffic and that live frame counters advance. No heartbeat is sent during
the quiet interval. The synthetic upstream terminates every fourth SSE response
after HTTP 200 and a real event; persisted Traffic must report
`stream_interrupted`, while complete streams report `completed`. This is a
transport test, with no provider inference.

Only HTTP 503 with the exact JSON error code `gateway_overloaded` counts as an
expected admission denial. HTTP/SSE denials are reported separately and do not
count toward the successful-HTTP threshold or validated stream outcomes. The
latest denial must persist as HTTP 503 with outcome `failed` and that exact
failure code. Terminal stream checks use the latest validated HTTP 200 stream
of each kind. Unknown responses, transport failures, and missing protocol
coverage fail the run. Leave headroom for the SSE worker when setting HTTP
concurrency: eight HTTP workers plus SSE can exceed an eight-slot body cap;
use at most six HTTP workers per process for a controlled run on that setup.

One process-local journal observer per instance reconnects after the normal
30-second authorization lease. Status reads go through both control listeners;
active publication reads select each process UUID through the first listener,
checking local and remote publication while applying the instance selection
before the fleet byte budget. Unrelated/stale publishers and global partial
coverage are recorded separately; assertions apply to the chosen two processes.
With a default two-second pause between samples, the harness records runtime
active counts, histogram/counter observations, effective body capacity,
diagnostic coverage, registry row/byte bounds, and quiet-socket
progress. Request latency adds to the observed sampling interval under load.
Body/query canaries and all harness credentials must be absent from
observer responses. Terminal metadata checks completion time and instance
attribution. After load drains, both permitted WebSocket slots must reopen per
process, the fixture's active rows must disappear, and serialized-body slots and
bytes must return to zero. It reports observer gaps, failures, natural lease
renewals, and successful/failed client operations separately.

Use `--concurrent-load-note` to record any simultaneous read benchmarks or other
workloads. Use `--gateway-pid` on macOS for native `ps` RSS samples because Linux `/proc`
and cgroup readings are unavailable. PIDs must match the candidate for the whole
run. Runtime process readings remain nullable; the harness does not invent
cgroup, OOM, or restart values. The default RSS-growth ceiling is 128 MiB above
the first sampled value per process; change `--max-rss-growth-mib` for an explicit
deployment-specific bound. The report includes per-process peaks and body-slot
peaks, so a run with no sampled body occupancy does not prove saturation or an
admission edge. Keep other proxy traffic out of the disposable processes while
testing drain assertions. Normal admin/UI reads may run concurrently.

An exact `harness.py` copy accompanies each run. Evidence is written
incrementally to `observations.ndjson`, with a final
`report.json` containing outcomes, run duration, Git identity, dirty-candidate
flag, harness checksum, fixture identifiers, protocol counts and limitations.
Output must be a fresh directory below ignored `output/` or `target/`. Nonzero
exit means an assertion, workload, cleanup, or observer failed; preserve that
directory and rerun into a new one. Record the built binary checksum/runtime
commit separately when publishing evidence; the Git checkout may contain
uncommitted code and is not proof of the running binary's identity.

For a future 24-hour run, use `--seconds 86400` and a new evidence directory.
Each socket is probed after every configured quiet interval (240 seconds by
default), within the registered-service idle timeout. The runtime's session-age
limit remains 24 hours. At 23 hours the harness explicitly opens and probes a
replacement before closing the old socket, validates the retired terminal row,
and checks replacement publication. HTTP/SSE load and journal observers continue
through this transition. Renewals and retired outcomes are recorded; unexpected
expiry or transport failure is still a failure, with no automatic recovery.
A 24-hour soak therefore covers continuous mixed traffic across an explicit
session renewal, not one socket beyond its configured maximum age. For a short
renewal smoke, use `--socket-renew-seconds 120 --quiet-seconds 30` with a run
longer than 120 seconds. Sampling is serialized briefly across renewal while
the protocol workers continue. Workers, observer buffers, request/response sizes,
and retained report state remain bounded; the incremental evidence file grows
with the selected duration. This harness does not inject
internal late callbacks after cancellation, exhaust the 2,048-row registry, or
inject dependency outages; focused Rust tests cover those independent paths.
Passing five or ten minutes does **not** establish a 24-hour soak, a memory-leak
proof, real-provider interoperability, or safe production throughput.
