# Gateway monitoring and scalable Usage

**Monitor → Health** includes a live Gateway section; **Monitor → Traffic**
includes a compact capacity strip. Select a process or all reporting instances,
and a one-, five-, or fifteen-minute window. Measurements refresh every two
seconds and identify partial coverage, stale publishers, failed refreshes, and
unsupported measurements explicitly.

The dashboards lead with visual signals. Health pairs a live request-rate chart
with buffer-occupancy meters, a protocol mix, and p50/p95/p99 latency comparisons.
Fleet dependency indicators show each reporting process. Expand the diagnostic
sections for exact latency values, process counters, and metric definitions.
Traffic provides HTTP/SSE and WebSocket tabs, concise request rows, and optional
advanced filters; Inspect opens the complete request investigation.

Live charts retain at most 90 browser observations, collected while that view is
open. Changing the instance or window starts a new series. They are not persisted
history; missing observations create gaps. Latency bars are histogram upper
bounds. Missing measurements remain unknown rather than zero.

## Usage at production volume

New Usage visits select **Last 24 hours**. All projects remains available, as do
explicit All time and custom windows. Applying filters freezes their time range
for subsequent section and page requests. Summary and recent events load
independently; a failed optional inventory, suggestion, or breakdown cannot
replace successful results. Select a breakdown or timeseries tab to load it.
Request-volume and estimated-cost charts share one additional, independent
timeseries read, capped at 168 returned buckets. Longer windows use daily buckets;
if more buckets are available, the chart identifies its first-page coverage.
All time requires a bounded range to draw trends. Chart failure leaves the
summary and request list usable. Ranked bars compare the request counts of the
returned breakdown groups; the underlying table remains available.
Suggestions use a bounded prefix search after focus or typing. Event pagination
does not reload summary or other tabs.

The existing dashboard API remains available with its released response shape.
New section reads use `/admin-ui/admin/usage/sections/{section}` for `projects`,
`keys`, `services`, `endpoints`, `providers`, `models`, `tasks`,
`project-key-services`, `timeseries`, `service-timeseries`, and `unused-keys`.
They use existing Usage filters and administrator authorization. Exact owner
project/service filters remain server-enforced on owner APIs.

Recent-event queries select a bounded Usage page before counting its guardrail
events. Chronological indexes support unscoped reads; project/key/service
indexes continue to support scoped reads. Exact summary totals and historical
P95 definitions remain unchanged. The raw ledger continues to support Studio,
budget reconstruction, and historical repricing; no rollup replaces it.

Usage reads have a separate four-connection pool per process, alongside the
existing ten-connection primary pool. Connection acquisition is bounded to two
seconds and PostgreSQL statements to fifteen seconds, inside a twenty-second
handler budget. Budget up to fourteen connections per replica plus migration
and operational connections when sizing PostgreSQL or a connection pooler.
Four administrator analytics handlers may run per process; additional admin
reads fail admission immediately instead of waiting behind expensive scans.
Owner reads share the bounded Usage pool and its acquisition deadline.
Analytics connections use
custom PostgreSQL plans so a repeated prepared query does not reuse a generic
time-range plan for a much broader ledger scan.

### Cursors and large downloads

`GET /admin-ui/admin/usage/events?cursor=start&limit=50` starts a chronological
traversal. Follow `next_cursor` with the same filters. Cursors include a unique
event-ID tie breaker and insertion-visibility boundary. Late inserts require a
new traversal. They are opaque paging values, not authorization credentials.
Existing offset/sort clients remain supported. Historical repricing or retention
can still change existing rows during a traversal; an export is not a database
backup or a retained MVCC transaction.

Insertion visibility uses immutable full-transaction-ID markers on new Usage and
guardrail records. Repricing preserves event membership. Legacy rows have null
markers and remain visible; their original insertion boundary cannot be
reconstructed retrospectively. The migration adds nullable columns before
setting defaults and does not backfill or rewrite the historical ledger.

Apply the additive migrations before deploying the new readers. Chronological
and Traffic indexes require PostgreSQL I/O and normal index-build locks during
migration; schedule the upgrade for the size of your ledger. Allow headroom for
index storage and verify migration duration on a production-shaped copy.

The CSV/JSON export endpoints accept additive `stream=true` with both `from` and
`to`. The stream computes its summary once and reads 1,000-row cursor batches.
CSV columns and formula escaping are unchanged. A failed batch terminates the
download; discard incomplete files and retry. The UI's All rows download writes
incrementally where the browser supports file streaming; other browsers receive
bounded-export and command-line guidance. Existing one-page preview and copied-request formats remain
available. Analytics and exports have bounded admission, so concurrent expensive
reads may receive `503 gateway_overloaded`; this is separate from proxy body
capacity exhaustion.

## Runtime measurements

Runtime observations are recorded independently of PostgreSQL Usage writes.
They include denied and unattributed traffic. Successful HTTP/SSE latency uses
the final lifecycle outcome, so an interrupted stream with HTTP 200 is a failure.
WebSocket handshake and open-session duration have separate distributions.
HTTP and WebSocket duration samples enter the rolling window at termination;
an open session's handshake sample is therefore available after it closes.
Use active Traffic for current session age and last observed activity.
Frame/byte counts include control traffic; they do not measure application-turn
success or message-response latency.

Histograms have fixed microsecond buckets. Displayed p50/p95/p99 values are bucket
upper bounds; no samples and values above the histogram range are explicit.
Cluster quantiles merge compatible bucket counts before calculating percentiles.
Window resolution is one second. The API reports the observed portion of the
selected window, sample count, and sampling timestamp; a newly started process
has less than a full window of observations.
Fleet arrival rate sums per-instance rates using each process's observed time,
so restarting a replica does not divide old counters by the new replica's age.

Body capacity reports effective configured slot/byte limits, used capacity,
vacant capacity, and rejection reasons. Slots follow retained managed request or
response buffers until their final reference is dropped. They are not all
connections or outstanding upstream work. Admission is fail-fast; no queue or
queue-wait measurement is invented. Admission decision and body-retention
latencies are separate. Assess total process/container memory alongside buffer
capacity because parsing, guardrails, sockets, and the runtime also use memory.

Linux `/proc` and cgroup v2 supply nullable RSS, cgroup memory/current limit, CPU
usage/limits/throttling, and observed OOM counters. These are sampled observations;
missing files, other operating systems, unlimited cgroup limits, and unavailable
restart counts remain unknown. Cgroup counters can include other processes in
the same cgroup. Kubernetes restart history requires an external collector.

## Multiple gateway instances

Each process publishes bounded metadata snapshots once per second through the
existing Redis connection configuration. Observation state never participates
in authentication, policy, rate limits, budgets, or forwarding decisions. The
publisher runs on the control runtime with bounded I/O timeouts. A failed
publisher or collector produces partial/unavailable observations; local
measurements remain available where authorization succeeds.

The namespace is `gateway:monitor:v1:`. It contains a bounded instance index and
separate `:<process-uuid>:stats` / `:<process-uuid>:traffic` snapshots. Individual
snapshots expire after 120 seconds; publishers become stale after five seconds.
Process UUIDs change on restart. Old/stale process counts never become numeric
zero-current-traffic claims. Stale instances are excluded from current totals
and identified in coverage metadata. After expiry, an absent process requires
deployment/collector inventory to distinguish a removed replica from an outage.

At most 64 publishers are retained. Per publisher, statistics are capped at
256 KiB and active Traffic at 4 MiB; a fleet read has an 8 MiB byte budget.
Individual-instance reads apply their selection before that budget. Budget
exhaustion is explicit, and narrowing the instance/project/protocol recovers
available detail. Reserve Redis memory for this ephemeral observation data in
addition to control state. Do not flush shared Redis to reset monitoring.

New administrator endpoints, both private/no-store and requiring `usage:read`:

- `GET /admin-ui/admin/gateway/status?instance_id=all&window_seconds=60`
- `GET /admin-ui/admin/traffic/cluster?instance_id=all&protocol=websocket&limit=100`

Status returns per-instance observations and a compatible fresh aggregate.
Traffic returns bounded active rows, independent active counts, coverage,
staleness, and pagination metadata. Existing `/traffic/live` remains the
process-local SSE journal and retains its authorization renewal protocol.
Neither endpoint contains bodies, authentication headers, credentials, query
strings, or raw unresolved paths. Project/service/request identifiers remain
diagnostic metadata rather than high-cardinality Prometheus labels.

Prometheus collection remains available at `/admin-ui/metrics`; no collector is
required for the live console. Existing metric names/labels are preserved.
Additive admission-capacity and diagnostic-recording metrics support external
collection. Process gauges and window histograms reset with process identity;
use per-instance series and merge compatible histogram buckets.

## Verification and capacity planning

The disposable scale harness at `tests/e2e/monitoring/scale.py` seeds 30 projects,
20 shared services, five links per project, and OpenAI/Anthropic plus registered
service history. It records one-million/ten-million-row cohorts, one/five/ten
concurrent readers, 24-hour/7-day/30-day ranges, all-project/project reads,
per-request outcomes, and query plans. See its
[run instructions](https://github.com/sarattha/relayna-gateway/blob/main/tests/e2e/monitoring/README.md) in the source repository.

Run latency comparisons and a 24-hour mixed HTTP/streaming/WebSocket soak on
your deployment resources before claiming a production capacity. A short test
does not establish a 24-hour soak or a safe limit. Version bumps separately
require the complete [release benchmark procedure](https://github.com/sarattha/relayna-gateway/blob/main/benches/body-admission/README.md).
