# Cortex E2E gateway slice

This development-only slice runs the real released Relayna Gateway with its own
ephemeral PostgreSQL and Redis in `cortex-e2e` on the existing `kind-keystone`
cluster. Cortex owns the BFF, frontend and explicit Accessa/agent fixtures.
Keystone owns the actual HTTP MCP server and shared development OIDC service.
It does not establish production Accessa or Entra connectivity.

Before applying, create `relayna-gateway-secrets` in `cortex-e2e` containing:

- `POSTGRES_PASSWORD` and `DATABASE_URL` for `relayna-postgres:5432/relayna`.
- `REDIS_URL=redis://relayna-redis:6379`.
- A generated `GATEWAY_ADMIN_TOKEN` in the supported `op_live_` format, kept on
  the operator side.
- `LITELLM_BASE_URL` and `LITELLM_SERVICE_KEY` (mandatory startup configuration;
  this test exposes no LLM route to the Cortex channel key).

Cortex also creates `cortex-accessa-secrets.ACCESSA_ADAPTER_TOKEN`, which is the
gateway-owned upstream bearer and must match the mock adapter. Supply secret
content through files or tool input; do not paste secrets into command arguments,
logs, source, browser state or reports.

Build/load from a clean candidate, then apply and wait:

    docker build -t relayna-gateway:cortex-e2e-e1bf6d2 .
    kind load docker-image --name keystone relayna-gateway:cortex-e2e-e1bf6d2
    kubectl --context kind-keystone apply -f deploy/kind/cortex-e2e.yaml
    kubectl --context kind-keystone -n cortex-e2e rollout status deployment/relayna-postgres
    kubectl --context kind-keystone -n cortex-e2e rollout status deployment/relayna-gateway
    kubectl --context kind-keystone -n cortex-e2e port-forward service/relayna-gateway 18381:8081 18380:8080

When using a newer image, change the deployment image to the exact locally built
tag with `kubectl set image`; record candidate commit and Docker image digest.
PostgreSQL migrations run during gateway startup. Dependencies can cause the
first gateway start to restart until PostgreSQL/Redis become ready.

With the adapter secret in place and control port forwarded, bootstrap:

    python3 deploy/kind/bootstrap-cortex.py

The script uses supported admin APIs to create the dedicated project, service-only
inherited policy, service, channel virtual key and `entra_and_relayna_key`
authentication profile. It saves
the raw virtual key once to excluded `target/kind-cortex-e2e/relayna-channel.key`
with mode0600, prints only metadata and reuses the key on subsequent calls.
Unchanged reruns preserve the authentication profile revision and live admissions.
If that file is lost, rotate explicitly; raw keys cannot be recovered from the
database. Provision the BFF's Kubernetes Secret from the private file.

Keystone's default-deny namespace policy must allow the `cortex-e2e`
`app=relayna-gateway` pod to reach its development OIDC service on TCP5556.
Discovery and the advertised JWKS URL must both be reachable inside Kind.

The BFF sends user JWT in `Authorization` and its virtual key in `X-Relayna-Key`
to `/app/cortex/channel/web/v1/*`. Signed user tokens must match the configured
issuer, tenant, `api://keystone-mcp` audience, both delegated scopes and expiry.
WS only upgrades `GET .../v1/run`, using v13 without compression. Full paths are
preserved. The adapter receives its own bearer, overwritten trusted caller
headers, app/channel and an opaque admission context. Router must make an
empty-body POST to `http://relayna-gateway:8080/internal/accessa/admissions/{turn UUID}`
with that context before dispatching each turn. See the released
[Accessa contract](../../docs/accessa-channels.md).

For this prototype only, BFF also places the same server-owned delegated user
token in private `x-cortex-delegated-token`. Existing generic header forwarding
preserves it; Gateway does not independently authenticate this header. The mock
broker must verify the JWT signature, issuer, audience, expiry and tenant and
match token `oid`/`tid` to the trusted caller headers before Keystone delegation.
Remove it before agent dispatch and from every response or log. This explicit
fixture enables per-user ACL tests without a static owner token. Production
delegation remains subject to the confirmed Accessa credential-broker contract.

The gateway slice does not delete the namespace or existing Keystone resources.
Its database is ephemeral; recreating its pod loses development registrations
and requires a new bootstrap. Do not use this deployment for durable data.
