# Azure Foundry agents and endpoint passthrough

Relayna Gateway 0.1.38 can authenticate and govern requests before forwarding them
to Microsoft Foundry Agent Service. Add an **Azure Foundry connection** in
Providers, then register a **Foundry service**. Azure runs the agent and its tools;
Relayna handles caller authentication, service permissions, limits and traffic
records. The Rust gateway serves the same embedded Admin UI 4.0 deployment.

## Choose an integration mode

| Mode | Who chooses the agent/model? | Use when |
| --- | --- | --- |
| **Registered agent** | An administrator fixes the Foundry agent name and optionally its version. | Applications should invoke one approved existing agent. Recommended for your research or Bing-grounded agent. |
| **Foundry endpoint passthrough** | An authorized client supplies a model deployment or `agent_reference`, instructions and tools. | Trusted applications need the project's Responses API flexibility. |

Both expose **POST `/services/NAME/responses`**. They forward to the configured
project's `/openai/v1/responses` endpoint. Passthrough does **not** skip Relayna
verification and does not expose every Azure API.

```mermaid
flowchart LR
    C[Client: Relayna key and optional Entra token] --> G[Relayna: authentication profile and service policy]
    G --> A[Azure token for the configured gateway identity]
    A --> F[Foundry project Responses API]
    F --> R[JSON or SSE with citations]
    R --> C
```

## Prepare Azure

1. Have a modern Foundry project and a deployed model or an existing agent that
   supports the project Responses API. Copy its project endpoint, for example
   `https://research-demo.services.ai.azure.com/api/projects/research`.
2. Choose a gateway identity. Configure Azure permissions for that identity, not
   the Relayna virtual key or the user's incoming Entra token.
3. Grant the identity permission to invoke the required APIs on the project.
   Microsoft recommends **Foundry Agent Consumer** for agent consumers;
   **Foundry User** includes broader project data actions needed for development
   and project operations. Select the role appropriate to the API being used and
   confirm a direct project Responses call succeeds. Agent-scoped roles apply
   specifically to agent endpoints; this integration uses the **project** endpoint.
   See [Microsoft's current RBAC guidance](https://learn.microsoft.com/en-us/azure/foundry/concepts/rbac-foundry).
4. Allow outbound access from every gateway replica to the project's HTTPS
   endpoint and the Azure identity endpoint. Private endpoint deployments still
   need working DNS and network access from the gateway.

This integration targets the [modern project Responses API](https://learn.microsoft.com/en-us/azure/foundry/agents/quickstarts/responses-api).
It does not translate classic assistant IDs, threads/runs APIs, published Agent
Application URLs, or per-agent protocol URLs. An existing classic agent needs a
compatible modern agent before it can be registered here.

## 1. Add a connection

Open **Providers → Add Azure Foundry**. Enter a recognizable name, the complete
project endpoint, and an Azure identity method.

![Foundry connection using workload identity](assets/screenshots/foundry/connection.png)

*The screenshot uses fictional endpoint and identity values.*

| Azure identity | Required configuration | Deployment requirement |
| --- | --- | --- |
| **Workload identity (AKS)** | Tenant ID and client ID | Configure federated credentials, the pod service account and projected token. Set `AZURE_FEDERATED_TOKEN_FILE` to its mounted assertion file on every replica. |
| **Managed identity (Azure VM)** | Optional user-assigned client ID | Run on an Azure VM with its identity enabled and IMDS reachable. Blank client ID uses the system-assigned identity. App Service's separate identity endpoint is not supported. |
| **Client secret** | Tenant ID, client ID and client secret | Store and rotate the Entra application secret through the connection editor. |

The gateway requests scope `https://ai.azure.com/.default` (the equivalent resource
for VM IMDS). Azure tokens are cached only in memory and refreshed before expiry.
Each replica acquires its own token. Editing or disabling a connection affects
new requests through the shared database; identity changes invalidate its cached
token on the next lookup. An in-flight request can finish with its existing token.
Deployment environment changes, such as the federated token file mount, still
require a rollout.

Secrets are write-only in the admin API. On edit, leaving **Replace client secret**
blank preserves the saved secret. The provider's **configured** indicator describes
saved configuration; it is not a successful Azure connectivity or RBAC check.

## 2A. Register an existing agent

Open **Services → Register Foundry service**:

1. Enter a service name such as `research-agent`.
2. Select the saved Foundry connection.
3. Keep **Integration mode → Registered agent**.
4. Enter the agent's API name, for example `research-assistant`. This is not its
   display label or a classic `asst_...` identifier.
5. Optionally pin an agent version. Leaving it blank delegates version selection
   to Foundry; pinning is preferable when repeatability matters.
6. Choose a Relayna project if the service belongs to one. Choose caller
   authentication and save.

![Registered agent with a pinned version](assets/screenshots/foundry/registered-agent.png)

The gateway inserts this reference into the upstream request:

```json
{"agent_reference":{"type":"agent_reference","name":"research-assistant","version":"1"}}
```

Clients cannot override the agent, model, instructions or tools in this mode.
Allowed request fields are `input`, `stream`, `store:false`, `metadata`,
`max_output_tokens` and Relayna's `guardrails` selection. The registered agent's
model and tools remain managed in Azure. Relayna's request-level model/tool
controls do not inspect the agent's internal tool calls; govern those through its
Azure definition and service access. Existing Bing grounding configuration stays
in Azure. Returned citation annotations pass through unchanged.

## 2B. Expose the Foundry Responses endpoint

Use **Register Foundry service**, choose a connection, and change **Integration
mode → Foundry endpoint passthrough**. Agent name/version fields disappear because
the client supplies its request definition.

![Foundry endpoint passthrough configuration](assets/screenshots/foundry/passthrough.png)

A request may supply a model deployment, `instructions`, `tools`, or a supported
`agent_reference`. Give this mode only to callers trusted to use the resources
available to the provider identity. Use a separate connection/project when
applications require different Azure resource boundaries.

## 3. Configure caller access

![Saved Foundry services in the catalog](assets/screenshots/foundry/services.png)

Open the saved service's **Edit** action to configure permissions, authentication
profiles, request limits and pricing. Both integration modes require an active,
unexpired Relayna virtual key with access to the service. In key policy, allow
`internal-service`, `/services/*`, and the specific service name; a profile binding
alone does not grant these permissions.

- **Relayna key only:** send the assigned/permitted Relayna key. No Entra token is
  needed for this service's identity check.
- **Entra + Relayna key profile:** assign the key to an enabled profile and supply
  the required Entra audience/claims. Send the Entra JWT in `Authorization` and
  the Relayna key in the configured key header (default `X-Relayna-Key`).
- **Use gateway settings:** retains the gateway's existing identity behavior.

See [Authentication profiles](operations/authentication-profiles.md) for profile
creation, key selection, ownership restrictions and maintenance. A key belongs to
at most one profile per route; one profile can have many keys. Disabled profiles
and unassigned keys cannot invoke a service using explicit profiles.

Caller credentials are removed before forwarding. Azure receives a bearer token
for the configured gateway identity. There is **no on-behalf-of token exchange**;
tools requiring the original user's delegated Azure identity need a different
integration.

## Call a registered agent

For a key-only service:

```bash
curl "$RELAYNA_URL/services/research-agent/responses" \
  -H "Authorization: Bearer $RELAYNA_KEY" \
  -H 'Content-Type: application/json' \
  -d '{"input":"Summarize the latest developments and cite your sources."}'
```

For streaming, add `"stream":true` and use `curl -N`. Relayna forwards SSE chunks
as they arrive without waiting for the complete response. Existing configured
post-call guardrails can still require buffering; choose guardrails appropriate
for a streaming application.

For passthrough:

```bash
curl "$RELAYNA_URL/services/foundry-responses/responses" \
  -H "Authorization: Bearer $RELAYNA_KEY" \
  -H 'Content-Type: application/json' \
  -d '{"model":"YOUR_DEPLOYMENT","input":"Hello","stream":false}'
```

For an Entra profile, replace the authorization header with
`Authorization: Bearer $ENTRA_JWT` and add `X-Relayna-Key: $RELAYNA_KEY` (or the
configured header name).

## Conversation and API boundaries

This first integration is **stateless text invocation**. Relayna forces
`store:false` and rejects stored conversation IDs, previous response IDs,
background execution, item/file references and tool-result submissions. Send
history as inline user/assistant messages instead:

```json
{
  "input": [
    {"role":"user","content":"What is our topic?"},
    {"role":"assistant","content":"Azure Foundry integration."},
    {"role":"user","content":"Explain the next step."}
  ]
}
```

Only inline text is accepted. Image/audio/file inputs, persisted conversations,
response retrieval/cancellation, agent creation, file management and Azure
management APIs are outside this route. No query parameters are accepted. The
upstream API path is fixed, so a caller cannot replace it with another Azure URL.

Public Azure project hostnames are supported. Sovereign-cloud hostnames are not
currently accepted. HTTP loopback endpoints are accepted solely for local mock
validation. TLS is used for real Azure connections.

Foundry POST calls are not automatically retried or sent to fallback services:
replaying an agent could run its tools twice. Azure HTTP errors and `Retry-After`
are forwarded. Relayna records traffic and available token usage, but does not
invent a token count or model/tool cost when Azure omits it. Usage inspection is
bounded to 64 KiB for a JSON response or an individual SSE data line; larger
payloads continue forwarding but may have unavailable usage. For predictable
billing, configure service pricing; model tokens alone do not include every
Azure tool charge.

## Admin API examples

Create a connection with `POST /admin-ui/admin/providers`:

```json
{
  "provider":"azure-foundry",
  "name":"Research Foundry",
  "base_url":"https://research-demo.services.ai.azure.com/api/projects/research",
  "enabled":true,
  "foundry":{
    "method":"workload_identity",
    "tenant_id":"11111111-1111-4111-8111-111111111111",
    "client_id":"22222222-2222-4222-8222-222222222222"
  }
}
```

For client-secret authentication, use `method:"client_secret"` and include the
write-only top-level `credential`. For VM managed identity, use
`method:"managed_identity"`, `tenant_id:null` and an optional `client_id`.

Create a service with `POST /admin-ui/admin/services`:

```json
{
  "name":"research-agent",
  "route_pattern":"/services/research-agent/*",
  "allowed_methods":["POST"],
  "access":{"skip_entra":true},
  "foundry":{
    "mode":"registered_agent",
    "provider_id":"REPLACE_WITH_CONNECTION_UUID",
    "agent_name":"research-assistant",
    "agent_version":"1"
  }
}
```

For passthrough, replace `foundry` with
`{"mode":"endpoint_passthrough","provider_id":"REPLACE_WITH_CONNECTION_UUID"}`.
Do not combine Foundry bindings with a generic upstream URL, static service
credential, fallback service, Accessa channel or Studio service ID. Configure
Azure credentials on the provider instead. PATCH the same resources to change a
connection, agent/version or integration mode. Existing service authentication
and key assignments remain in place.

## Troubleshooting

| Symptom | Cause and corrective action |
| --- | --- |
| `invalid_foundry_configuration` | Check the complete project endpoint, identity IDs, required secret and provider UUID. Use POST only. Remove generic upstream/credential/fallback/Accessa/Studio fields. The referenced provider must exist and be Azure Foundry. A disabled provider must be enabled before invocation. |
| `invalid_foundry_request` | Use `/services/NAME/responses` without a query string. Send inline text. Remove state/background fields and, for registered agents, definition overrides such as `model`, `instructions` or `tools`. |
| `foundry_credential_unavailable` (502) | Check the client secret or workload assertion, Azure tenant/client IDs, federated credential configuration, VM IMDS access, and network connectivity. Token error bodies are intentionally not returned to callers. |
| Azure 401/403 | Check the gateway identity's Azure role/scope and the project's access configuration. Relayna profile claims and Azure RBAC are separate checks. |
| Azure 404 | Confirm the project endpoint, agent API name and pinned version exist in that project. |
| Azure 429 | Respect `Retry-After`; check model/tool capacity and Azure quotas. |
| Relayna denies a key | Check expiry/disabled state, project/service permissions, profile assignment and Enabled, then the profile's Entra requirements. |
| `provider_config_in_use` (409) on deletion | Reassign or delete referencing Foundry services first. Disable the provider to stop new invocations without removing configuration. |

## Upgrade and verification

The additive migration stores nullable Foundry configuration on providers and
services and adds a provider foreign key. Existing LiteLLM and generic services
retain their behavior. Upgrade all replicas before configuring Foundry; do not
route Foundry traffic to an older binary. Disable Foundry services before rolling
back, and retain the additive database columns.

The mock E2E test exercises real admin HTTP, PostgreSQL, Redis and Pingora against
local Azure-token and Foundry Responses mocks:

```bash
DATABASE_URL=postgres://postgres:postgres@127.0.0.1:15432/relayna_gateway \
REDIS_URL=redis://127.0.0.1:16379/0 \
SSL_CERT_FILE=/etc/ssl/cert.pem \
cargo test -p gateway-api --test foundry_e2e
```

The database user must be able to create/drop the isolated test database. The test
sets a loopback-only `GATEWAY_FOUNDRY_MOCK_TOKEN_ENDPOINT`; real Azure project
connections ignore that override. Do not configure this variable in production.
Mock tests verify gateway contracts and streaming, not a particular tenant's
Azure RBAC, network, agent deployment or Bing tool setup. Validate one real Azure
invocation in your environment before production use.
