# Provider Intelligence

Provider intelligence turns Relayna Gateway routing into an observable control
plane while preserving the released proxy routes and credential handling.

## Routing Strategies

Routing decisions are represented in `gateway-core` and can be evaluated without
Axum or Pingora types. Supported strategies are:

- `priority`: choose the lowest numeric priority after constraints pass.
- `weighted`: choose from configured weights using a stable request hash.
- `least_latency`: choose the lowest observed average latency.
- `least_cost`: choose the lowest estimated request cost.
- `health_aware`: prefer low-latency healthy providers and exclude unhealthy or
  open-circuit providers.
- `budget_aware`: exclude providers whose remaining budget is lower than the
  estimated request cost, then choose the lowest cost.
- `region_affinity`: require the preferred region before priority selection.
- `capability_aware`: require all requested capabilities before priority
  selection.

Ambiguous provider state fails closed. Disabled, unhealthy, over-budget, missing
capability, mismatched-region, and open-circuit candidates are rejected before a
provider is selected.

## Fallback Policy

Fallback is conservative and retry-safe. Gateway retries only configured safe
classes:

- HTTP `429`, `500`, `502`, `503`, and `504`.
- Upstream read/write timeout classes.
- Provider failover only when an alternate gateway-managed upstream is
  configured.

The default policy allows two total attempts and a cooldown window. Existing
client routes and upstream credential stripping remain unchanged: client
`Authorization`, `Proxy-Authorization`, and `x-api-key` headers are removed
before gateway-managed upstream credentials are injected.

## Circuit Breakers and Health State

Provider health state is persisted separately from usage aggregates so
operators can inspect active and passive health:

- `status`: `healthy`, `degraded`, `unhealthy`, or `unknown`.
- `circuit_state`: `closed`, `open`, or `half_open`.
- `active_check_ok`: result of the latest active health validation when known.
- `passive_success_count` and `passive_failure_count`.
- `consecutive_failures`.
- `average_latency_ms`.
- `last_error_code`.
- `cooldown_until`, `checked_at`, and `updated_at`.

An open circuit excludes the provider until its cooldown has elapsed. A
half-open provider can be selected for recovery probing, and a successful
passive result closes the circuit.

## Debug Bundles

Request debug bundles are keyed by `request_id` and are designed for operator
inspection without exposing secrets or full prompts. Bundles may include:

- Route match, provider, service name, and backend selection trace.
- Policy trace and guardrail trace.
- Upstream latency.
- Retry and fallback history.
- Redacted request and response hashes.

Debug bundles do not store provider credentials, LiteLLM keys, bearer tokens,
raw request bodies, raw response bodies, or prompt text. Body fields are hashed
with a bounded prefix and length marker.

## Service Import Preview and Rollback

Service registry imports now support an operator workflow:

- Preview reports added, changed, removed, and invalid services without changing
  runtime registrations.
- Activation validates the import, writes a snapshot version, and imports each
  service through the existing Studio-compatible service path.
- Version history lists prior import snapshots.
- Rollback reactivates a stored snapshot and writes a new rollback snapshot that
  records the source version.

Rollback snapshots use the same redaction rules as normal service records:
runtime credentials are write-only and are not returned to clients.
