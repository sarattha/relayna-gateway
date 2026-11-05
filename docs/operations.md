# Operations

Monitor → Traffic shows live request timelines, failure reasons and saved history. See [Traffic Monitor](operations/traffic-monitor.md) for operation and retention details.

## Configuration

Required environment variables:

| Name | Purpose |
| --- | --- |
| `DATABASE_URL` | PostgreSQL connection string for durable gateway state. |
| `REDIS_URL` | Redis connection string for readiness, rate limits, and budgets. |
| `LITELLM_BASE_URL` | LiteLLM or OpenAI-compatible upstream base URL. |
| `LITELLM_SERVICE_KEY` | Startup fallback LiteLLM credential used when no active LiteLLM provider config supplies a default credential. |
| `GATEWAY_BIND_ADDR` | Proxy listener, for example `0.0.0.0:8080`. |
| `GATEWAY_CONTROL_BIND_ADDR` | Control listener, for example `0.0.0.0:8081`. |
| `LOG_LEVEL` | Rust tracing filter. |

Optional variables:

| Name | Purpose |
| --- | --- |
| `DIRECT_OPENAI_BASE_URL` | Optional direct provider base URL. |
| `DIRECT_OPENAI_SERVICE_KEY` | Optional direct provider credential. |
| `RELAYNA_WORKER_TOKEN` | Optional shared token for Relayna worker integration. |
| `RELAYNA_STUDIO_BASE_URL` | Optional Relayna Studio backend base URL for Admin portal service import. |
| `RELAYNA_STUDIO_TOKEN` | Optional bearer token used when Gateway fetches the Studio service catalog. |
| `GATEWAY_MAX_BUFFERED_REQUESTS` | Maximum concurrent managed requests or post-call responses that may retain complete bodies in memory. Defaults to `8`. |
| `GATEWAY_MAX_INFLIGHT_BUFFER_BYTES` | Maximum aggregate serialized body bytes retained by buffered proxy requests and responses. Defaults to `536870912` (512 MiB). |
| `GATEWAY_MAX_BODY_WORKING_BYTES` | Optional additional estimated working-set budget for buffered requests/responses. Unset preserves existing count/byte admission. |
| `GATEWAY_BODY_ELASTIC_MAX_REQUESTS` | Optional elastic-mode safety ceiling, at least `GATEWAY_MAX_BUFFERED_REQUESTS`. Requires a working budget. Explicitly replaces the ordinary count ceiling; byte charges determine concurrency below it. Unset retains the ordinary ceiling and upfront workspace reservation. |
| `GATEWAY_SMALL_BODY_RESERVED_REQUESTS` | With a working-set budget, reserve this many count permits and their estimated working bytes for declared bodies up to 256 KiB. Defaults to `0`; must be below `GATEWAY_MAX_BUFFERED_REQUESTS`. |
| `GATEWAY_BODY_WORKSPACE_MULTIPLIER` | Estimated temporary workspace in multiples of body size, in addition to allocation capacity and 64 KiB/request overhead. Defaults to `2`; accepts `1`–`16`. Requires a working-set budget. |
| `GATEWAY_BODY_UPLOAD_TIMEOUT_MS` | Buffered-upload deadline from first admitted chunk, default `30000`, maximum `600000`. Checked on each chunk; HTTP/1 reads also use the remaining deadline. Requires a working-set budget. |
| `GATEWAY_MAX_UPSTREAM_REQUESTS` | Optional fail-fast process-wide upstream request/stream ceiling, held through completion or cancellation and reused across retries. Unset preserves existing behavior. |
| `MALLOC_MMAP_THRESHOLD_` | glibc allocator setting; the official Debian image pins `131072` (128 KiB). Set before starting native glibc deployments to match the image's large-allocation reclamation behavior. Other allocators/platforms may ignore it. |
| `ENTRA_AUTH_DEBUG` | Enables the temporary structured Entra, trusted-Apigee, portal-login, and cookie-session decision trail. Defaults to `false`; see [Entra Authorization Debug Mode](operations/entra-authorization-debug.md). |

Body admission limits are process-wide and complement per-route request limits.
When a valid request cannot acquire process body capacity, Gateway returns
`503 gateway_overloaded` with a short retry hint. Route-size violations remain
`413 request_body_too_large`. Eligible fixed or unpriced registered-service
requests with non-JSON bodies and no active body guardrails stream without
consuming the buffered-body budget. Size the byte limit well below the pod
memory limit because JSON object trees and guardrails require additional
working memory.

Buffered bodies grow incrementally. For requests with a declared
`Content-Length`, buffer growth uses that length as an allocation hint to avoid
geometric capacity overshoot (for example, a 16 MiB allocation for a 10 MiB
body). The hint does not preallocate the full body or replace actual-byte
admission and route-size checks. Unknown-length bodies still grow within the
route body limit. Token estimation runs when an input-token or token-per-minute
limit needs it; policy and guardrail checks still inspect the complete body.
Metadata analysis and any required token estimate are retained with the exact
immutable body bytes and reused by subsequent admission checks. Guardrail
rewrites receive fresh analysis, and the original buffer is released before
final admission of the rewritten body. Both original and rewritten bodies
remain subject to policy checks; TPM is charged once from the forwarded body.
These allocation savings do not establish a new safe slot count. Validate
slot and byte settings with representative traffic and pod memory headroom.
The [native arm64 allocation comparison](benchmarks/body-buffer-allocation-00e996b.md)
records measured gains, overload responses and remaining OOM limits.

### Mixed body sizes

The request-count ceiling charges one permit per retained body, regardless of
size. With mixed traffic, that ceiling can reject small bodies while the byte
budget still has space. The optional working-set budget allows operators to
evaluate a higher count ceiling with a second size-dependent bound.
For elastic admission, explicitly set `GATEWAY_BODY_ELASTIC_MAX_REQUESTS` to
a larger safety ceiling. The working budget then admits many small bodies or
fewer large bodies as the mix changes, without a sampled average body size or
a feedback loop. The safety ceiling bounds tiny/empty-body bookkeeping; it is
not a target concurrency. Without this option, the ordinary count ceiling
remains unchanged. The serialized-byte limit always remains authoritative.

Working-set admission charges 64 KiB of request overhead, the larger of buffer
capacity or forecast body length, and `GATEWAY_BODY_WORKSPACE_MULTIPLIER` times
the forecast length. A declared Content-Length reserves estimated working bytes
without allocating the complete body. Actual bytes and route limits remain
authoritative. Growth is checked before buffer allocation, including rewritten
JSON output; buffer capacity is checked again after allocation. Workspace is
released after request analysis/rewrite, while the forwarded allocation remains
charged through its last reference. In elastic mode, upload reservations cover
the body and overhead; the workspace multiplier is added only immediately
before complete-body processing. This avoids charging processing workspace
during network waits. After synchronous analysis, requests with only compact
selectors retain at most 64 KiB of metadata workspace during policy/database
waits, and must reacquire full workspace before further body processing.
Large selectors, client guardrail data, batches, provider transforms and service
pricing retain the full estimate. That upgrade can fail after an upload has completed:
the body is dropped and rejected without waiting or forwarding. Operators must
measure these late denials as well as early rejections. Buffered post-call
responses use the same processing upgrade and share the budget, retaining
their reservation through downstream completion.

For example, an experimental configuration can use a 384 MiB working budget,
an elastic safety ceiling of 512, four reserved small requests, multiplier 2,
and an upstream ceiling of 1,024. These values are not production capacity
guarantees. A higher count ceiling can shift the bottleneck to authentication,
CPU, sockets or the upstream; preserve independent limits on those resources.

Declared bodies up to 256 KiB may use protected small-body capacity. Large and
unknown-length bodies use shared capacity; a small body that grows beyond its
classification must acquire shared capacity before growth. The protected count
and estimated-memory reserve are not lent to large requests. This limits
interference but does not guarantee fairness or minimum throughput for either
class. Admission remains fail-fast, without a queue of partially retained
bodies. Saturation returns `503 gateway_overloaded`; route-size violations
remain `413 request_body_too_large`.

This budget is an estimate, not an RSS or allocator limit. JSON object density,
guardrail internals, provider transforms, and networking can consume additional
memory. Choose workspace multipliers and pod headroom from representative
measurements; the default multiplier is not a worst-case expansion bound.
The upload timeout is checked whenever another chunk arrives. Pingora's
downstream read timeout currently applies to HTTP/1, not HTTP/2; use ingress
stream/idle deadlines for silent HTTP/2 uploads. Timeout can close a connection
without delivering a structured denial. The limits do not protect against
distributed abuse or replace ingress connection limits.

An optional upstream ceiling bounds open provider requests even after their
upload buffers have been released. It is shared across providers, includes long
streams, and does not reserve per-tenant or per-provider capacity. Rejections
use the existing overload response. The gauges `gateway_body_working_bytes`
and `gateway_admitted_upstream_requests` report estimated reserved memory and
requests covered by the optional upstream limiter; a zero upstream gauge with
that limiter unset is not a count of all provider traffic. Rejection counters
use `kind="body_admission",reason="working_set"` for buffer admission,
`kind="body_admission",reason="processing_workspace"` for a late elastic
workspace upgrade, and
`kind="upstream_admission",route="proxy",reason="concurrency"` respectively.

Buffered request slots and byte reservations follow the forwarded buffer until
its last reference is released. An upload queued behind a slow upstream writer
still consumes admission; upstream processing time after the upload is consumed
does not. Complete-body policy, pricing, and guardrail checks still finish
before buffered request bytes are forwarded. Guardrail expansion must fit the
aggregate byte budget as well. These limits bound managed buffers, not total
pod RSS or all in-flight connections. See the [native amd64 two-pod 5 MiB benchmark](https://github.com/sarattha/relayna-gateway/blob/8903a62191d461f12c53d10108b1c880452945eb/internal/test-reports/body-admission-5mib/amd64/README.md)
for measured memory and timing at different concurrency limits. To repeat the
resource search or a fixed-slot matrix on native amd64 or Apple Silicon/arm64,
use the [source benchmark CLI](https://github.com/sarattha/relayna-gateway/blob/main/benches/body-admission/README.md).
It provisions an isolated local cluster and generates an interactive HTML report
after completed, failed, or interrupted runs, with memory, latency, admission
rejections and OOM evidence kept separate. Resource edges need production
headroom; they are not recommended defaults.

`gateway_buffered_requests` is a gauge of retained managed bodies, not a count
of requests per second or all requests waiting for a service response. A low
value can coexist with many upstream requests in flight after their uploads
have been consumed. Evaluate admission rejections together with CPU, total pod
memory, service latency, and request rate when sizing replicas. Increasing the
slot limit permits more simultaneous body retention; it does not add CPU or
establish that the higher offered load is sustainable.

The Debian image also fixes glibc's mmap threshold at 128 KiB. Without an
explicit threshold, glibc can raise it dynamically after large allocations,
and freed bodies or authentication workspaces can leave high resident memory
in fragmented arenas. A fixed threshold keeps large allocations eligible for
return to the operating system when freed. This does not change live-buffer
limits or bound total RSS. Set the variable before process startup when using
a native glibc build; allocator-specific overrides remain operator controlled.
See the [glibc allocator documentation](https://sourceware.org/glibc/manual/latest/html_node/Memory-Allocation-Tunables.html).

`RELAYNA_STUDIO_BASE_URL` and `RELAYNA_STUDIO_TOKEN` are fallback settings.
Operators can set, replace, test, or clear the Studio connection in Admin portal
Settings after startup. Admin-saved settings override environment settings until
the persisted base URL is cleared. The base URL must point to the Studio
backend. For local development this is commonly `http://127.0.0.1:8000`; for
Docker Desktop from a container to a host backend use
`http://host.docker.internal:8000`; for Kubernetes use the backend Service DNS
name such as `http://relayna-studio-backend:8000`.

Validate the connection in two steps:

```bash
curl -sS "$RELAYNA_STUDIO_BASE_URL/studio/gateway/services"
curl -sS \
  -H "Authorization: Bearer $GATEWAY_OPERATOR_TOKEN" \
  -X POST \
  http://127.0.0.1:8081/admin-ui/admin/studio/connection/test
curl -sS \
  -H "Authorization: Bearer $GATEWAY_OPERATOR_TOKEN" \
  http://127.0.0.1:8081/admin-ui/admin/studio/services
```

The first command proves Studio exports services. The test route proves Gateway
can reach the effective configured connection. The services route proves Gateway
can fetch and map the export.

### LiteLLM Credential Header And Mapping

`LITELLM_BASE_URL` and `LITELLM_SERVICE_KEY` remain the startup fallback for
LiteLLM passthrough. In production, prefer managing the active LiteLLM provider
from Admin portal Providers so operators can rotate the provider default
credential and configure the upstream credential header without changing
deployment environment variables.

For LiteLLM installations that expect a custom header, set the LiteLLM provider
credential mode to `custom_header` and set the header name, for example
`x-litellm-api-key`. Gateway then sends only that custom header upstream. It
does not also send `Authorization` for LiteLLM in custom-header mode.

LiteLLM credential mappings are write-only. Add them from the same Providers
view:

1. Choose `key` scope to bind one LiteLLM virtual key to one Relayna virtual
   key, or choose `project` scope to bind one LiteLLM virtual key to every
   Relayna key in that project.
2. Select the target key or project.
3. Paste the LiteLLM virtual key into the write-only credential field.
4. Keep the mapping enabled and save it.

Gateway resolves LiteLLM credentials in this order for LiteLLM routes only:
Relayna key mapping, project mapping, active LiteLLM provider default
credential, then the `LITELLM_SERVICE_KEY` startup fallback when no active
provider config overrides it. Disabled mappings are skipped and fall back to
the next level. If no credential exists at any level, Gateway fails closed with
the upstream configuration error behavior.

### LiteLLM Wildcard Passthrough And Route Modes

LiteLLM wildcard passthrough lets Gateway be the only public ingress in front
of LiteLLM. Use it when clients can send standard Gateway credentials but
cannot send the custom LiteLLM header required by your LiteLLM deployment.
Gateway accepts Relayna credentials, strips all client credentials, and injects
the resolved internal LiteLLM credential before forwarding.

For routes inheriting gateway settings, client authentication remains Gateway
authentication:

| Gateway auth mode | Client request contract |
| --- | --- |
| Entra disabled | `Authorization: Bearer <Relayna rk_live_... key>` |
| Entra enabled | `Authorization: Bearer <Entra JWT>` plus the configured Relayna key header, default `X-Relayna-Key: <Relayna rk_live_... key>` |
| Trusted Apigee mode | Apigee supplies the signed identity headers and the configured Relayna key header. |

Do not give clients the LiteLLM master key or LiteLLM virtual keys as Gateway
credentials. Those secrets are upstream credentials selected by Gateway through
the mapping/default/fallback precedence above.

When a route uses authentication profiles, its selected profile determines the
credential requirements instead. Both types require an assigned Relayna key;
Entra profiles also require verified identity. Direct forwarding and trusted
ingress shortcuts do not bypass saved profiles. See the
[profile guide](operations/authentication-profiles.md).

Configure passthrough from Admin portal Providers:

1. Enable `LiteLLM passthrough`.
2. Set `Allowed paths`. For OpenAI-compatible LiteLLM API passthrough, use
   `/v1/*`. Add `/ui` and `/ui/*` only when the deployment has a deliberate
   operator access pattern.
3. Set `Allowed methods`. The safe default is `GET,POST`.
4. Choose `LiteLLM UI exposure` and `LiteLLM admin API exposure`.
5. Save settings and verify the audit event.

Exposure modes are intentionally separate from the path allowlist:

| Mode | Operational effect |
| --- | --- |
| `disabled` | Sensitive paths are rejected even when listed in `Allowed paths`. Use this unless you are actively exposing the path. |
| `operator_only` | Sensitive paths require the Gateway Entra or trusted Apigee identity layer plus Relayna virtual-key auth. This is appropriate when an identity-aware browser/operator ingress adds the required Gateway auth context. If Entra/Apigee identity is not enabled on the proxy path, `operator_only` requests fail closed. |
| `explicitly_exposed` | Sensitive paths are reachable to authenticated Relayna virtual-key clients when also allowed by path and method. Treat this as a high-risk setting and protect it with network and identity controls. |
| `trusted_ingress` | Allows browser-safe LiteLLM UI access for trusted identity-aware ingress without Relayna credentials when requesting `/ui` and LiteLLM support endpoints. Sensitive non-UI passthrough paths such as `/v1/models` still require normal Relayna proxy auth checks. |

Canonical OpenAI-compatible and Anthropic-compatible routes have separate modes
on the Routes page:

| Route mode | What Gateway enforces |
| --- | --- |
| `managed_by_gateway` | Route enablement, Relayna auth, policy, model/provider allowlists, RPM/TPM, budgets, guardrails, provider forwarding, and full usage when provider responses expose accounting fields. |
| `direct_litellm_passthrough` | Route enablement, Relayna auth, policy, model/provider allowlists, RPM/TPM, budgets, credential stripping/injection, and direct LiteLLM forwarding. Guardrail body rewriting and token accounting are bypassed; usage is status-only. |

Wildcard non-canonical passthrough follows the allowlist/auth/audit model and
records reduced status-only usage. It does not
run Relayna policy, budget, guardrail, or token accounting because the route is
not one of the canonical governed generation routes.

Browser access to LiteLLM `/ui` through Gateway needs more than the passthrough
setting. A plain browser address-bar request cannot attach `Authorization` and
Relayna key headers by itself. In production, use an identity-aware ingress,
reverse proxy, or equivalent operator portal flow that supplies Gateway auth
for browser requests; otherwise open LiteLLM directly on a private operator
network.

## Health and Metrics

- `/admin-ui/healthz` confirms the process can serve the control API.
- `/admin-ui/readyz` checks PostgreSQL and Redis.
- `/admin-ui/metrics` exposes Prometheus text format.

Use readiness probes for traffic routing and liveness probes for process restart decisions. Do not use `/admin-ui/healthz` as a dependency readiness signal.

Current operator workflows add provider health state, circuit breaker
status, debug bundles, service import versions, trace-aware usage analytics,
and audit-event review to the control plane. See
[Current Feature Highlights](current-features.md) for a feature-oriented
overview with Admin UI screenshots.

For registered-service failures, filter Usage by `method`, effective `endpoint`,
numeric `status_code`, or the existing `status=failure`. The effective endpoint
is the most-specific synced OpenAPI template with a query-free concrete-path
fallback. This captures both Gateway-produced and upstream statuses of 400 or
greater; it does not identify failure origin. Endpoint detail remains in
PostgreSQL-backed usage APIs and is intentionally not added to Prometheus
labels.

Prometheus metrics are intentionally low-cardinality. Metric labels are bounded
to route, provider, status class, decision kind, denial reason, circuit state,
guardrail name, guardrail mode, guardrail action, failure policy, and stream
mode. Do not add request IDs, trace IDs, raw virtual keys, prompt text, raw
paths, or unbounded model/user values as metric labels.

Core metric names:

| Metric | Type | Labels |
| --- | --- | --- |
| `gateway_requests_total` | counter | none |
| `gateway_requests_by_dimension_total` | counter | `route`, `provider`, `status_class` |
| `gateway_request_duration_ms` | histogram | `route`, `provider`, `stream` |
| `gateway_upstream_duration_ms` | histogram | `route`, `provider`, `stream` |
| `gateway_guardrail_duration_ms` | histogram | `route`, `provider`, `stream` |
| `gateway_first_token_latency_ms` | histogram-compatible counter and buckets | `route`, `provider`, `stream` on buckets |
| `gateway_auth_failures_total` | counter | none |
| `gateway_denials_total` | counter | `kind`, `route`, `reason` |
| `gateway_rate_limit_rejections_total` | counter | none |
| `gateway_budget_rejections_total` | counter | none |
| `gateway_provider_fallbacks_by_provider_total` | counter | `from_provider`, `to_provider`, `reason` |
| `gateway_active_requests` | gauge | none |
| `gateway_active_streams` | gauge | none |
| `gateway_circuit_breaker_state` | gauge | `provider`, `name`, `state` |

Example Prometheus scrape configuration:

```yaml
scrape_configs:
  - job_name: relayna-gateway
    metrics_path: /admin-ui/metrics
    static_configs:
      - targets: ["relayna-gateway-control:8081"]
```

Grafana panels should prefer request rate, p95 request/upstream duration,
first-token latency, denials by kind, guardrail block count, fallback rate,
active streams, buffered request/byte gauges, body-admission rejections, and
circuit state. Use `route` and `provider` filters only from the bounded label
sets emitted by the gateway.

## Tracing

Gateway preserves valid W3C `traceparent` headers on upstream provider and
service calls. When `traceparent` is present, the 32-character trace ID is stored
on usage events and request debug bundles so operators can move from Studio
analytics to provider traces or gateway logs without exposing raw keys or
prompts.

JSON logs include tracing span fields from gateway request, auth verification,
policy evaluation, guardrail, rate-limit, budget, upstream, and usage recording
points. Configure `LOG_LEVEL` with standard Rust tracing filters, for example:

```bash
LOG_LEVEL=info,gateway_proxy=debug,gateway_api=info
```

`ENTRA_AUTH_DEBUG=true` enables its dedicated warning-level diagnostic target
even when `LOG_LEVEL` would normally hide debug events. Do not use it as a
general verbosity switch: decoded Entra claims can contain personal data. Use
the [Entra Authorization Debug Mode](operations/entra-authorization-debug.md)
runbook to enable, interpret, and disable it.

If logs are shipped to an OpenTelemetry collector through the deployment
platform, map the `otel.trace_id` field and `traceparent` header to the same
trace context. The gateway does not use request IDs or trace IDs as Prometheus
labels.

## Budgets and Rate Limits

Virtual key policies can set request-per-minute (`rpm_limit`),
token-per-minute (`tpm_limit`), daily budget, and monthly budget limits. Request
and token rate limits are Redis minute counters. Budget checks use Redis daily
and monthly counters for fast enforcement, while PostgreSQL usage events remain
the durable accounting ledger.

Gateway admits estimated cost and reserves it in one Redis operation against
committed spend plus outstanding reservations. Actual provider cost can exceed
the estimate; these limits do not guarantee an absolute actual-cost ceiling.
Reservations use private server IDs, independently of caller correlation IDs.
Completion and cancellation consume each reservation once, using its original
UTC period even if the request crosses midnight.

Startup and periodic reconciliation seed committed daily/monthly spend from
PostgreSQL for active keys, including inherited budgets. Seeding never lowers
committed counters or overwrites outstanding reservations. New budgeted keys and
periods hydrate on first admission. Configure Redis with `maxmemory-policy
noeviction` and enough capacity for counters and reservations. Missing or invalid
limited-key counters fail closed until durable spend can be hydrated.

Optional historical service repricing updates reporting costs without changing
committed budget spend. Budget reconstruction uses the preserved original
`budget_estimated_cost` when present, otherwise the recorded `estimated_cost`.
A preserved zero remains zero even if the report price becomes positive. This
also applies to startup/reconciliation and Accessa durable spend reads; existing
Redis counters are not rewritten. Upgrade all budget readers before enabling
historical updates. See [service pricing](openapi-service-pricing.md#changing-prices-for-recorded-requests)
and [deployment](deployment.md#upgrade-to-0142).

The accounting epoch prevents a running replica from silently rearming after
Redis state loss. PostgreSQL cannot reconstruct in-flight reservations. After a
Redis restart without retained state, flush, or snapshot rollback, drain all
requests and restart every gateway replica before recovering from the durable
ledger. Reconciliation alone cannot recover outstanding requests.

**Upgrade requirement:** drain old replicas and in-flight requests before
introducing this accounting writer. Do not run old and new writers together.
Existing committed counter key names are retained; new outstanding keys append
`:reserved` and reservation values carry a `v2` suffix. Legacy reservation
values fail closed; resolve them through a drained recovery before resuming.

Requests that exceed `tpm_limit` return the stable
`token_rate_limit_exceeded` error. When Redis exposes the active bucket TTL,
Gateway includes retry timing in the response.

## Authentication and UI Resource Bounds

JWKS refreshes share one in-flight operation and a minimum 30-second interval,
including failed attempts. Unknown key IDs cannot trigger additional refreshes
inside that interval. Signing-key rotation is picked up after the interval;
configured cache lifetimes below 30 seconds use a 30-second floor.

The LiteLLM operator cookie always uses `Secure`, `HttpOnly`, and `SameSite=Lax`.
Serve the operator UI over HTTPS. Its existing bearer-token value and one-hour
browser lifetime are retained; the browser lifetime does not revoke the token.

LiteLLM UI text rewriting collects at most 2 MiB of upstream bytes per response, with eight buffers
admitted concurrently. The admission permit remains held while buffered bytes
await downstream consumption. Larger resources stream unchanged, including
chunked responses without a declared length. Other resource types stream
without collection. Exhausted rewrite capacity returns `gateway_overloaded`.

## Secret Handling

- Store `DATABASE_URL`, `REDIS_URL`, provider credentials, LiteLLM credentials, Studio tokens, and operator tokens in a secret manager.
- Never log raw virtual keys, operator tokens, provider keys, prompts, or request bodies.
- Use `GATEWAY_ADMIN_TOKEN` only to seed a fresh database. After an active
  operator token exists, env changes are ignored; rotate the token from the
  Admin portal to change it.
- Assign the narrowest operator scopes practical for automation. Use
  `audit:read` for audit-only readers, `usage:read` and `usage:export` for
  analytics workflows, and mutation scopes such as `keys:create`,
  `keys:disable`, `providers:update`, or `services:update` only where needed.
- Review `/admin-ui/admin/audit-events` after key, policy, guardrail, provider,
  service, Studio settings, or operator token changes. Audit rows include
  request ID, actor token ID, action, target, IP, user agent, and redacted
  before/after snapshots.
- Prefer private control-plane access for `/admin-ui/admin/*`, `/admin-ui`, and `/admin-ui/metrics`.
- Configure `RELAYNA_WORKER_TOKEN` only through secret management. Worker token
  verification uses constant-time comparison, and the gateway strips
  `x-relayna-worker-token` before forwarding upstream.
- Treat non-expiring virtual keys as high-risk service credentials. Store them
  only in a secret manager, scope their policies narrowly, rotate them through
  an external process, and revoke or disable them immediately when ownership or
  deployment context changes.

## Backup and Retention

Back up PostgreSQL because it contains virtual key metadata, policies, usage events, service registry state, and operator token hashes. Preserve Redis state while requests are active. PostgreSQL can rehydrate completed spend, but cannot recover request-per-minute, token-per-minute, or in-flight reservation state. Use the drained recovery procedure above after Redis loss.

## Upgrade Notes

Before deploying a new release:

1. Read `CHANGELOG.md`.
2. Build and scan the Docker image.
3. Run CI, including Rust checks, security scans, admin UI tests, and docs
   build.
4. Confirm PostgreSQL migrations apply in a staging database.
   For `0.1.26`, verify existing operator-token access remains available, new
   Entra identities start pending, exact service and project memberships scope owner data,
   and unmanaged workload identities are denied.
5. Confirm release metadata validation passes for the intended tag, for example `python3 scripts/validate-release-metadata.py v0.1.39`.
6. Roll out one gateway replica and check `/admin-ui/readyz`, `/admin-ui/metrics`, proxy traffic, route toggles, service routes, and the admin portal before scaling out.

## Supply Chain and Runtime Hardening

CI runs dependency, secret, static-analysis, filesystem, and image security
checks. Treat failures as blocking unless a temporary exception is documented in
`docs/security-exceptions.md`.

Release images are published to GHCR with SBOM, signature, and provenance
artifacts. Verify signatures and attestations before promotion into production
clusters.

Run production pods with the restricted settings from
`deploy/kubernetes/relayna-gateway.yaml`: non-root UID/GID `10001`, read-only
root filesystem, default seccomp profile, no privilege escalation, and no Linux
capabilities. Keep proxy and control-plane Services separate, and expose the
control plane only through private ingress, VPN, identity-aware proxy, or
equivalent access control.
