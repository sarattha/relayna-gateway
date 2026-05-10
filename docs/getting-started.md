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

## Run Checks

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
node tests/admin-ui.test.mjs
```
