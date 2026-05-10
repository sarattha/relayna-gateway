# Deployment

Relayna Gateway ships as one binary and one Docker image. The image serves both the core proxy and the admin portal because the admin UI is embedded in the `gateway-api` binary.

## Docker Image

Build the image:

```bash
docker build -t relayna-gateway:0.0.2 .
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
  -e GATEWAY_BIND_ADDR="0.0.0.0:8080" \
  -e GATEWAY_CONTROL_BIND_ADDR="0.0.0.0:8081" \
  -e LOG_LEVEL="gateway_api=info,gateway_proxy=info" \
  relayna-gateway:0.0.2
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

1. Build and publish the image:

   ```bash
   docker build -t ghcr.io/sarattha/relayna-gateway:0.0.2 .
   docker push ghcr.io/sarattha/relayna-gateway:0.0.2
   ```

2. Update the Deployment image:

   ```yaml
   image: ghcr.io/sarattha/relayna-gateway:0.0.2
   ```

3. Store secrets through your cluster secret manager:

   ```bash
   kubectl create secret generic relayna-gateway-secrets \
     --from-literal=DATABASE_URL='postgres://user:password@postgres:5432/relayna_gateway' \
     --from-literal=REDIS_URL='redis://redis:6379' \
     --from-literal=LITELLM_BASE_URL='http://litellm:4000' \
     --from-literal=LITELLM_SERVICE_KEY='sk-litellm-service-key'
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
