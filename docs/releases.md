# Releases

Relayna Gateway uses `vMAJOR.MINOR.PATCH` Git tags. Version `0.1.26` is the
current release target.

Version `0.1.26` consolidates Entra-authenticated browser sessions and workload
API authorization onto one confidential Web/API application registration. It
defines separate `gateway.invoke` and `gateway.monitor.read` application roles
for least-privilege managed identities and replaces the duplicated startup
audience/client-ID variables with `ENTRA_APPLICATION_ID`.

It retains Entra-authenticated browser sessions for administrators
and registered service owners, exact Owner and Viewer service memberships,
scoped service monitoring APIs, and managed-identity workload bindings. It also
adds owner incident charts for error rate and P95 latency, validated service-
version observations, all-request filtering and pagination, and exact-service
sanitized request details with optional debug bundles.
The portal confidential client uses certificate-backed PS256
`private_key_jwt`, the production control Ingress exposes `/owner/v1`, and
first-admin bootstrap is bound to tenant, immutable object ID, and email.
Existing operator tokens remain available for emergency access. It retains
endpoint-level failure monitoring, body admission, OpenAPI endpoint billing,
persisted timeout handling, the Aurora Teal Admin UI 2.0 shell, policy
governance, provider intelligence, supply-chain hardening, LiteLLM passthrough
and credential mapping, and opt-in Entra ID and Apigee front-door authorization.
See
[Current Feature Highlights](current-features.md),
[Entra Portal and Service-owner Monitoring](operations/entra-portal-and-owner-monitoring.md),
[Entra Integration Requirements](operations/entra-integration-requirements.md),
[OpenAPI Service Pricing](openapi-service-pricing.md),
[Entra ID Auth](entra-id-auth.md), and
[Apigee Gateway Path](apigee-gateway-path.md) for the feature overview.

## Release Checklist

1. Update workspace crate versions.
2. Update `CHANGELOG.md` with release notes.
3. Run the full verification stack:

   ```bash
   python3 scripts/validate-release-metadata.py v0.1.26
   cargo fmt --all --check
   cargo clippy --workspace --all-targets --all-features -- -D warnings
   cargo test --workspace --all-features
   cargo audit --ignore RUSTSEC-2023-0071 --ignore RUSTSEC-2024-0437
   cargo deny check
   cargo machete
   cargo nextest run --workspace --all-features
   trivy fs --severity HIGH,CRITICAL --exit-code 1 --skip-dirs target --skip-dirs site .
   gitleaks detect --source . --redact
   semgrep scan --config .semgrep.yml
   node tests/admin-ui.test.mjs
   mkdocs build --strict
   ```

4. Build the release image:

   ```bash
   docker build -t relayna-gateway:0.1.26 .
   ```

5. Commit the release changes.
6. Create and push the tag:

   ```bash
   git tag -a v0.1.26 -m "Release v0.1.26"
   git push origin v0.1.26
   ```

The GitHub release workflow validates that the tag version, workspace package
version, and matching `CHANGELOG.md` section agree before it builds or
publishes anything. It then extracts release notes from the matching changelog
section, publishes the Docker image to GitHub Container Registry, scans the
image, generates an SBOM, signs the image digest with Cosign keyless signing,
and attaches provenance.

For `v0.1.26`, the workflow publishes:

```text
ghcr.io/sarattha/relayna-gateway:0.1.26
ghcr.io/sarattha/relayna-gateway:0.1
ghcr.io/sarattha/relayna-gateway:latest
```

Release artifacts include `CHANGELOG.md` and an SPDX JSON SBOM named
`relayna-gateway-<tag>.spdx.json`. Verify image signatures with Cosign against
the GHCR image digest published by the release workflow.
