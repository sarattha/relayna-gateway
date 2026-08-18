# Local project-owner inspection stack

This development-only Compose stack runs Relayna Gateway with PostgreSQL,
Redis, an Entra-shaped OIDC issuer, and an OpenAI-compatible mock upstream. It
uses ports in the `18380`-`19432` range so it does not collide with the normal
Gateway, PostgreSQL, or Redis defaults.

From the repository root, start it with:

    ./deploy/local/run.sh --rebuild

Then open <http://127.0.0.1:18381/admin-ui>, select **Sign in with Microsoft**,
and choose one of these development identities:

- **Analytics Project Owner** — active project Owner for the seeded `Analytics
  Platform` project and its read-only dashboard.
- **Gateway Administrator** — active Admin with member, project, service, and
  managed-identity controls.
- **Orders Service Owner** — active service Owner for `orders-api`.
- **Pending Service Owner** — exercises the pending-member state.

The seed is idempotent and refreshes 168 hourly usage events, failures, endpoint
and model breakdowns, version transitions, guardrail actions, and sanitized
debug bundles. The same development managed identity is bound to both
`analytics-api` and `Analytics Platform` with the shared
`gateway.monitor.read` application role.

Useful local endpoints:

- Admin UI and control API: <http://127.0.0.1:18381/admin-ui>
- Proxy listener: <http://127.0.0.1:18380>
- Mock upstream: <http://127.0.0.1:18382/health>
- Development OIDC issuer: <http://127.0.0.1:18390/health>
- PostgreSQL: `127.0.0.1:19432`
- Redis: `127.0.0.1:19379`

To request the managed-identity token and inspect the project API:

    ACCESS_TOKEN="$(curl --silent --request POST http://127.0.0.1:18390/token \
      --data-urlencode grant_type=client_credentials \
      --data-urlencode client_id=00000000-0000-0000-0000-000000000201 \
      --data-urlencode client_secret=relayna-development-monitor-secret \
      --data-urlencode scope=api://relayna-gateway-local/.default \
      | jq --raw-output .access_token)"
    curl --header "Authorization: Bearer ${ACCESS_TOKEN}" \
      http://127.0.0.1:18381/owner/v1/projects/10000000-0000-0000-0000-000000000001/dashboard

Stop the stack while preserving its local PostgreSQL volume with:

    docker compose -f deploy/local/docker-compose.yml down

To remove the mock database too, explicitly add `--volumes` to that command.
The generated private key is under `target/development-oidc` and is excluded
from the Docker build context and Git.
