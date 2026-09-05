# Live traffic monitor

Open **Monitor → Traffic** in Admin UI. The live table shows incoming proxy
requests before authentication, current processing stage, upstream attempts,
client and upstream HTTP statuses, elapsed time, and failure/recording reasons.
Use **Inspect** for the timeline, project/key attribution and gateway instance.
Filter by request ID, service, project/key UUID, HTTP status or failure reason;
click a reason count to filter its requests, then click the highlighted count
again to clear that reason while preserving other filters. The selected reason
remains removable even with zero matching records. **Pause** freezes the live display;
**Resume** reconnects and reports missed updates.

## Request investigation

The **Routing mode** column distinguishes gateway-managed requests from LiteLLM
passthrough using recorded request metadata. Passthrough token/cost details say
**Not metered by gateway**, including when no Usage row exists. Historical or
unresolved modes show **Not recorded**. Terminal logs include `relayna.routing_mode`.

**Traffic → Inspect** and **Usage & cost → Debug** share one investigation
layout: outcome and HTTP statuses, request context, network/response timing,
event timeline, usage and estimated cost, policy/guardrails, routing decisions,
and expandable raw diagnostics/hashes. Copy actions export only the sanitized
metadata displayed in the panel. Project names are resolved from the current
inventory; IDs remain visible for correlation.

New Usage diagnostics contain `traffic_id`, the gateway's unique internal ID.
The drawer uses that ID to find the exact saved Traffic record, even when a
client reuses its `x-request-id`. Completed Traffic records include their own
allowlisted usage and debug snapshots. Missing/older Traffic records do not
prevent opening Usage metadata. Legacy Usage records without `traffic_id` are
not joined to arbitrary request-ID-only snapshots; the existing Health debug
lookup remains available and is explicitly labeled as a legacy snapshot.

### Timing definitions

Each upstream attempt captures up to the following values:

| Field | Meaning |
| --- | --- |
| DNS resolution | Elapsed asynchronous hostname lookup, including the OS resolver/cache. IP literals report “Not needed.” Resolution precedes connection-pool lookup, so it may occur even if the connection is reused. |
| TCP connect | Socket preparation hook to TCP-established timestamp on a new connection. This excludes DNS and TLS. |
| TLS handshake | TCP-established to TLS-established timestamps on a new TLS connection. Plain HTTP reports “Not used.” |
| Response headers | Attempt start to receipt of upstream response headers, including resolution, connection and request transmission. |
| First body byte | Attempt start to receipt of the first nonempty upstream body chunk. |
| First content token | Attempt start to the first complete supported SSE event containing output text or tool-argument content; role-only events, comments and keepalives are excluded. |
| Attempt duration | Attempt start until its failure/retry or terminal gateway logging; the successful attempt includes downstream delivery. |

Connection reuse reports **Reused connection** for TCP and TLS, without copying
measurements from the original connection. Unavailable values report **Not
recorded**, including incomplete connection handshakes and older records. TCP/TLS
measurement uses connection timestamps; invalid clock ordering produces no value.
DNS/TCP/TLS durations are stored in microseconds and displayed in milliseconds;
request milestones use a monotonic elapsed clock. DNS errors are classified at
the resolution stage, and lookup is bounded by the configured route timeout.

HTTPS upstreams use Pingora's Rustls backend with certificate-chain and hostname
verification enabled. The connector uses platform trust roots (including
`SSL_CERT_FILE` / `SSL_CERT_DIR` overrides). Connection errors return a failed
request or use the configured fallback; they do not leave retry decisions unset.

The token observer reads at most the first 64 KiB per attempt, retains only an
incomplete SSE event, and never waits for a full response or modifies chunks. It handles
Chat Completions, Responses text/tool deltas and Anthropic content deltas. Unknown
formats, content after that observation limit, and non-streaming JSON have no
first-content-token measurement. This is an upstream observation, not a claim
that the client has rendered the token. Attempt timings retain the latest 32
attempts and the drawer reports truncation.

## Understanding a failure

A 503 can originate from different places. `gateway_overloaded` means the
body-processing admission limit was exhausted. `control_state_unavailable` means
the gateway could not access rate-limit or budget state; the failure stage tells
you which operation failed. `upstream_http_error` with upstream status 503 means
an upstream response was actually received. Connection refusal, TLS certificate
failure, timeout, write failure, and interrupted responses have separate codes.

The timeline distinguishes selecting an upstream connection, establishing it,
preparing request headers, and receiving its response. Preparation does **not**
prove upstream application receipt. Each connection selection has an attempt
number, including retries/fallbacks. No attempt means the request was rejected
before upstream connection selection. A client disconnect can leave delivery
unknown; the monitor never invents a delivered status.

Client HTTP status is independent of outcome. A streamed response may have
already sent **200** when upstream closes; it appears as **stream interrupted**
and its usage row is a failure. The monitor does not buffer the stream.

Gateway errors include `x-request-id` and `x-relayna-request-id`, and the JSON
error includes the correlation ID. Existing valid `x-request-id` values are
preserved (1–128 ASCII letters/digits or `-_.:`); other values receive a generated
ID. Search that ID in Traffic or gateway JSON logs. Internal diagnostic IDs are
unique even when callers reuse correlation IDs. Provider response bodies are
passed through unchanged; detailed internal diagnostics remain admin-only.

A 503 generated by an ingress or load balancer **before** reaching the gateway
requires that component's logs. A gateway process crash may also prevent a
terminal diagnostic from being emitted.

## Live scope, history and gaps

The live feed belongs to **one gateway process**, identified by a UUID that
changes on restart. It is not a cluster-wide traffic counter. All records and
failure logs include that instance ID. With several replicas, use an instance
address or session affinity for continuous live inspection of one replica; use
saved history for completed requests across replicas.

The process journal keeps the latest **512 lifecycle updates**. Each update
contains the retained request timeline (up to **32 steps**); the browser displays
up to **200 requests**. Older updates and requests can be evicted, including
long-lived active requests. Active counts and reason counts refer only to retained
records. Truncated timelines, journal eviction, replay gaps, instance changes and
disconnected streams are explicitly reported. Pausing, overflow or reconnecting
may therefore lose intermediate observations.

**Saved history · all instances** queries completed PostgreSQL records, newest
first, 100 at a time. Its default lookback is 24 hours. Set History from/to for
other periods and use Older/Newest for pagination. Request IDs are exact matches
in history (substring matches in the retained live window). Anonymous failures
have nullable key/project IDs and never create fabricated billing identities.
Records from before this release have no diagnostic history.

## Storage failures

Terminal failures are logged before database operations. Usage, debug bundle and
traffic writes each have a two-second timeout; failed writes emit
`gateway diagnostic recording failed` with a fixed destination name. The live
record shows **Recording failed** and lists affected destinations. Recording is
best effort: a timeout can have an uncertain commit outcome, and a failed write
is not retried automatically. A disconnected browser cannot recover an evicted
record that also failed persistence; retain process logs using your existing log
collector.

The in-process journal does not depend on PostgreSQL or Redis. Admin authorization
still uses existing identity storage: during a PostgreSQL outage, an already-open
feed can continue until its authorization lease expires, but new connections may
fail. Process logs remain the independent diagnostic source.

## Access, data and deployment

Traffic APIs require the existing `usage:read` operator scope or an active admin
portal session (with the existing CSRF header). Owner-only sessions cannot read
unattributed or other-project traffic. The UI uses authenticated fetch streaming,
never query-string credentials. Streams expire after 30 seconds and reconnect
through authorization again; revocations take effect at the next connection.
There are at most 32 simultaneous live viewers per process. Responses are marked
private/no-store; configure your ingress to pass `text/event-stream` without
buffering and allow streaming connections. The browser reports a disconnect if
it receives no data for 12 seconds.

Diagnostic records contain no request/response bodies, auth headers, query
strings, raw transport error messages, or raw unresolved request paths. Endpoints
are bounded route templates; service names and identity IDs are metadata. Treat
correlation IDs as metadata, not a place to put secrets. Existing optional Entra
authorization debugging remains a separate, explicitly enabled feature.

The additive SQLx migration adds nullable diagnostic attributes inside a
`diagnostics` JSON object on usage records and creates `request_traffic` with
indexes for time, request ID and failures. Existing usage records read with empty
diagnostics; existing CSV columns remain unchanged. Apply the migration through
the normal startup migration flow. Old binaries can continue using existing
columns. To roll back the application, keep the additive table/column until
retention and reader requirements allow removal.

History retention is operator-managed, like usage history. Set a retention policy
appropriate to traffic volume; for example, schedule bounded deletion of records
older than seven days and monitor database size:

```sql
DELETE FROM request_traffic WHERE id IN (
  SELECT id FROM request_traffic
  WHERE started_at < now() - interval '7 days'
  ORDER BY started_at LIMIT 10000
);
```

## API reference

- `GET /admin-ui/admin/traffic/live`: SSE `traffic` events, each containing
  `instance_id`, `cursor`, `gap`, `evicted_updates` and deduplicated request `rows`.
  Send `Last-Event-ID` with the previous cursor to resume; the server reports a gap
  if that cursor has expired or belongs to another process.
- `GET /admin-ui/admin/traffic/history`: completed diagnostic rows. Optional
  filters: `id` (internal diagnostic UUID), `request_id`, `service`, `project_id`, `key_id`, `status`,
  `failures_only`, `failure_code`, `from`, `to`. `limit` is 1–200 (default 100).
  An exact `id` lookup omits the default 24-hour lookback unless `from` is supplied;
  it still requires the same admin authorization.
  Pass both `before` (last row's start time) and `before_id` for stable pagination.
- Usage JSON responses now include `diagnostics`: `failure_stage`, `failure_code`,
  `failure_source`, `outcome`, `upstream_status`, `instance_id`, optional
  `traffic_id`, and optional `routing_mode` (`managed_by_gateway` or
  `litellm_passthrough`). Missing modes remain unknown. Traffic records add
  defaulted `upstream_timings`, `usage`,
  `debug_bundle`, and masked `key_prefix` fields inside their existing JSON.
  These additions need no schema migration; older saved records remain readable.
