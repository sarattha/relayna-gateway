# Route authentication profiles

Authentication profiles let one canonical route or registered service accept
several explicitly assigned caller populations. The forwarding mode remains a
separate setting. An Entra profile requires both a Relayna virtual key and a
verified identity; a key-only profile still authenticates a Relayna virtual key.
Native LiteLLM credentials are not an authentication profile.

## Configure a route

In **Routes → Edit identity**, choose **Explicit authentication profiles**.
Add named profiles with stable IDs, enabled state and authentication type.
For Entra profiles, configure audience, required scopes, required roles and
allowed groups. All scopes and roles are required; any listed group may match.
Tenant, issuer, signing algorithms, JWKS and JWT clock skew still come from
Settings. Signed Apigee identity is accepted only when enabled on the selected
profile; its expiry remains strict.

Assign existing key UUIDs to each profile. A key may have exactly one assignment
on a route; an absent, disabled or invalid assignment denies access. Assigning a
profile never grants a route forbidden by the key's effective policy. Existing
route, project, provider, rate and budget controls continue to apply according
to the forwarding mode. Aliases share the canonical route's profile set.

Registered services expose the same editor under **Endpoint identity and
Accessa**. Project-owned services accept bindings only for keys in that project.
Accessa profiles must all require Entra, including disabled profiles.

In **Virtual keys → Edit → Inspect and assign route authentication profiles**,
inspect inherited/legacy routes and save individual explicit route assignments.
No key secret is displayed or recreated. Assignments are saved separately from
key lifecycle edits. Profile editing uses existing administrator scopes and
audited route/service update endpoints; service owners cannot use admin APIs.

Disabling a profile blocks all its assigned keys. Remove its assignments
explicitly before removing the profile. To reassign callers, edit the profile
and binding set in one save. Never use profile order as authorization. Retain at
least one profile after opting in; to stop all access, disable every profile.
Returning an opted-in route to legacy authentication is intentionally rejected.

## API and revisions

The existing route API is `PUT /admin-ui/admin/route-identities`. Service create
and PATCH APIs accept the same `access` object. GET route identities and service
responses provide the effective saved configuration and revision. For example:

```json
{
  "route": "/v1/responses",
  "access": {
    "authentication_profiles": {
      "revision": 0,
      "profiles": [
        {
          "id": "employees",
          "name": "Employee applications",
          "enabled": true,
          "type": "entra_and_relayna_key",
          "entra": {
            "audience": "api://employees",
            "required_scopes": ["responses.invoke"],
            "required_roles": [],
            "allowed_groups": []
          }
        },
        {
          "id": "automation",
          "name": "Internal automation",
          "enabled": true,
          "type": "relayna_key_only"
        }
      ],
      "bindings": []
    }
  }
}
```

The empty bindings above deliberately deny every key. Add objects containing
`key_id` and `profile_id` for the approved callers before enabling production
traffic. Revision zero is used for initial opt-in. Subsequent writes submit the
revision returned by GET or the last successful write. PostgreSQL atomically
checks and increments revisions for changed access configuration. A stale write
returns 409 `authentication_profile_conflict`; reload and reconcile instead of
blindly retrying. An unchanged policy may retain its revision. Invalid profile
references, duplicate bindings and unknown key UUIDs reject the entire save.

There are at most 32 profiles and 1,024 key assignments per route. IDs are 1–64
ASCII letters, digits, hyphens or underscores; names are at most 120 bytes and
unique ignoring case and surrounding whitespace. Each claim list has at most
64 entries, each at most 512 bytes. IDs preserve bindings across name changes.
Legacy `entra` and `skip_entra` are mutually exclusive with explicit profiles.

## Credential contract

For Entra + key, put the JWT in `Authorization: Bearer <JWT>` and the Relayna
key in the configured key header (normally `x-relayna-key`). Signed Apigee
requests use that same key header and existing signed identity headers.

For key-only, use either the configured key header **or**
`Authorization: Bearer <Relayna key>`. Sending both is rejected, even when the
keys match. Duplicate Authorization/key headers, extra bearer/JWT credentials
with a dedicated key, or Apigee headers on a key-only profile are rejected.
Missing or rejected Entra credentials never fall back to key-only. There is no
client profile selector; caller-controlled selector or identity hints cannot
change the profile selected by the authenticated key.

The gateway preserves existing upstream credential mapping, strips the client
key/JWT, and strips caller-supplied internal admission credentials. Only eligible
Accessa paths receive a gateway-issued admission context.

## Diagnostics and socket lifecycle

Traffic **Inspect** and Usage **Debug** show profile ID/name, revision,
authentication type, route-key binding source and outcome. Entra requirements
and successfully verified claims appear alongside the profile. Key-only traffic
says **Entra not required by selected profile**. Credential failures, selection
failures and identity failures remain distinct. Missing or ambiguous bindings
return a sanitized 403 `authentication_profile_denied`; invalid credentials use
existing 401 errors, and insufficient verified claims use 403. Storage failures
remain unavailable errors and never select weaker access.

Snapshots retain the name and revision at request time; later edits cannot
rewrite past events. Old records without profile fields remain readable. No JWT,
key, admission token or payload is recorded, and no profile identifiers are
added to metrics labels.

Accessa handshakes save the full profile configuration and selected profile in
the existing Redis admission session. Each fresh turn rechecks the key and reads
the current service policy. Disabling, rebinding, renaming or editing any part
of that route's identity configuration blocks subsequent admission on existing
sockets. Reconnect to select a fresh profile; the gateway never silently changes
identity mid-session. Already admitted work is not retroactively cancelled.

## Rollout, compatibility and rollback

Apply migration `20260919000100_authentication_profile_revisions.sql` before
using profiles. It adds write guards to the existing route/service access JSON;
no legacy rows are rewritten. Existing inherited Entra, explicit single-policy,
No Entra and native direct LiteLLM routes retain released v0.1.37 behavior until
explicitly opted in. Inherited routes still follow later gateway-setting edits.

Upgrade every replica before opting in. Effective policy is read from PostgreSQL
for every new request and Accessa turn, with no profile cache. Committed edits
therefore affect the next policy read; requests already past that read may finish
under their recorded snapshot. Store failures fail closed.

v0.1.37's strict access deserializer rejects the new profile field. The database
write guards also reject old writers attempting to erase an opted-in policy.
Mixed-version deployments can therefore deny affected routes on old replicas;
they cannot safely serve the new feature. This is an availability consideration,
not a migration path for keeping old replicas active.

For application rollback, keep the migration and write guards installed and
leave opted-in routes unavailable on old binaries. Do not drop the guards or
rewrite access JSON to bypass rejection. Restore the upgraded binary to regain
access. To avoid disruption, complete an all-replica upgrade and validate a
nonproduction route before opting in. Shared profile templates, native LiteLLM
key profiles and multiple issuer trust authorities are deferred.
