# Releases

Relayna Gateway uses `vMAJOR.MINOR.PATCH` Git tags. Version **1.0.0** was
[released October 25, 2026](https://github.com/sarattha/relayna-gateway/releases/tag/v1.0.0).
The release attaches the changelog and an SPDX SBOM. The documentation hosts
[versioned evidence](benchmarks/v1.0.0.md), raw and failed trials, checksums and
console screenshots.

The published image is **linux/amd64**:
`ghcr.io/sarattha/relayna-gateway:1.0.0`, with immutable index digest
`sha256:20a17e64b435abac5f8fa41c469969b2d7871af275f8686fa7732cb3f9bf4d21`.
Its signature and provenance identify the tagged merge commit
`e5200b662a7c1a3d085c00f56bf8ed09c1293f88`. The native ARM64 benchmark
used a separate source-built image; it does not imply an ARM64 release image.

The major release combines scalable all-project Usage, HTTP/SSE and WebSocket
monitoring, live gateway capacity, and an expanded operator console.
Read the [console tour](v1.0.0.md), [upgrade guide](deployment.md#upgrade-to-100)
and [monitoring semantics](operations/gateway-monitoring.md). Released v0.1.49
proxy, identity, Usage and budget contracts remain supported.

Every version starting with v0.1.49 requires a full body-admission benchmark
and versioned results before release. See the [benchmark history and release
protocol](benchmarks/index.md). The [v0.1.49 results](benchmarks/v0.1.49.md)
establish the initial six-size native amd64 resource baseline, including memory,
latency, admission outcomes and raw evidence.

Version `0.1.49` excludes Node.js releases affected by CVE-2026-111497 from
build/test tooling and helper containers. npm enforces patched 24.x/26.x
versions, CI selects patched Node before Rust tests that launch the OIDC helper,
and local/test/benchmark images use Node.js 22.23.4. Rebuild and recreate existing
helper containers; no database migration or runtime API change is required. See
[tooling requirements](getting-started.md#install-tools) and
[upgrade guidance](deployment.md#upgrade-to-0149).

Version `0.1.48` fixes caller-owned LiteLLM WebSocket authentication when the
active provider uses a custom credential header. The configured header keeps
its raw/bearer format, and socket handshakes also receive Bearer Authorization
with the same caller key. HTTP and registered-service credentials retain their
existing behavior. Real LiteLLM 1.100.1 and pinned 1.106.0 each passed 16 checks
with a custom-header requirement, native provider socket frames and Sol-only
model restrictions. No migration or new configuration is required. See
[WebSocket credentials](websocket-transport.md#caller-owned-litellm-passthrough)
and [upgrade guidance](deployment.md#upgrade-to-0148).

Version `0.1.47` improves body admission and large-request memory reclamation.
Slots follow retained upload buffers rather than upstream response-header
latency, and the Debian image pins glibc's mmap threshold at 128 KiB. Complete
policy/guardrail checks and byte bounds remain enforced before forwarding;
rewritten-body capacity is checked before budget reservation. Native amd64
validation passed the two-pod controlled matrix and separate burst sweep at
8/16/24/32/64 slots. Defaults stay at eight slots and 512 MiB. See
[operations](operations.md#configuration), [upgrade guidance](deployment.md#upgrade-to-0147),
and the [native amd64 report](https://github.com/sarattha/relayna-gateway/blob/8903a62191d461f12c53d10108b1c880452945eb/internal/test-reports/body-admission-5mib/amd64/README.md).
This patch also includes the WebPKI/root-data updates and WebSocket test timing
fix merged after 0.1.46. No migration or new Redis format is introduced.

Version `0.1.46` adds opt-in HTTP/WebSocket transport for registered services
and caller-owned LiteLLM passthrough, including Realtime and direct Responses
routes. Transport has frame/message, connection, idle and session limits;
LiteLLM retains session governance and accounting. Existing HTTP and Accessa
contracts remain covered. No new SQL migration or Redis format is introduced.
Upgrade all replicas before enabling `access.websocket`; see
[WebSocket transport](websocket-transport.md) and
[deployment](deployment.md#upgrade-to-0146).

Version `0.1.45` adds configurable, opt-in forwarding of verified Accessa user
JWTs and PUT-capable channel provisioning. It fixes header-setting collisions
and conversion controls in service editing, and patches Rustls and Quinn
vulnerabilities. Upgrade all replicas before saving forwarding configuration;
see [Accessa forwarding](accessa-channels.md#verified-user-token-forwarding) and
[deployment](deployment.md#upgrade-to-0145).

Version `0.1.44` restores complete-request admission in Relayna-authenticated
direct mode, requires provider-enforced output maxima, blocks shared batch
lifecycle access and records passthrough service costs beyond the response
prefix. Nested model overrides cannot bypass model policy, and batch policy
checks reuse each distinct model without repeatedly parsing the full body.
There is no new migration or Redis format. Upgrade every replica and review
new request restrictions; see [deployment](deployment.md#upgrade-to-0144) and
[LiteLLM admission](litellm-passthrough.md#output-token-admission).

Version `0.1.43` fixes historical selector attribution, service-price precision
and Foundry connection-pool exhaustion from PR #126. The additive fingerprint
migration has no backfill; unproven historical rule costs stay unchanged.
Service amounts support eight decimal places without changing the ledger.
See [deployment](deployment.md#upgrade-to-0143) and
[service pricing](openapi-service-pricing.md#changing-prices-for-recorded-requests).

Version `0.1.42` fixes small service prices and adds optional historical reporting
cost updates while preserving original budget charges. Startup applies the
additive historical-pricing migration and index. Upgrade all budget readers
before opting in; see [deployment](deployment.md#upgrade-to-0142) and
[service pricing](openapi-service-pricing.md#changing-prices-for-recorded-requests).

Version `0.1.41` strengthens request governance, accounting and resource bounds.
Drain old replicas before changing the Redis accounting writer; recovery after
state loss also requires draining and restarting. See [operations](operations.md#budgets-and-rate-limits).

Version `0.1.40` added independent LiteLLM caller-key authentication and path
deny rules, with additive defaults and Admin UI controls. Upgrade all replicas
before opting in; see [LiteLLM passthrough](litellm-passthrough.md).

Version `0.1.39` added Admin UI 4.0, explicit authentication profiles, editable key
names, corrected Traffic filters and automatic refresh of persisted gateway Entra
settings across replicas. The [illustrated profile guide](operations/authentication-profiles.md)
explains both authentication types and key assignment. Upgrade every replica
before opting in; old binaries cannot read saved profiles. See
[deployment](deployment.md) for migrations, synchronization and rollback.

Azure Foundry is available through **Providers → Create provider → Azure Foundry**.
Connections support workload, managed and client-secret identities, read-only
connection verification, registered agents and project Responses passthrough.
See the [illustrated Foundry guide](azure-foundry.md), including API and streaming
limitations.

Version `0.1.37` added Accessa channel WebSockets, endpoint-specific Entra
requirements, service creation presets and WebSocket/verified-identity diagnostics
in Traffic and Usage. Apply the additive endpoint-access and route-identity
migrations through normal startup; existing routes retain inherited behavior.
Deploy adapter/Router admission support before enabling channel bindings. See
[Accessa channels](accessa-channels.md) for setup, rollout and measurement limits.

Version `0.1.36` makes `POST /embeddings` a LiteLLM alias of
`POST /v1/embeddings`, sharing mode, policy, limits, and usage identity while
preserving the requested path upstream. Explicit registered `/embeddings`
services retain precedence and their legacy policy identity, so linked service
keys continue to work. No database migration is introduced. See
[LiteLLM route precedence](litellm-passthrough.md#route-precedence).

Version `0.1.35` added LiteLLM-reported spend for mapped keys, improved Overview
analytics reliability, and protected show-once credentials from accidental
dismissal. See [Admin portal](admin-portal.md#overview-reliability-and-litellm-reported-spend).

Version `0.1.34` added `unverified_bearer_enabled` for temporary troubleshooting
of gateway-managed requests. Clients keep both headers while Gateway validates
only the Relayna virtual key and suspends Entra verification. Admin Settings
makes the active mode explicit and restores the saved verifier when the option
is cleared. Trusted Apigee headers cannot be combined with it. Native bearer-key,
direct LiteLLM passthrough, portal and owner authentication defaults are unchanged.

Apply the additive false-default auth-settings migration through normal startup.
Existing rows stay disabled. Disable the option before rolling back binaries;
the extra column may remain. See [Entra authentication](entra-id-auth.md#temporarily-pause-verification-while-keeping-both-headers)
for configuration, trust semantics and replica rollout guidance.

It retains the `0.1.26` single-application Entra contract,
Entra-authenticated browser sessions for administrators
and registered owners, exact Owner and Viewer service memberships, scoped
service monitoring APIs, and managed-identity workload bindings. The portal includes
owner incident charts for error rate and P95 latency, validated service-
version observations, all-request filtering and pagination, and exact-service
sanitized request details with optional debug bundles.
The portal confidential client uses certificate-backed PS256
`private_key_jwt`, the production control Ingress exposes `/owner/v1`, and
first-admin bootstrap is bound to tenant, immutable object ID, and email.
Existing operator tokens remain available for emergency access. It retains
endpoint-level failure monitoring, body admission, OpenAPI endpoint billing,
persisted timeout handling, the Admin UI 4.0 shell, policy
governance, provider intelligence, supply-chain hardening, LiteLLM passthrough
and credential mapping, and opt-in Entra ID and Apigee front-door authorization.
See
[Current Feature Highlights](current-features.md),
[Entra Portal and Service-owner Monitoring](operations/entra-portal-and-owner-monitoring.md),
[Entra Integration Requirements](operations/entra-integration-requirements.md),
[OpenAPI Service Pricing](openapi-service-pricing.md),
[Entra ID Auth](entra-id-auth.md),
[Entra Authorization Debug Mode](operations/entra-authorization-debug.md), and
[Apigee Gateway Path](apigee-gateway-path.md) for the feature overview.

## Release Checklist

Local verification and CI/release checks use Rust `1.98.0`, pinned in
`rust-toolchain.toml` and the workflow toolchain actions. Update these pins
together and run the verification stack when upgrading the compiler. This
avoids changing the required lint baseline whenever the stable channel moves.

1. Update workspace crate versions.
2. Update `CHANGELOG.md` with release notes.
3. Run the full verification stack:

   ```bash
   python3 scripts/validate-release-metadata.py v1.0.0
   cargo fmt --all --check
   cargo clippy --workspace --all-targets --all-features -- -D warnings
   cargo test --workspace --all-features
   cargo audit --ignore RUSTSEC-2023-0071 --ignore RUSTSEC-2024-0437 --ignore RUSTSEC-2026-0173 --ignore RUSTSEC-2026-0381
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
   docker build -t relayna-gateway:1.0.0 .
   ```

5. Commit the final runtime candidate, run the fresh six-size adaptive benchmark
   with at least three fresh-pod trials per confirmed setting, and publish its
   raw evidence and versioned results. Finish the continuous ten-minute
   HTTP/SSE/WebSocket CI check, controlled forwarding-latency comparison, and
   required PR reviews/checks. Extended 24-hour soaks are intentional manual
   runs; they are not CI, PR or release gates.
   Results/documentation commits may follow an unchanged measured runtime.
6. Merge the approved PR only after these gates pass.
7. Create and push the tag at the merged release commit:

   ```bash
   git tag -a v1.0.0 -m "Release v1.0.0"
   git push origin v1.0.0
   ```

The GitHub release workflow validates that the tag version, workspace package
version, and matching `CHANGELOG.md` section agree before it builds or
publishes anything. It then extracts release notes from the matching changelog
section, publishes the Docker image to GitHub Container Registry, scans the
image, generates an SBOM, signs the image digest with Cosign keyless signing,
and attaches provenance.

For `v1.0.0`, the workflow publishes:

```text
ghcr.io/sarattha/relayna-gateway:1.0.0
ghcr.io/sarattha/relayna-gateway:1.0
ghcr.io/sarattha/relayna-gateway:latest
```

Release artifacts include `CHANGELOG.md` and an SPDX JSON SBOM named
`relayna-gateway-<tag>.spdx.json`, alongside GitHub's automatic source archives.
Use the Git tag as the release title. Keep benchmark reports, supporting evidence
and screenshots in documentation hosting, outside the release asset list.
Verify image signatures with Cosign against the GHCR image digest published by
the release workflow.

The documentation deployment retrieves the v1.0.0 assets from an immutable
[documentation archive](https://github.com/sarattha/relayna-gateway/tree/e38a6ed887a0e8797650a8d55b4a98187e0242d5),
verifies the original SHA-256 manifest and publishes them with the site. Original
evidence stays outside the application source tree. The interactive report copy
only changes its download links to documentation URLs; the original report and
checksums remain available in the archive.
