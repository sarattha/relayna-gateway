# Deployment

Relayna Gateway ships as one binary and one Docker image. The image serves both the core proxy and the admin portal because the admin UI is embedded in the `gateway-api` binary.

## Docker Image

Build the image:

```bash
docker build -t relayna-gateway:0.0.11 .
```

Run it with required dependencies:

```bash
docker run --rm \
  -p 8080:8080 \
  -p 8081:8081 \
  -e DATABASE_URL="postgres://user:password@host.docker.internal:5432/relayna_gateway" \
  -e REDIS_URL="redis://host.docker.internal:6379" \
  -e LITELLM_BASE_URL="http://host.docker.internal:4000" \
  -e LITELLM_SERVICE_KEY="sk-litellm-service-key" \
  -e GATEWAY_ADMIN_TOKEN="op_live_replace_with_secret_value" \
  -e RELAYNA_STUDIO_BASE_URL="http://host.docker.internal:8000" \
  -e GATEWAY_BIND_ADDR="0.0.0.0:8080" \
  -e GATEWAY_CONTROL_BIND_ADDR="0.0.0.0:8081" \
  -e LOG_LEVEL="gateway_api=info,gateway_proxy=info" \
  relayna-gateway:0.0.11
```

The proxy listens on port `8080`. The control API, admin portal, readiness, and metrics listen on port `8081`.

`GATEWAY_ADMIN_TOKEN` is optional. Set it only for the first startup against a
fresh database when you want to seed a known `op_live_...` operator token. Omit
it to let Gateway generate and print a one-time operator token. After an active
operator token exists in PostgreSQL, env changes are ignored; rotate the token
from the Admin portal to change it.

## PostgreSQL Container

For local container testing:

```bash
docker run --rm --name relayna-postgres \
  -p 5432:5432 \
  -e POSTGRES_USER=relayna_gateway \
  -e POSTGRES_PASSWORD=relayna_gateway \
  -e POSTGRES_DB=relayna_gateway \
  postgres:16
```

Use:

```text
postgres://relayna_gateway:relayna_gateway@host.docker.internal:5432/relayna_gateway
```

## Redis Container

```bash
docker run --rm --name relayna-redis -p 6379:6379 redis:7
```

Use:

```text
redis://host.docker.internal:6379
```

## Kubernetes

The repository includes a baseline manifest at `deploy/kubernetes/relayna-gateway.yaml`.

1. Use the image published by the tag-based release workflow:

   ```text
   ghcr.io/sarattha/relayna-gateway:0.0.11
   ```

   To build and publish manually to another registry:

   ```bash
   export RELAYNA_GATEWAY_IMAGE="<your-registry>/<your-org>/relayna-gateway:0.0.11"
   docker build -t "$RELAYNA_GATEWAY_IMAGE" .
   docker push "$RELAYNA_GATEWAY_IMAGE"
   ```

2. Update the Deployment image when you use a different registry or tag:

   ```yaml
   image: <your-registry>/<your-org>/relayna-gateway:0.0.11
   ```

3. Store secrets through your cluster secret manager:

   ```bash
   kubectl create secret generic relayna-gateway-secrets \
     --from-literal=DATABASE_URL='postgres://user:password@postgres:5432/relayna_gateway' \
     --from-literal=REDIS_URL='redis://redis:6379' \
     --from-literal=LITELLM_BASE_URL='http://litellm:4000' \
     --from-literal=LITELLM_SERVICE_KEY='sk-litellm-service-key' \
     --from-literal=GATEWAY_ADMIN_TOKEN='op_live_replace_with_secret_value' \
     --from-literal=RELAYNA_STUDIO_BASE_URL='http://relayna-studio-backend:8000' \
     --from-literal=RELAYNA_STUDIO_TOKEN='studio-gateway-token'
   ```

4. Apply the manifest:

   ```bash
   kubectl apply -f deploy/kubernetes/relayna-gateway.yaml
   ```

5. Verify readiness:

   ```bash
   kubectl rollout status deployment/relayna-gateway
   kubectl port-forward svc/relayna-gateway 8081:8081
   curl http://127.0.0.1:8081/admin-ui/readyz
   ```

## Network Exposure

Expose the proxy port to clients that need LLM traffic. Keep the control port
private or protected by internal ingress, VPN, identity-aware proxy, or strict
network policy.

All Gateway control-plane paths are rooted under `/admin-ui` so an AKS ingress
can route `/admin-ui` and `/admin-ui/*` to Relayna Gateway even when another
gateway owns `/`, `/healthz`, `/readyz`, and `/metrics`. Use
`/admin-ui/healthz`, `/admin-ui/readyz`, and `/admin-ui/metrics` for probes and
scrapers.

## Guardrail Configuration

Database migrations create the guardrail catalog, key policy, execution event,
and per-key override tables/columns on startup. The built-in `pii-redact`
catalog entry is enabled but not default-on, so existing keys keep unguarded
behavior until an operator selects guardrails for the key.

Use Admin portal Guardrails to manage global catalog defaults:

- `runtime_config` is the actual default config used when a guardrail runs.
- `config_schema` documents the shape operators should use for runtime config.
- HTTP guardrail endpoint URL, timeout, and bearer token are separate catalog
  fields. Bearer tokens are write-only and are never returned by the API.

Use Admin portal Keys to manage each key:

- `mandatory_guardrails` always run for that key.
- `optional_guardrails` may run when the client requests them.
- `forbidden_guardrails` are hidden from discovery and rejected if requested.
- `guardrail_config_overrides` tunes selected guardrails per key.

Per-key overrides are shallow-merged over the catalog runtime config. They must
be JSON objects, and they only take effect when the guardrail is applied by
mandatory, optional, default-on, or client-requested policy. For example, one
key can restore `pii-redact` placeholders in responses while another leaves
redacted placeholders in the final output:

```json
{
  "guardrail_policy": {
    "mandatory_guardrails": ["pii-redact"],
    "optional_guardrails": [],
    "forbidden_guardrails": [],
    "guardrail_config_overrides": {
      "pii-redact": {
        "restore_output": false
      }
    }
  }
}
```

When guarded traffic may stream, ensure every selected response guardrail
supports `during_call`; otherwise Gateway fails closed with
`guardrail_unavailable` instead of buffering an unsupported stream.

## Studio Import Connectivity

Gateway imports Studio services by calling the Studio backend endpoint
`GET /studio/gateway/services`. The configured Studio base URL should therefore
be the backend base URL. `RELAYNA_STUDIO_BASE_URL` and `RELAYNA_STUDIO_TOKEN`
remain startup fallbacks; operators can override them in Admin portal Settings
without restarting Gateway. Clearing the persisted base URL returns Gateway to
the environment fallback.

| Deployment shape | Example value |
| --- | --- |
| Gateway and Studio on the same host | `http://127.0.0.1:8000` |
| Gateway in Docker, Studio on host | `http://host.docker.internal:8000` |
| Gateway and Studio in Kubernetes | `http://relayna-studio-backend:8000` |
| Gateway to protected Studio over TLS | `https://studio.internal.example.com` |

Test Studio directly:

```bash
curl -sS "$RELAYNA_STUDIO_BASE_URL/studio/gateway/services"
```

Test through Gateway after startup:

```bash
curl -sS \
  -H "Authorization: Bearer $GATEWAY_OPERATOR_TOKEN" \
  -X POST \
  http://127.0.0.1:8081/admin-ui/admin/studio/connection/test

curl -sS \
  -H "Authorization: Bearer $GATEWAY_OPERATOR_TOKEN" \
  http://127.0.0.1:8081/admin-ui/admin/studio/services
```

If Gateway returns `studio_unavailable`, check that the backend URL is reachable
from the Gateway process, that the path `/studio/gateway/services` exists, that
the effective token matches Studio's expected token when authentication is
enabled, and that Studio returns valid service names and route patterns.
