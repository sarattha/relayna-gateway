# Entra portal and service-owner monitoring

Relayna Gateway serves all human browser views from `/admin-ui`. Microsoft
Entra ID proves human identity through a confidential OIDC BFF flow; the browser
receives an opaque HttpOnly session cookie, not an Entra token. Relayna's
`portal_members` and `service_memberships` records decide whether that identity
is pending, active, blocked, an administrator, an Owner, or a Viewer.

Existing operator tokens remain available as break-glass credentials. Use one
to approve the first Entra administrator from **Members**, then use Entra for
normal administration.

## Routes

- Browser portal: `/admin-ui`
- BFF protocol: `/admin-ui/auth/*`
- Existing administrator APIs: `/admin-ui/admin/*`
- Service-owner API: `/owner/v1/services/{service_name}/*`
- Gateway request plane: `/v1/*` and `/services/*`

Owner endpoints include service details, dashboard aggregates, sanitized usage
events/request logs, failures, and endpoint breakdowns. The server always
overwrites the usage query's service filter with the service in the route.

## Human OIDC configuration

```text
PORTAL_OIDC_ENABLED=true
PORTAL_OIDC_TENANT_ID=<tenant UUID>
PORTAL_OIDC_CLIENT_ID=<confidential browser application ID>
PORTAL_OIDC_CLIENT_SECRET=<secret reference>
PORTAL_OIDC_ISSUER=https://login.microsoftonline.com/<tenant>/v2.0
PORTAL_OIDC_DISCOVERY_URL=https://login.microsoftonline.com/<tenant>/v2.0/.well-known/openid-configuration
PORTAL_OIDC_REDIRECT_URI=https://gateway.example/admin-ui/auth/callback
PORTAL_OIDC_POST_LOGOUT_REDIRECT_URI=https://gateway.example/admin-ui
PORTAL_SESSION_TTL_SECONDS=28800
PORTAL_LOGIN_TTL_SECONDS=600
PORTAL_SESSION_COOKIE_SECURE=true
```

The callback validates single-use state, nonce, PKCE S256, signature, issuer,
tenant, audience, not-before, and expiry. A short-lived HttpOnly login cookie
binds the transaction to the browser that initiated sign-in, and expired or
abandoned transactions are pruned as new logins are created. Cookie-authenticated
Admin mutations also require the session-bound `x-csrf-token` value. Sign-out
revokes the Relayna session and then navigates through Entra's discovered
end-session endpoint before returning to `PORTAL_OIDC_POST_LOGOUT_REDIRECT_URI`.

## Workload monitoring configuration

```text
OWNER_ENTRA_AUTH_ENABLED=true
OWNER_ENTRA_TENANT_ID=<tenant UUID>
OWNER_ENTRA_AUDIENCE=api://relayna-gateway-owner
OWNER_ENTRA_ISSUER=https://login.microsoftonline.com/<tenant>/v2.0
OWNER_ENTRA_OIDC_DISCOVERY_URL=https://login.microsoftonline.com/<tenant>/v2.0/.well-known/openid-configuration
```

Register each managed identity in the portal with its tenant, client ID,
optional exact object ID, service, and required application role. The default
role is `gateway.monitor.read`. A valid tenant-wide token without an enabled
exact Relayna binding cannot read a service.

## Local development issuer

The local issuer is adapted from Arcweft's development OIDC fixture and refuses
production environments. Start it with:

```bash
node scripts/entra/development-oidc.mjs
```

Configure the gateway control listener on `127.0.0.1:18381` and use:

```text
PORTAL_OIDC_TENANT_ID=00000000-0000-0000-0000-000000000001
PORTAL_OIDC_CLIENT_ID=relayna-gateway-local
PORTAL_OIDC_CLIENT_SECRET=relayna-development-browser-secret
PORTAL_OIDC_ISSUER=http://127.0.0.1:18090
PORTAL_OIDC_DISCOVERY_URL=http://127.0.0.1:18090/.well-known/openid-configuration
PORTAL_OIDC_REDIRECT_URI=http://127.0.0.1:18381/admin-ui/auth/callback
PORTAL_OIDC_POST_LOGOUT_REDIRECT_URI=http://127.0.0.1:18381/admin-ui
PORTAL_SESSION_COOKIE_SECURE=false
OWNER_ENTRA_TENANT_ID=00000000-0000-0000-0000-000000000001
OWNER_ENTRA_AUDIENCE=api://relayna-gateway-owner
OWNER_ENTRA_ISSUER=http://127.0.0.1:18090
OWNER_ENTRA_OIDC_DISCOVERY_URL=http://127.0.0.1:18090/.well-known/openid-configuration
```

The account chooser provides pending, administrator, and service-owner-shaped
identities. They still begin pending; use break-glass access to approve and
assign them. The workload fixture uses client ID
`00000000-0000-0000-0000-000000000101`, object ID
`00000000-0000-0000-0000-000000000102`, and the development-only secret printed
in the source file. Never deploy this issuer or its fixed credentials.
