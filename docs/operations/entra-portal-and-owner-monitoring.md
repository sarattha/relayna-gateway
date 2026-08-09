# Entra portal and service-owner monitoring

Relayna Gateway serves human browser views from `/admin-ui`. Microsoft Entra ID
proves human identity through a confidential OIDC BFF flow; the browser receives
an opaque HttpOnly session cookie, not an Entra token. Relayna's persisted
memberships decide whether the identity is pending, active, blocked, an
administrator, an Owner, or a Viewer.

Release `0.1.26` authenticates the confidential client with a certificate-signed
PS256 `private_key_jwt`. It does not accept a portal client secret. The raw
ConfigMap and Secrets in `deploy/kubernetes/relayna-gateway.yaml` are the
deployment contract; Helm rendering, tenant provisioning, managed-identity
creation, and AKS workload-identity federation remain DevOps-owned.

See [Entra Integration Requirements](entra-integration-requirements.md) for the
complete application, role, managed-identity, certificate, and issuer handoff.

## Routes

- Browser portal: `/admin-ui`
- BFF protocol: `/admin-ui/auth/*`
- Administrator APIs: `/admin-ui/admin/*`
- Service-owner API: `/owner/v1/services/{service_name}/*`
- Gateway request plane: `/v1/*` and `/services/*`

The private control Ingress must route both `/admin-ui` and `/owner/v1` to the
control Service on port 8081. Owner endpoints include service details,
dashboard aggregates, sanitized usage events and request logs, failures,
endpoint breakdowns, and exports. The server overwrites the usage query's
service filter with the service named in the route.

## Human OIDC configuration

Put non-secret values in `relayna-gateway-config`:

```text
PORTAL_OIDC_ENABLED=true
ENTRA_APPLICATION_ID=<single confidential Web/API application ID>
PORTAL_OIDC_TENANT_ID=<tenant UUID>
PORTAL_OIDC_PRIVATE_KEY_PATH=/var/run/secrets/relayna-portal-oidc/portal-private-key.pem
PORTAL_OIDC_CERTIFICATE_PATH=/var/run/secrets/relayna-portal-oidc/portal-certificate.pem
PORTAL_OIDC_ISSUER=https://login.microsoftonline.com/<tenant>/v2.0
PORTAL_OIDC_DISCOVERY_URL=https://login.microsoftonline.com/<tenant>/v2.0/.well-known/openid-configuration
PORTAL_OIDC_REDIRECT_URI=https://gateway.example/admin-ui/auth/callback
PORTAL_OIDC_POST_LOGOUT_REDIRECT_URI=https://gateway.example/admin-ui
PORTAL_ADMIN_EMAILS=admin@example.com
PORTAL_ADMIN_OBJECT_IDS=<immutable Entra user object ID>
PORTAL_SESSION_TTL_SECONDS=28800
PORTAL_LOGIN_TTL_SECONDS=600
PORTAL_SESSION_COOKIE_SECURE=true
```

`PORTAL_ADMIN_EMAILS` implements the requested first-deployment ConfigMap
bootstrap pattern. For safety, `PORTAL_ADMIN_OBJECT_IDS` must be configured with
it. Gateway grants the initial Admin role only when the verified token's tenant,
immutable `oid`, and normalized email all appear in the configured allowlists.
If one list is populated and the other is empty, startup fails. After each
intended administrator has signed in and the persisted active Admin role is
verified, remove both settings and roll the Deployment. Existing persisted
roles remain active.

The callback validates single-use state, nonce, PKCE S256, signature, issuer,
tenant, audience, not-before, and expiry. A short-lived HttpOnly login cookie
binds the transaction to the initiating browser. Cookie-authenticated Admin
mutations require the session-bound `x-csrf-token`. Sign-out revokes the Relayna
session and then uses Entra's discovered end-session endpoint.

## Certificate Secret and lifecycle

DevOps must create `relayna-gateway-portal-oidc` with these exact keys:

```text
portal-private-key.pem       PKCS#8 or PKCS#1 RSA private key
portal-certificate.pem       matching X.509 public certificate
```

The Deployment mounts the Secret read-only at
`/var/run/secrets/relayna-portal-oidc`. Register only the public certificate on
the shared Relayna Gateway application. Gateway parses the certificate, verifies that its
RSA public key matches the private key, computes `x5t#S256`, and fails startup
on missing, invalid, or mismatched material. Client assertions use PS256, the
client ID as issuer and subject, the exact discovered token endpoint as
audience, a unique JTI, and a five-minute lifetime.

Rotate without downtime:

1. Generate a new RSA key and certificate in the approved key-management path.
2. Add the new public certificate to the shared Entra application while the old
   certificate remains registered.
3. Update both Secret keys as one version and roll the Gateway Deployment.
4. Run `scripts/entra/verify-deployment.sh`, sign in, sign out, and sign in
   again through `/admin-ui`.
5. Keep the old Entra certificate and recoverable previous Secret version for
   the rollback window, then remove them after the new rollout is stable.

Rollback by restoring both files from the previous Secret version and rolling
the Deployment while the previous public certificate is still registered in
Entra. Never mix a private key from one version with a certificate from another.

## Workload monitoring configuration

```text
OWNER_ENTRA_AUTH_ENABLED=true
ENTRA_APPLICATION_ID=<same confidential Web/API application ID>
OWNER_ENTRA_TENANT_ID=<tenant UUID>
OWNER_ENTRA_ISSUER=https://login.microsoftonline.com/<tenant>/v2.0
OWNER_ENTRA_OIDC_DISCOVERY_URL=https://login.microsoftonline.com/<tenant>/v2.0/.well-known/openid-configuration
OWNER_ENTRA_ACCEPTED_ALGORITHMS=RS256
```

Configure the one shared Entra application with identifier URI
`api://<ENTRA_APPLICATION_ID>` and requested access-token version 2. Managed
identities request `api://<ENTRA_APPLICATION_ID>/.default`; Gateway validates
the resulting token's `aud` as the application ID GUID. Register each monitoring
identity in Relayna with its tenant ID, client ID, immutable object ID, exact
service, and `gateway.monitor.read` application role. A tenant-wide token or
Entra app-role assignment without an enabled exact Relayna binding cannot read
any service.

Use a separate request-plane managed identity with only `gateway.invoke` when a
Relayna worker or another service calls provider routes. Those calls still need
a Relayna virtual key and must pass route, model, budget, rate-limit, and
guardrail policy. Do not assign both application roles to one identity unless
the combined capability boundary is explicitly intended.

## Incident monitoring and request details

The service dashboard supports 6-hour, 24-hour, and 7-day windows. Its incident
chart plots error rate and P95 latency and keeps the matching usage table as a
textual fallback. If a registered upstream returns
`X-Relayna-Service-Version`, Gateway records it only when it matches
`[A-Za-z0-9][A-Za-z0-9._+-]{0,63}`. Chart markers mean the first time Gateway
observed a transition; they do not claim to be the deployment timestamp.
Repeated events for one version do not create duplicate markers, while later
rollbacks create a new transition marker.

Request logs can show all outcomes or only Success or Failure, filter an exact
HTTP status code, and page through results. **View details** calls:

```text
GET /owner/v1/services/{service_name}/requests/{request_id}
```

The route requires exact service membership. Missing request IDs and IDs that
belong to another service return the same `request_not_found` 404 response. The
response contains sanitized usage metadata plus an optional redacted debug
bundle. Historical and demo rows remain inspectable when the bundle is absent.
Request bodies, prompts, credentials, raw headers, and unredacted provider
errors are never included.

## Raw Kubernetes verification

This verification intentionally targets the current raw manifest, not a Helm
layout:

```bash
kubectl apply --dry-run=client -f deploy/kubernetes/relayna-gateway.yaml
scripts/entra/verify-deployment.sh \
  --namespace <gateway-namespace> \
  --control-ingress-namespace <internal-ingress-namespace> \
  --certificate-file <approved-public-certificate.pem>
```

The verifier is read-only. It checks ConfigMap settings, first-admin bootstrap
consistency, Secret mounts, certificate validity and key matching, seven-day
expiry headroom, `/admin-ui` and `/owner/v1` routing, NetworkPolicy admission,
and Deployment availability.

## Local development issuer

The local issuer is adapted from Arcweft's certificate-authenticated fixture
and refuses production environments. Generate development-only material, then
start it:

```bash
scripts/entra/generate-development-portal-certificate.sh
RELAYNA_DEV_OIDC_BROWSER_CERTIFICATE_PATH=target/development-oidc/portal-certificate.pem \
  node scripts/entra/development-oidc.mjs
```

Set Gateway's `PORTAL_OIDC_PRIVATE_KEY_PATH` and
`PORTAL_OIDC_CERTIFICATE_PATH` to the generated files, and use issuer/discovery
URLs rooted at `http://127.0.0.1:18090`. Use shared application ID
`relayna-gateway-local`, identifier URI `api://relayna-gateway-local`, redirect URI
`http://127.0.0.1:18381/admin-ui/auth/callback`, logout return
`http://127.0.0.1:18381/admin-ui`, and `PORTAL_SESSION_COOKIE_SECURE=false`.
Use the generated private-key and certificate paths instead of a client secret.
The account chooser provides pending, administrator, and service-owner-shaped
identities. The development administrator object ID is
`00000000-0000-0000-0000-000000000002`. The invoke fixture uses client/object
IDs ending in `0101`/`0102`, and the monitoring fixture uses IDs ending in
`0201`/`0202`; each has a separate development-only secret and only its named
application role. Never deploy the fixture, its keys, or its credentials.
