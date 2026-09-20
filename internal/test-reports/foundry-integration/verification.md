# Azure Foundry integration verification

Date: 2026-09-20. Base: `53f90d9fdeb668705cc426a429ead81e94513547`.
Release target remains 0.1.38; latest released compatibility baseline is v0.1.37.

## Automated checks

- Rust formatting and all-target/all-feature Clippy: passed.
- Workspace all-feature tests with PostgreSQL and Redis: passed.
- Mock Foundry E2E: passed through real admin HTTP and Pingora, isolated PostgreSQL,
  Redis, mock Entra token endpoint and mock Foundry Responses API.
- Changed production Rust LLVM line coverage: **510/517 (98.646%)**.
  See `changed-rust-coverage.json`; production error paths are not excluded.
- React: **41 tests**, 100% statements (363), branches (288), functions (144), lines (310).
- Existing admin, design-system and Traffic filter suites: passed, including 192
  Traffic filter combinations. TypeScript and embedded Vite asset build: passed.
- Strict MkDocs build and Git whitespace check: passed.
- Full verification script: all ten commands passed. Nextest **384/384**, zero skipped; audit with existing exceptions, deny, machete, Trivy, Gitleaks and Semgrep passed.

## Foundry contract coverage

- Both provider/service modes, create/edit/readback and provider reference protection.
- Azure client-secret, workload assertion and VM managed-identity token contracts;
  invalid responses, redirects, malformed/missing tokens, oversized responses and
  assertion files, token expiry, revision invalidation and bounded cache behavior.
- Fixed registered agent name/version and rejection of client definition overrides.
- Passthrough preserves model/instructions while restricting request path and state.
- Invalid, unassigned and disabled keys denied before agent execution; existing
  shared profile suites cover Entra/key-only profiles, aliases and revisions.
- Complete-body model, streaming, tools and body-size policy checks.
- Caller credential stripping; Azure credentials absent from admin responses and traffic.
- JSON and streamed citation preservation; first SSE delta arrives before delayed
  completion; JSON and SSE token usage are recorded.
- Azure 429 and Retry-After; credential rotation/failure, disabled connections;
  forbidden Studio/generic-upstream configuration combinations.
- Core tests cover malformed input, inline history, stored-state references,
  unsupported methods/paths/query strings, endpoint validation and secret-safe Debug.

## Computer Use walkthrough

Used the Computer Use plugin in Chrome against the locally rebuilt embedded UI.
Created a fictional workload-identity connection, a registered agent and a
passthrough service. Verified conditional fields, create/save/reload, persisted
agent name/version, version edit, invalid-agent error with retained draft, and
successful correction. Verified Foundry services show Azure identity and Not
probed rather than missing generic credentials. Checked the final caller-auth
section omits Accessa/Studio controls. Captured real UI images for the guide.

Screenshots use fictional Azure values; no Azure resources were created or changed.
The two local service fixtures and their connection were removed after validation.
Staged-file Gitleaks and Semgrep checks also passed.

## Limits of verification

No live Azure tenant was used. Tenant RBAC, private networking, deployed agent
compatibility and Bing grounding configuration require a real-environment smoke
test. The implemented surface is stateless text Responses invocation; classic
Assistants, stored conversation/file APIs and delegated OBO are outside scope.
Usage inspection is intentionally bounded; missing/oversized usage is not
fabricated. Existing dependency audit exceptions remain unchanged.
