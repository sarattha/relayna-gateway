# OpenAPI Service Import and Endpoint Pricing

Relayna Gateway can discover the operations exposed by a registered service
from an OpenAPI 3.x JSON document and assign billing per HTTP method and path.
Discovery is an explicit control-plane workflow: Gateway previews the document,
an operator reviews drift, and a separate sync persists the endpoint catalog
and pricing rules. Gateway never downloads OpenAPI while proxying user traffic.

This feature is useful when one service exposes both billable work and
operational endpoints. For example, an OCR service can charge for `POST /ocr`
while Relayna feeds, status, events, DLQ, execution, and health operations stay
free.

## Prerequisites

Before discovery, the service registration must have:

- a stable service name and route pattern;
- an absolute HTTP or HTTPS upstream base URL reachable by Gateway;
- every client method that Gateway should forward in `allowed_methods`;
- a service default cost mode and an estimate when that mode is `fixed`; and
- an operator token with `services:update` scope.

The service should publish OpenAPI 3.x JSON at a relative path on its own
origin. `/openapi.json` is the default and preferred source. Swagger UI HTML at
`/docs` is not an import source.

## Import from the Admin UI

1. Open **Discover → Services** and create or edit the service.
2. Confirm the upstream URL, allowed methods, service cost mode, and default
   estimate are correct.
3. In the OpenAPI endpoint pricing area, enter `/openapi.json` and choose
   **Preview OpenAPI**.
4. Review the document title/version, schema hash, discovered operations, and
   the added/removed drift lists. Preview does not mutate the service.
5. Choose **Sync endpoint pricing** only after the preview matches the intended
   upstream deployment.
6. Review each method/path row, change its cost mode or fixed estimate where
   needed, and save the service.

Sync stores the source path, schema hash, sync timestamp, compact endpoint
catalog, and endpoint pricing rules in PostgreSQL. A later preview compares the
current document with that durable snapshot. Prices for unchanged method/path
operations are preserved across syncs. Removed operations are reported before
sync, and their existing pricing rules are retained as stale safety state until
an operator intentionally edits or removes them.

## Import with the Admin API

Preview the service document:

```bash
export GATEWAY_CONTROL_URL="http://127.0.0.1:8081"
export GATEWAY_OPERATOR_TOKEN="op_live_replace_with_secret_value"
export SERVICE_NAME="ocr-service"

curl -sS \
  -H "Authorization: Bearer $GATEWAY_OPERATOR_TOKEN" \
  -H "Content-Type: application/json" \
  -X POST \
  "$GATEWAY_CONTROL_URL/admin-ui/admin/services/$SERVICE_NAME/openapi/preview" \
  -d '{"source_path":"/openapi.json"}' \
  | tee /tmp/relayna-openapi-preview.json
```

The response includes `schema_hash`, `endpoints`, `added`, and `removed`.
Inspect it, then sync the exact previewed hash:

```bash
export OPENAPI_SCHEMA_HASH="$(jq -r '.schema_hash' /tmp/relayna-openapi-preview.json)"

curl -sS \
  -H "Authorization: Bearer $GATEWAY_OPERATOR_TOKEN" \
  -H "Content-Type: application/json" \
  -X POST \
  "$GATEWAY_CONTROL_URL/admin-ui/admin/services/$SERVICE_NAME/openapi/sync" \
  -d "{\"source_path\":\"/openapi.json\",\"expected_schema_hash\":\"$OPENAPI_SCHEMA_HASH\"}"
```

Gateway fetches the document again during sync. If its hash changed after the
preview, sync fails with `service_openapi_changed`; preview again instead of
accepting unreviewed billing drift.

Endpoint prices can also be updated through the existing service PATCH API:

```bash
curl -sS \
  -H "Authorization: Bearer $GATEWAY_OPERATOR_TOKEN" \
  -H "Content-Type: application/json" \
  -X PATCH \
  "$GATEWAY_CONTROL_URL/admin-ui/admin/services/$SERVICE_NAME" \
  -d '{
    "endpoint_pricing_rules": [
      {
        "method": "GET",
        "path_template": "/events/feed",
        "operation_id": "feed_events_feed_get",
        "cost_mode": "none"
      },
      {
        "method": "POST",
        "path_template": "/ocr",
        "operation_id": "submit_ocr_ocr_post",
        "cost_mode": "fixed",
        "estimated_cost_usd": 0.01
      }
    ],
    "pricing_rules": [
      {
        "name": "docint",
        "json_pointer": "/engine",
        "equals": "docint",
        "cost_mode": "fixed",
        "estimated_cost_usd": 0.5
      }
    ]
  }'
```

Submit the complete endpoint and body-rule lists that should remain on the
service; these PATCH fields replace their respective stored lists.

## Cost Modes

| Mode | Accounting behavior |
| --- | --- |
| `none` | Records the request with no estimated cost and makes no cost-based budget reservation. |
| `fixed` | Uses `estimated_cost_usd`; the estimate is required and must be non-negative. |
| `passthrough` | Reads a supported cost field from the upstream response. If none is present, usage records `missing_upstream_cost`. |

Newly discovered endpoints under `/events`, `/status`, `/dlq`, `/broker/dlq`,
`/failed-tasks`, `/relayna`, and `/executions`, plus exact `/health` and
`/history`, default to `none`. This classification is only a billing default.
Operators may change those endpoints to `fixed` or `passthrough`, and other
endpoints inherit the service default when first synced.

Endpoint matching is method-aware and segment-aware. OpenAPI templates such as
`/executions/{execution_id}` match one concrete path segment. When multiple
templates could match, Gateway prefers the template with the most static path
segments.

## Endpoint and Request-Body Cost Precedence

Gateway resolves registered-service pricing in this order:

1. Match the request method and rewritten upstream path against an endpoint
   pricing rule.
2. If the matched endpoint is `none`, stop: the request is unpriced and body
   selectors cannot make it billable.
3. For a `fixed` or `passthrough` endpoint, evaluate the service's body pricing
   rules. A matching JSON or multipart selector overrides the endpoint base.
4. If no endpoint rule matches, retain the legacy service-default and body-rule
   behavior.

For JSON requests, `json_pointer` follows JSON Pointer syntax. `/engine`
selects the top-level `engine` field; `/payload/page_count` selects a nested
field. The request itself still uses normal JSON keys.

For `multipart/form-data`, every bounded non-file UTF-8 field becomes a
top-level string selector. Therefore this OCR request matches `/engine =
docint` and resolves to `$0.50`:

```bash
curl -sS \
  -H "Authorization: Bearer $RELAYNA_API_KEY" \
  -F "engine=docint" \
  -F "file=@sample.pdf;type=application/pdf" \
  "http://127.0.0.1:8080/services/ocr-service/ocr"
```

Uploaded file bytes are never copied into pricing metadata. Multipart selector
extraction is bounded to 128 fields, 256 bytes per field name, 16 KiB per field
value, and 64 KiB total. Oversized, non-UTF-8, or file fields do not match body
rules and therefore fall back to the endpoint or service base.

## Budgets and Usage

Gateway must reserve cost before it knows which JSON or multipart body rule
will match. For every billable endpoint, preflight therefore uses the highest
fixed estimate among that endpoint base and all service body rules. Final usage
reconciles the reservation to the resolved rule. Configure
`max_cost_per_request` and daily/monthly budgets to permit the highest variant
that a key may submit.

An endpoint set to `none` bypasses this unrelated selector ceiling. It still
requires successful authentication and policy checks and still writes a usage
event with cost mode/source `none`. Fixed and passthrough endpoint usage uses
the operation ID, when present, as `pricing_rule_name`; a named body selector
replaces it when that selector matches.

## Security Requirements

OpenAPI discovery deliberately is not a general URL fetcher:

- `source_path` must start with one `/`, be at most 512 characters, and contain
  no authority, query, fragment, backslash, double slash, or control character;
- Gateway combines the path with the registered upstream origin only;
- redirects are disabled;
- registered service credentials are not forwarded;
- the request accepts only JSON responses and has an eight-second timeout;
- the response body is limited to 1 MiB;
- only OpenAPI 3.x documents are accepted, with at most 500 supported HTTP
  operations; and
- external `$ref` targets are not fetched.

Preview and sync require `services:update`; successful sync writes an audit
event. Endpoint cost mode does not authorize traffic. Virtual-key route and
service policy, project/key service links, registered `allowed_methods`, body
limits, timeouts, rate limits, budgets, guardrails, and credential stripping
remain independent enforcement layers. Keep operational mutation endpoints
such as DLQ retry or failed-task management restricted even when their billing
mode is `none`.

## Troubleshooting

| Symptom | Check |
| --- | --- |
| Preview reports unavailable | Confirm Gateway can reach the registered upstream and that `/openapi.json` returns HTTP 2xx within eight seconds. |
| Preview reports invalid OpenAPI | Confirm `Content-Type` contains `json`, the body is at most 1 MiB, `openapi` starts with `3.`, and `paths` contains supported method objects. |
| Sync reports `service_openapi_changed` | The upstream schema changed after preview. Preview again and review the new hash/drift. |
| `docint` uses the endpoint/default price | Confirm the request is multipart, `engine` is a non-file UTF-8 field, and the rule uses `json_pointer: "/engine"` with `equals: "docint"`. |
| A fixed request is denied before forwarding | Raise the key's `max_cost_per_request` or budget to cover the highest fixed body-rule ceiling allowed on that billable service endpoint. |
| A free endpoint is inaccessible | Billing `none` does not grant access. Check service links, virtual-key policy, route pattern, and `allowed_methods`. |

See [Admin Portal](admin-portal.md) for the full service setup flow and
[Database](database.md) for the durable service-registration fields.
