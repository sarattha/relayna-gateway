# Entra integration requirements

This is the production handoff for Relayna Gateway `v0.1.26` and later. It
follows Arcweft's single-application Entra pattern: one registration is both the
confidential Web client and API resource, while separate managed identities
receive least-privilege application roles. Certificate-backed OIDC uses PS256
`private_key_jwt`, `x5t#S256`, the exact token-endpoint audience, five-minute
assertions, and overlap certificate rotation.

## Entra application contract

Create one Entra application registration in one tenant:

| Object | Purpose | Required configuration |
| --- | --- | --- |
| Relayna Gateway application | Human `/admin-ui` sign-in plus request-plane and `/owner/v1` API access | Confidential Web platform; exposed API with identifier URI `api://<application-id>`; v2 access tokens; exact redirect and logout URIs; public signing certificate; ID tokens enabled; `gateway.invoke` and `gateway.monitor.read` application roles allowed for Applications; no client secret or delegated product scopes. |

The application ID GUID is the portal OAuth client ID and the exact expected
`aud` of Entra v2 workload access tokens. The related identifier URI is the
resource managed identities use to request
`api://<application-id>/.default`. Do not create a second API registration or
configure the identifier URI itself as Relayna's expected v2 token audience.

Human portal sign-in does not require an Entra application role. Human Admin,
Owner, and Viewer authorization remains stored and enforced by Relayna after
Entra authenticates the user.

## Entra team checklist

Complete Entra changes plan-first against the exact tenant and application.
The change record must identify the environment, tenant ID, application ID,
redirect and logout URIs, public-certificate fingerprint, and affected managed
identities before anything is applied. Never include private keys, access
tokens, client assertions, or Kubernetes Secret values in the plan or evidence.

- [ ] Confirm the target tenant ID, shared Relayna Gateway application ID, and
      production host with DevOps.
- [ ] Confirm that one existing or new application registration is used as
      both the confidential Web client and API resource; do not create a second
      API registration.
- [ ] Configure the exact Web redirect URI
      `https://<gateway-host>/admin-ui/auth/callback` and logout return
      `https://<gateway-host>/admin-ui`.
- [ ] Enable ID tokens, set the identifier URI to
      `api://<application-id>`, and request v2 access tokens.
- [ ] Confirm that the application defines no delegated product scopes and has
      no client secret.
- [ ] Register only the approved public certificate and record its SHA-256
      fingerprint and expiry. Keep the previous public certificate registered
      during an overlap rotation.
- [ ] Define `gateway.invoke` and `gateway.monitor.read` as application roles
      allowed for `Applications`; do not define Relayna's `Admin`, `Owner`, or
      `Viewer` roles in Entra.
- [ ] After DevOps supplies each managed identity's immutable service-principal
      object ID, assign only the required application role or roles.
- [ ] Verify that the shared application's service principal and every role
      assignment are in the intended tenant. Allow for managed-identity token
      caching, then validate with a newly acquired token.
- [ ] Hand DevOps the tenant ID, application ID, identifier URI, issuer,
      discovery URL, role values, certificate fingerprint and expiry, and
      role-assignment evidence. Do not hand off private key material.

Entra sign-off is complete when the application configuration and assignments
match the handoff values, a fresh `gateway.invoke` token and a fresh
`gateway.monitor.read` token have the application ID GUID as `aud`, and each
token contains only the role assigned to that identity.

## DevOps team checklist

DevOps owns the environment-specific runtime, managed identities, federation,
plain Kubernetes ConfigMap and Secret objects, network boundary, deployment,
and rollback evidence. Entra application and role mutations remain with the
Entra team. This handoff does not use Helm values or templates.

- [ ] Give the Entra team the exact environment, tenant ID, production host,
      callback/logout URIs, and approved public certificate. Never send the
      private key.
- [ ] Create one user-assigned managed identity per approved environment,
      workload, team, and capability boundary. Do not share identities merely
      to reduce their count.
- [ ] Configure AKS workload-identity federation for each exact Kubernetes
      service account and provide the Entra team with the identity's client ID,
      immutable service-principal object ID, display name, and required app
      role.
- [ ] Obtain the Entra handoff values and role-assignment evidence before
      creating or updating the Gateway's Kubernetes objects.
- [ ] Create or update the normal `v1/ConfigMap` named
      `relayna-gateway-config` with the values in [Kubernetes ConfigMap and
      Secrets](#kubernetes-configmap-and-secrets). Use the one handed-off
      `ENTRA_APPLICATION_ID` for portal, request-plane, and monitoring modes;
      do not put credentials or private key material in the ConfigMap.
- [ ] Create or update the normal `v1/Secret` named
      `relayna-gateway-secrets` for database, Redis, LiteLLM, Entra verifier,
      and other application secrets.
- [ ] Create or update the normal `v1/Secret` named
      `relayna-gateway-portal-oidc` with the matching
      `portal-private-key.pem` and `portal-certificate.pem` files. Keep the
      previous Secret version recoverable for the rollback window; do not
      commit Secret values to Git or package them as Helm values.
- [ ] Apply the ConfigMap, Secrets, and raw Deployment independently with the
      environment's normal `kubectl` or raw-manifest workflow. Confirm the
      Deployment uses `configMapRef`, `secretRef`, and the read-only portal
      certificate Secret volume shown in
      `deploy/kubernetes/relayna-gateway.yaml`.
- [ ] Configure initial administrator email and immutable object-ID allowlists
      together. Require every intended bootstrap administrator to sign in and
      verify each persisted Admin role before clearing both bootstrap values
      and rolling the Deployment.
- [ ] Ask a Relayna administrator to create each required service or project
      managed-identity binding. Record the exact Relayna resource name and
      required `gateway.monitor.read` role; an Entra assignment alone grants no
      monitoring access.
- [ ] Keep `/admin-ui` and `/owner/v1` behind the approved internal ingress and
      ensure its namespace has `relayna.io/control-plane-access=true`.
- [ ] Run the raw-manifest dry run and
      `scripts/entra/verify-deployment.sh`, then execute every check in
      [Network and verification contract](#network-and-verification-contract).
- [ ] Record the deployed image digest, ConfigMap revision, non-secret Secret
      version identifiers, certificate fingerprint, Deployment revision, and
      rollback owner. Do not capture Secret contents or tokens.
- [ ] During certificate rotation, require Entra to register the new public
      certificate first; then roll the matching Secret, verify the deployment,
      and remove the old certificate only after the observation window.

DevOps sign-off is complete when the verifier passes, browser sign-in and
logout pass, first-admin bootstrap has been removed, an allowed monitoring
resource returns HTTP 200, an unbound resource is denied, request-plane policy
still requires a Relayna virtual key, and the previous deployment and Secret
versions remain recoverable.

Create one user-assigned managed identity per approved workload security
boundary. A deployment that uses both service-to-service surfaces should start
with two identities:

| Managed identity boundary | App role | Permitted entrance | Additional Relayna authorization |
| --- | --- | --- | --- |
| Relayna runtime or other governed provider caller | `gateway.invoke` | `/v1/*`, `/providers/*`, and `/services/*` when Entra front-door auth is enabled | Valid Relayna virtual key plus route, model, budget, rate-limit, and guardrail policy. |
| Owner-monitoring workload | `gateway.monitor.read` | `/owner/v1/services/{service_name}/*` and `/owner/v1/projects/{project_id}/*` | Enabled exact managed-identity binding for the requested service or project. |

The Gateway portal itself requires zero managed identities. Do not share an
identity across unrelated teams, environments, services, or capability
boundaries merely to reduce the count. Assigning both roles to one identity is
allowed only when the combined blast radius is an explicit least-privilege
decision. Create one Relayna binding per monitored service.

For every managed identity, the teams must:

1. The Entra team assigns only the shared application's role or roles required
   by that managed identity service principal.
2. DevOps configures AKS workload-identity federation for the intended
   Kubernetes service account outside this repository.
3. DevOps gives the Relayna administrator the tenant ID, managed-identity
   client ID, immutable service-principal object ID, display name, and exact
   Relayna service or project name.
4. The Relayna administrator creates an enabled managed-identity binding in
   **Members** for each allowed service or project, with required role
   `gateway.monitor.read`.

Both authorization layers are mandatory. Entra app-role assignment permits a
coarse API capability. Relayna's virtual-key policy governs provider calls, and
its exact binding limits which service a monitoring identity can read.

## Application role inventory

| App registration | Role value | Allowed member type | Assigned to | Relayna effect |
| --- | --- | --- | --- | --- |
| Relayna Gateway | `gateway.invoke` | Applications | Managed identities that may enter governed request-plane routes | Request must also carry a valid Relayna virtual key and pass its policy. |
| Relayna Gateway | `gateway.monitor.read` | Applications | Managed identities that may call `/owner/v1` | Token must also match an enabled exact service or project binding. |

`Admin`, `Owner`, and `Viewer` remain Relayna roles, not Entra application
roles. Do not expose them on the shared registration.

## Shared application settings

Use these exact relationships, replacing placeholders with the production host
and tenant:

| Setting | Value |
| --- | --- |
| Application ID | One tenant-assigned GUID used by all Relayna Entra modes |
| Application ID URI | `api://<application-id>` |
| Requested access-token version | `2` |
| Client type | Confidential Web application |
| Redirect URI | `https://<gateway-host>/admin-ui/auth/callback` |
| Front-channel/logout return | `https://<gateway-host>/admin-ui` |
| Issuer | `https://login.microsoftonline.com/<tenant-id>/v2.0` |
| Discovery | `https://login.microsoftonline.com/<tenant-id>/v2.0/.well-known/openid-configuration` |
| Portal ID-token audience | Application ID GUID |
| Workload access-token audience | Application ID GUID |
| Workload token resource/scope | `api://<application-id>/.default` |
| Client authentication | `private_key_jwt`; PS256; no client secret |
| Requested scopes | `openid profile email` |
| Delegated product scopes | None |
| Application roles | `gateway.invoke`, `gateway.monitor.read`; both allow `Applications` |

Register the public certificate under the shared application's certificates.
Do not upload, paste, or log the private key. The token endpoint discovered from
the configured metadata is the assertion audience; do not hard-code a different
tenant or endpoint.

## Workload token settings

| Setting | Value |
| --- | --- |
| Expected `aud` | Shared application ID GUID |
| Requested resource | `api://<application-id>/.default` |
| Issuer | `https://login.microsoftonline.com/<tenant-id>/v2.0` |
| Discovery | `https://login.microsoftonline.com/<tenant-id>/v2.0/.well-known/openid-configuration` |
| Accepted signing algorithm | `RS256` |
| Required application role | `gateway.invoke` for request-plane traffic; `gateway.monitor.read` for owner monitoring |

Managed-identity tokens must carry the tenant, shared application ID audience,
and assigned role. Relayna additionally matches monitoring client ID and, when
supplied, immutable object ID against the enabled service binding.

## Kubernetes ConfigMap and Secrets

Use ordinary Kubernetes `v1/ConfigMap` and `v1/Secret` objects, applied through
the environment's normal `kubectl` or raw-manifest workflow. Do not move these
values into a Helm chart. The checked-in
`deploy/kubernetes/relayna-gateway.yaml` shows the required object names and
Deployment references; DevOps replaces every placeholder before deployment.

Set these portal values in `relayna-gateway-config`:

```text
PORTAL_OIDC_ENABLED=true
ENTRA_APPLICATION_ID=<single-application-id-guid>
PORTAL_OIDC_TENANT_ID=<tenant-id>
PORTAL_OIDC_PRIVATE_KEY_PATH=/var/run/secrets/relayna-portal-oidc/portal-private-key.pem
PORTAL_OIDC_CERTIFICATE_PATH=/var/run/secrets/relayna-portal-oidc/portal-certificate.pem
PORTAL_OIDC_ISSUER=https://login.microsoftonline.com/<tenant-id>/v2.0
PORTAL_OIDC_DISCOVERY_URL=https://login.microsoftonline.com/<tenant-id>/v2.0/.well-known/openid-configuration
PORTAL_OIDC_REDIRECT_URI=https://<gateway-host>/admin-ui/auth/callback
PORTAL_OIDC_POST_LOGOUT_REDIRECT_URI=https://<gateway-host>/admin-ui
PORTAL_ADMIN_EMAILS=<comma-separated initial admin emails>
PORTAL_ADMIN_OBJECT_IDS=<comma-separated immutable Entra user object IDs>
PORTAL_SESSION_COOKIE_SECURE=true
ENTRA_AUTH_DEBUG=false
```

Keep `ENTRA_AUTH_DEBUG=false` during normal operation. During a controlled
incident, it can be set to `true` temporarily to record every Entra validation,
portal login, and cookie-session decision. The logs may contain decoded claims;
follow the retention and access controls in the
[debug-mode runbook](entra-authorization-debug.md).

Every initial admin must be represented in both allowlists. Gateway requires a
verified tenant, object ID, and email match. Require every intended bootstrap
administrator to sign in and verify each persisted Admin role before clearing
both bootstrap values and rolling the Deployment.

For provider/request-plane authorization, set `ENTRA_AUTH_ENABLED=true`,
`ENTRA_TENANT_ID`, issuer/discovery values, and
`ENTRA_REQUIRED_ROLE=gateway.invoke`. For monitoring, set
`OWNER_ENTRA_AUTH_ENABLED=true`, `OWNER_ENTRA_TENANT_ID`, and its
issuer/discovery values. Both verifiers use `ENTRA_APPLICATION_ID`; do not set
the removed `ENTRA_AUDIENCE`, `PORTAL_OIDC_CLIENT_ID`, or
`OWNER_ENTRA_AUDIENCE` variables.

Store the matching RSA private key and public certificate in
`relayna-gateway-portal-oidc` under
`portal-private-key.pem` and `portal-certificate.pem`. The raw Deployment mounts
that Secret read-only; `relayna-gateway-secrets` continues to hold database,
Redis, LiteLLM, and other application secrets.

## Upgrade from v0.1.25

Before deploying `v0.1.26`, merge the former portal Web and owner API
configuration onto one Entra registration. Add the Web redirect/logout URIs,
public certificate, identifier URI, v2 token setting, and both application roles
to that registration. Reassign managed identities to its service principal.
Then set `ENTRA_APPLICATION_ID` to the shared application ID GUID and remove:

- `ENTRA_AUDIENCE`
- `PORTAL_OIDC_CLIENT_ID`
- `OWNER_ENTRA_AUDIENCE`

Existing browser sessions may be invalid after the application-ID cutover; ask
users to sign in again. Existing Relayna members, service memberships, managed
identity service bindings, virtual keys, and usage data do not require data
rewrites. Release `0.1.31` retains the separate project membership and project
managed-identity binding tables.

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

Entra tenant/application provisioning and app-role assignment automation remain
Entra-team responsibilities. Managed-identity creation, AKS workload-identity
automation, and the plain Kubernetes ConfigMap, Secrets, and Deployment remain
DevOps responsibilities. Helm is not part of this handoff. All environment
values and identity mutations remain outside this repository.
