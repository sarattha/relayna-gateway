# Entra integration requirements

This is the production handoff for Relayna Gateway `v0.1.25` and later. It uses
the local Arcweft implementation as the reference for certificate-backed OIDC:
PS256 `private_key_jwt`, `x5t#S256`, exact token-endpoint audience, five-minute
assertions, and overlap certificate rotation.

## DevOps-owned objects

Create two Entra application registrations in one tenant:

| Object | Purpose | Required configuration |
| --- | --- | --- |
| Relayna Gateway portal Web application | Human `/admin-ui` sign-in | Confidential Web client; exact redirect and logout URIs; public signing certificate; ID tokens enabled; no client secret. |
| Relayna Gateway owner API | Workload `/owner/v1` monitoring | Application ID URI/audience such as `api://relayna-gateway-owner`; application role `gateway.monitor.read` allowed for Applications. |

The portal application does not need an Entra application role for Relayna
administration. Human Admin, Owner, and Viewer authorization is stored and
enforced by Relayna after Entra authenticates the user.

Create one user-assigned managed identity per approved workload security
boundary. The minimum is therefore:

```text
managed identity count = number of independently deployed workloads that call /owner/v1
```

The Gateway portal itself requires zero managed identities. The verified local
`orders` example needs one. Do not share an identity across unrelated teams,
environments, or services merely to reduce the count. A single workload may use
one identity for several services only when that shared blast radius is an
explicit least-privilege decision; create one Relayna binding per service.

For every managed identity, DevOps must:

1. Assign the owner API's `gateway.monitor.read` application role to the
   managed identity service principal.
2. Configure AKS workload-identity federation for the intended Kubernetes
   service account outside this repository.
3. Give the Relayna administrator the tenant ID, managed-identity client ID,
   immutable service-principal object ID, display name, and exact Relayna
   service name.
4. Create an enabled managed-identity binding in **Members** for each allowed
   service, with required role `gateway.monitor.read`.

Both authorization layers are mandatory: Entra app-role assignment permits the
API role, while Relayna's exact binding limits which service can be read.

## Application role inventory

| App registration | Role value | Allowed member type | Assigned to | Relayna effect |
| --- | --- | --- | --- | --- |
| Owner API | `gateway.monitor.read` | Applications | Every managed identity that may call `/owner/v1` | Token must also match an enabled exact service binding. |

No other Entra application role is required by the `v0.1.25` portal/owner
integration. `Admin`, `Owner`, and `Viewer` are Relayna roles, not Entra app
roles.

## Portal application settings

Use these exact relationships, replacing placeholders with the production host
and tenant:

| Setting | Value |
| --- | --- |
| Client type | Confidential Web application |
| Redirect URI | `https://<gateway-host>/admin-ui/auth/callback` |
| Front-channel/logout return | `https://<gateway-host>/admin-ui` |
| Issuer | `https://login.microsoftonline.com/<tenant-id>/v2.0` |
| Discovery | `https://login.microsoftonline.com/<tenant-id>/v2.0/.well-known/openid-configuration` |
| Portal audience | Portal application's client ID |
| Client authentication | `private_key_jwt`; PS256; no client secret |
| Requested scopes | `openid profile email` |

Register the public certificate under the portal application's certificates.
Do not upload, paste, or log the private key. The token endpoint discovered from
the configured metadata is the assertion audience; do not hard-code a different
tenant or endpoint.

## Owner API settings

| Setting | Value |
| --- | --- |
| Audience/Application ID URI | `api://relayna-gateway-owner` or the approved equivalent |
| Issuer | `https://login.microsoftonline.com/<tenant-id>/v2.0` |
| Discovery | `https://login.microsoftonline.com/<tenant-id>/v2.0/.well-known/openid-configuration` |
| Accepted signing algorithm | `RS256` |
| Required application role | `gateway.monitor.read` |

Managed-identity tokens must carry the tenant, owner API audience, and role.
Relayna additionally matches client ID and, when supplied, immutable object ID
against the enabled service binding.

## Kubernetes ConfigMap and Secrets

Set these portal values in `relayna-gateway-config`:

```text
PORTAL_OIDC_ENABLED=true
PORTAL_OIDC_TENANT_ID=<tenant-id>
PORTAL_OIDC_CLIENT_ID=<portal-client-id>
PORTAL_OIDC_PRIVATE_KEY_PATH=/var/run/secrets/relayna-portal-oidc/portal-private-key.pem
PORTAL_OIDC_CERTIFICATE_PATH=/var/run/secrets/relayna-portal-oidc/portal-certificate.pem
PORTAL_OIDC_ISSUER=https://login.microsoftonline.com/<tenant-id>/v2.0
PORTAL_OIDC_DISCOVERY_URL=https://login.microsoftonline.com/<tenant-id>/v2.0/.well-known/openid-configuration
PORTAL_OIDC_REDIRECT_URI=https://<gateway-host>/admin-ui/auth/callback
PORTAL_OIDC_POST_LOGOUT_REDIRECT_URI=https://<gateway-host>/admin-ui
PORTAL_ADMIN_EMAILS=<comma-separated initial admin emails>
PORTAL_ADMIN_OBJECT_IDS=<comma-separated immutable Entra user object IDs>
PORTAL_SESSION_COOKIE_SECURE=true
```

Every initial admin must be represented in both allowlists. Gateway requires a
verified tenant, object ID, and email match. After sign-in and persisted Admin
role verification, clear both bootstrap values and roll the Deployment.

Set `OWNER_ENTRA_*` from the owner API table above. Store the matching RSA
private key and public certificate in `relayna-gateway-portal-oidc` under
`portal-private-key.pem` and `portal-certificate.pem`. The raw Deployment mounts
that Secret read-only; `relayna-gateway-secrets` continues to hold database,
Redis, LiteLLM, and other application secrets.

## Certificate standard and lifecycle

Use an organization-approved RSA certificate; RSA 3072 with SHA-256 and a
validity period compatible with the certificate policy is the recommended
baseline. Track owner, issuance time, expiry, Entra registration, Secret
version, and emergency rollback version. Alert well before expiry; the supplied
verifier treats less than seven days of validity as a failure.

Rotation order is public certificate first, then Secret rollout, verification,
and finally old-certificate removal. Keep old and new public certificates
registered during the rollout. Keep the previous Secret version recoverable
for the rollback window. Gateway fails startup when the certificate is invalid
or its RSA public key does not match the private key.

## Network and verification contract

The internal Ingress routes `/admin-ui` and `/owner/v1` to
`relayna-gateway-control:8081`. Its namespace must carry
`relayna.io/control-plane-access=true` so the checked-in NetworkPolicy admits
it. Keep the host behind internal ingress, VPN, IAP, and approved source ranges.

Before production sign-off:

1. Run the raw-manifest dry run and `scripts/entra/verify-deployment.sh`.
2. Verify first-admin sign-in, persistence after bootstrap values are cleared,
   pending and blocked denial, and logout/account switching.
3. Verify an Owner sees only assigned services.
4. Obtain a managed-identity token and prove its allowed `/owner/v1` service is
   HTTP 200 and another service is denied.
5. Verify certificate fingerprint, expiry, Deployment availability, and both
   private Ingress paths.

Helm templates, tenant provisioning, managed-identity creation, app-role
assignment automation, and AKS workload-identity automation are intentionally
outside this repository change and remain DevOps responsibilities.
