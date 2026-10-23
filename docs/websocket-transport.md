# WebSocket transport

From v0.1.46, Relayna can proxy WebSockets alongside HTTP on the existing public listener.
This is opt-in for registered services and caller-owned LiteLLM passthrough.
Existing Accessa bindings retain their separate run-socket contract.

## Registered services

In **Discover → Services**, create or edit a service, include `GET` in allowed
methods, and expand **WebSocket transport**. Enable **Allow WebSocket alongside
HTTP** and save. Use an `http://` or `https://` upstream URL; the HTTP upgrade
establishes the socket. Existing wildcard prefix rewriting and query forwarding
apply, so `/services/events/socket?channel=a` becomes `/socket?channel=a` when
the service route is `/services/events/*`.

The same configuration is accepted by service create/patch APIs:

```json
{
  "access": {
    "websocket": {
      "idle_timeout_ms": 60000,
      "max_connections": 100,
      "max_connections_per_key": 10,
      "max_frame_bytes": 1048576,
      "max_session_duration_seconds": 3600
    }
  }
}
```

Omitting `websocket`, or setting it to `null`, keeps a service HTTP-only. An empty
object enables the displayed defaults. Service patches replace `access`, so
include existing identity settings when updating transport through the API.
Generic transport cannot be combined with Accessa or Azure Foundry bindings.

Clients authenticate with the route's existing Relayna key/Entra/profile contract.
Gateway strips client credentials, injects the registered service credential,
and preserves WebSocket subprotocol negotiation. Browser applications should use
a backend that supplies the required credentials; browser WebSocket APIs cannot
set arbitrary authentication headers. Do not put long-lived keys in URL queries.

Connection admission checks route, provider, service and streaming permissions,
allowed hours, request rate and budget eligibility. Enable streaming in the
key's policy. HTTP content guardrails that would apply cause socket admission to
fail closed, because this transport does not inspect message content.

Socket messages are opaque. The service owns per-message authorization, model
restrictions, operation limits and charging. Relayna records one metadata-only
usage event for a governed connection, without tokens or cost. HTTP service
pricing and body selectors do not price socket messages. Existing HTTP requests
retain their normal policy and billing behavior.

## Caller-owned LiteLLM passthrough

For `/v1/realtime` and other wildcard LiteLLM WebSocket endpoints:

1. In **Providers → LiteLLM passthrough**, enable forwarding, choose **Caller
   LiteLLM key** (`litellm_bearer`), allow `GET` and the required endpoint paths.
2. In **Discover → Routes**, edit identity for `/litellm/*`, expand **WebSocket
   transport**, and enable it. Preserve the existing identity contract: mandatory
   Gateway profiles/Entra requirements cannot be bypassed by caller mode.
3. Connect with `Authorization: Bearer <caller-owned LiteLLM key>` to
   `wss://gateway.example/v1/realtime?model=your-model`.

For `/v1/responses` (and `/responses`), set the canonical Responses route to
`direct_litellm_passthrough`, keep it enabled, and enable WebSocket transport on
its own route identity. These upgrades always use the canonical route's settings;
the wildcard cannot override a disabled or managed Responses route.

The route identity API is `PUT /admin-ui/admin/route-identities`:

```json
{
  "route": "/litellm/*",
  "access": { "websocket": {} }
}
```

Use `"route": "/v1/responses"` for canonical Responses settings. Include existing
identity fields when replacing `access`. LiteLLM endpoint availability depends on
the deployed LiteLLM version and configured upstream models.

Path denies, method allows and UI/admin exposure gates remain in force. Caller
keys use the configured LiteLLM credential header translation. From v0.1.48, WebSocket
upgrades also send `Authorization: Bearer <same caller-owned LiteLLM key>`.
This supports LiteLLM versions such as 1.100.1 whose initial WebSocket auth reads
Authorization before general auth enforces `litellm_key_header_name`. Keep the
provider's custom header and its raw/bearer value format configured as required
by LiteLLM. For example, a bearer-formatted `x-litellm-key` deployment receives:

```http
x-litellm-key: Bearer <caller-owned LiteLLM key>
Authorization: Bearer <same caller-owned LiteLLM key>
```

Both headers come from the selected caller key, replacing inbound credential
headers. Standard Authorization mode sends one Authorization value. HTTP
translation and registered-service socket credentials retain their existing
behavior. This compatibility header does not widen the key's model permissions.
Gateway never retries using a stored service/provider key. Relayna virtual keys, trusted-ingress
stored-key access and Relayna-governed LiteLLM WebSocket sessions are unsupported
in this feature; those requests are rejected before opening an upstream socket.

LiteLLM owns session/model policy, usage and spend. Relayna records correlated
Traffic diagnostics, with no invented Relayna key identity or per-key usage row.
The per-key connection cap uses a process-local credential digest, never a stored
raw caller key.

## Limits, diagnostics and deployment

Only HTTP/1.1 `GET` upgrades with WebSocket version 13 and a valid handshake key
are accepted. Text, binary, fragmented messages, ping/pong and close frames stream
bidirectionally without whole-message buffering. Compression offers are declined
by removing the extension offer upstream. Unsolicited compressed upgrades fail.

Both frame size and complete fragmented-message size are bounded. Connection
limits apply per route and key per process, within the shared 4,096 connection
ceiling, including Accessa. Limits are not distributed across replicas. Failed
handshakes and closed/error connections release their slots. Sessions are never
automatically retried or failed over after an upgrade.

Idle read/write timeouts apply in each direction. Session age is checked on
traffic; verified identity/key expiry and service stream-duration policy can
shorten it. Idle connections also expire through read/write timeouts; expiry does
not schedule a separate wall-clock close or continuously recheck key revocation.
Reconnect to reauthenticate. The upstream service owns reconnect/resume semantics.

Traffic shows handshake status, duration, directional bytes/frame counts, close
observations and failure diagnostics. Frame payloads and credentials are excluded.
Upstream handshake failures retain their status and response body.

Configure ingress/load balancers to forward WebSocket upgrades and use suitable
connection idle timeouts. Upgrade every gateway replica before enabling the new
`access.websocket` field. It is optional JSON in existing persisted access
settings; no SQL migration is required. Before rolling back to older binaries,
drain affected sockets and remove this field entirely from configured routes/services
using the new binary. Confirm that saved JSON omits the field entirely: older
strict readers reject `"websocket": null` too. Saving settings affects new
connections; it does not cancel existing sockets.


## Verified LiteLLM interoperability

On 2026-10-21, custom-header interoperability passed 16 checks each against
real LiteLLM 1.100.1 (PyPI) and the pinned 1.106.0 image. Both required
`x-litellm-key` and used real Sol-only LiteLLM virtual keys with a deterministic
local HTTPS/WSS provider. Responses sockets exchanged delta/completed events,
including two turns, aliases and model selection in the first frame. Luna stayed
denied in both query and first-frame checks without contacting the provider.
HTTP controls, spoofed alternate credentials and metadata-only Traffic accounting
also passed. See the [sanitized report](https://github.com/sarattha/relayna-gateway/blob/8903a62191d461f12c53d10108b1c880452945eb/internal/test-reports/litellm-websocket-custom-header/README.md).

On 2026-10-20, the official `docker.litellm.ai/berriai/litellm:latest` image
contained LiteLLM v1.106.0. Seventeen local checks passed against the actual
LiteLLM application: HTTP coexistence, Realtime, native and LiteLLM-managed
Responses sockets, multi-turn streaming, aliases, model/key restrictions and
gateway route denials. Provider responses came from deterministic HTTPS/WSS
fixtures; external provider inference was not part of this test.

LiteLLM's managed Responses mode bridges provider HTTP streaming to WebSocket
messages. It remains caller-owned from Relayna's perspective and is separate
from the deferred Relayna-governed session feature. Native provider fixtures
used HTTPS/WSS, because v1.106.0 rejected a plain `ws://` Responses backend with
an SSL configuration error. Verify the provider URL and protocol supported by
your deployed LiteLLM version.
