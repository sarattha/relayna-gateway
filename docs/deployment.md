# Deployment

Relayna Gateway ships as one binary and one Docker image. The image serves both the core proxy and the admin portal because the admin UI is embedded in the `gateway-api` binary.

## Docker Image

Build the image:

```bash
docker build -t relayna-gateway:0.0.6 .
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
  -e RELAYNA_STUDIO_BASE_URL="http://host.docker.internal:8000" \
  -e GATEWAY_BIND_ADDR="0.0.0.0:8080" \
  -e GATEWAY_CONTROL_BIND_ADDR="0.0.0.0:8081" \
  -e LOG_LEVEL="gateway_api=info,gateway_proxy=info" \
  relayna-gateway:0.0.6
```

The proxy listens on port `8080`. The control API, admin portal, readiness, and metrics listen on port `8081`.

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
   ghcr.io/sarattha/relayna-gateway:0.0.6
   ```

   To build and publish manually to another registry:

   ```bash
   export RELAYNA_GATEWAY_IMAGE="<your-registry>/<your-org>/relayna-gateway:0.0.6"
   docker build -t "$RELAYNA_GATEWAY_IMAGE" .
   docker push "$RELAYNA_GATEWAY_IMAGE"
   ```

2. Update the Deployment image when you use a different registry or tag:

   ```yaml
   image: <your-registry>/<your-org>/relayna-gateway:0.0.6
   ```

3. Store secrets through your cluster secret manager:

   ```bash
   kubectl create secret generic relayna-gateway-secrets \
     --from-literal=DATABASE_URL='postgres://user:password@postgres:5432/relayna_gateway' \
     --from-literal=REDIS_URL='redis://redis:6379' \
     --from-literal=LITELLM_BASE_URL='http://litellm:4000' \
     --from-literal=LITELLM_SERVICE_KEY='sk-litellm-service-key' \
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
   curl http://127.0.0.1:8081/readyz
   ```

## Network Exposure

Expose the proxy port to clients that need LLM traffic. Keep the control port private or protected by internal ingress, VPN, identity-aware proxy, or strict network policy.

## Studio Import Connectivity

Gateway imports Studio services by calling the Studio backend endpoint
`GET /studio/gateway/services`. The configured `RELAYNA_STUDIO_BASE_URL` should
therefore be the backend base URL:

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
  http://127.0.0.1:8081/admin/studio/services
```

If Gateway returns `studio_unavailable`, check that the backend URL is reachable
from the Gateway process, that the path `/studio/gateway/services` exists, that
`RELAYNA_STUDIO_TOKEN` matches Studio's expected token when authentication is
enabled, and that Studio returns valid service names and route patterns.
