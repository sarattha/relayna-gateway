# Releases

Relayna Gateway uses `vMAJOR.MINOR.PATCH` Git tags. Version `0.1.14` is the current release target.

Version `0.1.14` includes Admin UI 2.0, operator governance, policy
governance, provider intelligence, observability analytics, supply-chain
hardening, LiteLLM `/v1/embeddings` passthrough, opt-in Entra ID and Apigee
front-door authorization, LiteLLM wildcard passthrough, per-route canonical
OpenAI mode selection, and Admin portal controls for passthrough path/method
and sensitive endpoint exposure. It also covers direct LiteLLM bearer
delegation for canonical direct-mode routes, trusted-ingress dashboard/admin
passthrough for explicitly exposed LiteLLM admin paths, and bearer-prefixed
custom LiteLLM credential header values.
See
[Current Feature Highlights](current-features.md),
[Entra ID Auth](entra-id-auth.md), and
[Apigee Gateway Path](apigee-gateway-path.md) for the feature overview.

## Release Checklist

1. Update workspace crate versions.
2. Update `CHANGELOG.md` with release notes.
3. Run the full verification stack:

   ```bash
   python3 scripts/validate-release-metadata.py v0.1.14
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
   docker build -t relayna-gateway:0.1.14 .
   ```

5. Commit the release changes.
6. Create and push the tag:

   ```bash
   git tag -a v0.1.14 -m "Release v0.1.14"
   git push origin v0.1.14
   ```

The GitHub release workflow validates that the tag version, workspace package
version, and matching `CHANGELOG.md` section agree before it builds or
publishes anything. It then extracts release notes from the matching changelog
section, publishes the Docker image to GitHub Container Registry, scans the
image, generates an SBOM, signs the image digest with Cosign keyless signing,
and attaches provenance.

For `v0.1.14`, the workflow publishes:

```text
ghcr.io/sarattha/relayna-gateway:0.1.14
ghcr.io/sarattha/relayna-gateway:0.1
ghcr.io/sarattha/relayna-gateway:latest
```

Release artifacts include `CHANGELOG.md` and an SPDX JSON SBOM named
`relayna-gateway-<tag>.spdx.json`. Verify image signatures with Cosign against
the GHCR image digest published by the release workflow.
