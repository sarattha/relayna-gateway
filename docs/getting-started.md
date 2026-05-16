# Getting Started

These steps run Relayna Gateway locally with PostgreSQL, Redis, and an OpenAI-compatible upstream.

## Install Tools

- Rust stable with `cargo`, `rustfmt`, and `clippy`.
- PostgreSQL 14 or newer.
- Redis 6 or newer.
- Node.js 20 or newer for admin UI test checks.
- Python 3 for documentation checks.

## Start PostgreSQL

Create a database and user:

```bash
createdb relayna_gateway
createuser relayna_gateway
psql -d relayna_gateway -c "grant all privileges on database relayna_gateway to relayna_gateway;"
```

Set the gateway database URL:

```bash
export DATABASE_URL="postgres://relayna_gateway@localhost:5432/relayna_gateway"
```

The gateway runs bundled SQLx migrations on startup through `PostgresStore::connect`, so a fresh database is enough for local development.

## Start Redis

Run Redis locally:

```bash
redis-server
```

Set the Redis URL:

```bash
export REDIS_URL="redis://127.0.0.1:6379"
```

Redis stores rate-limit and budget counters. Do not point local development at production Redis.

## Configure Upstream Access

For LiteLLM:

```bash
export LITELLM_BASE_URL="http://127.0.0.1:4000"
export LITELLM_SERVICE_KEY="sk-litellm-service-key"
```

For an optional direct OpenAI-compatible upstream:

```bash
export DIRECT_OPENAI_BASE_URL="https://api.openai.com"
export DIRECT_OPENAI_SERVICE_KEY="sk-provider-key"
```

## Connect Relayna Studio

If Relayna Studio is running, point Gateway at the Studio backend export API.
Use the backend URL, not the frontend URL. You can set the connection in Admin
portal Settings after Gateway starts, or provide environment variables as a
startup fallback.

Local example:

```bash
export RELAYNA_STUDIO_BASE_URL="http://127.0.0.1:8000"
```

If the Gateway process runs in Docker and Studio runs on the host:

```bash
export RELAYNA_STUDIO_BASE_URL="http://host.docker.internal:8000"
```

If Studio requires a token for Gateway import:

```bash
export RELAYNA_STUDIO_TOKEN="studio-gateway-token"
```

Admin-saved Studio settings override these environment values. Clearing the
persisted base URL in Settings reveals the environment fallback again. Token
values are write-only in Gateway API responses.

Verify Studio exports services before starting Gateway:

```bash
curl -sS "$RELAYNA_STUDIO_BASE_URL/studio/gateway/services"
```

After Gateway starts, verify Gateway can reach Studio:

```bash
curl -sS \
  -H "Authorization: Bearer $GATEWAY_OPERATOR_TOKEN" \
  -X POST \
  http://127.0.0.1:8081/admin/studio/connection/test

curl -sS \
  -H "Authorization: Bearer $GATEWAY_OPERATOR_TOKEN" \
  http://127.0.0.1:8081/admin/studio/services
```

The Gateway response should include mapped services with `studio_service_id`,
`route_pattern`, and `import_request` fields. Open `/admin-ui`, go to Services,
and use `Import from Studio` to register selected services locally.

## Run the Gateway

```bash
export GATEWAY_BIND_ADDR="127.0.0.1:8080"
export GATEWAY_CONTROL_BIND_ADDR="127.0.0.1:8081"
export LOG_LEVEL="gateway_api=info,gateway_proxy=info"
cargo run -p gateway-api
```

The first startup prints a raw operator token once. Use it for the admin portal and store it securely.

## Verify Local Health

```bash
curl http://127.0.0.1:8081/healthz
curl http://127.0.0.1:8081/readyz
```

Open the admin portal at:

```text
http://127.0.0.1:8081/admin-ui
```

In the portal, create Projects first when service access should be shared by a
team or application. Open a Project's `Select services` picker to link imported
or locally registered services. In Keys, choose `Project` for keys that inherit
those links, or choose `Individual` and use `Select services` to link services
directly to one key.

## Create a Non-Expiring Key

In the Admin portal, open Keys and select `No expiration` when creating or
editing a virtual key. Through the API, use `expires_at: null` with an explicit
owner type:

```bash
curl -sS -X POST http://127.0.0.1:8081/admin/keys \
  -H "Authorization: Bearer $GATEWAY_OPERATOR_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "owner_type": "project",
    "project_id": "<project-id>",
    "expires_at": null
  }'
```

Use non-expiring keys only for service-to-service workloads that have a separate
rotation and revocation process. Keep their policies narrow and store the raw
key in a secret manager.

## Run Checks

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
node tests/admin-ui.test.mjs
```
