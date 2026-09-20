# Foundry connection verification

Base: `5b7546d5d66ee00235d07c9a45533509bab5f81a`. Gateway version 0.1.38;
latest released compatibility boundary v0.1.37. No migration is added.

## Scope

The operator action checks a saved provider on the receiving gateway instance.
Fresh token acquisition uses the forwarding identity implementation without its
cache. A bounded GET agents probe checks project read access without invoking an
agent, following redirects or returning agent records. Disabled providers can be
checked but remain disabled for requests. Each completed diagnostic is audited.

## Evidence

- Focused real admin/Pingora/PostgreSQL/Redis mock Foundry E2E passes. Tests cover
  fresh exchanges, no agent execution, 200/401/403/404/429/503/302/400 probe outcomes,
  invalid success envelopes, unauthorized/missing-scope callers, missing providers,
  disabled-provider verification, failed credential exchange, skipped probes and
  secret/error-body redaction. Existing invocation/streaming assertions still pass.
- Token/probe unit tests exercise all three identity methods, missing/unreadable/
  empty/oversized workload assertions, token validation/cache rotation, bounded
  malformed/oversized responses, refused connections and real HTTP timeouts.
- React: 44 tests; 100% measured statements (388/388), branches (306/306), functions
  (150/150) and lines (326/326). Check state, retry clearing, errors, partial results,
  pending dismissal, focus/dialog lifecycle and instance details are covered.
- `npm test`, TypeScript check and embedded Vite build pass. A source-contract
  regression catches missing `/admin-ui/admin` in the verification action URL.
- Computer Use against the actual embedded UI verified workload identity success,
  project read 403, removed token file -> actionable failure/skipped probe, then
  restored token file -> success on retry. Two unedited screenshots are included
  in the Foundry guide. The local mock provider and temporary identity setup were
  removed after validation; no existing provider/service was changed.
- Strict MkDocs build and release metadata validation for v0.1.38 pass.

## Full verification

All ten mandatory verification commands passed: formatting, Clippy (warnings
as errors), workspace/all-feature tests, audit (three existing advisory exceptions),
deny, machete, nextest, Trivy, Gitleaks and Semgrep. Nextest: **388/388**, zero skipped.
Semgrep: zero findings across 79 targets under the two repository rules; Trivy and
Gitleaks: no findings. Final formatting/Clippy rerun includes the added CSRF test.

Changed production Rust: **168/170 executable lines (98.824%)**, measured against
`5b7546d`, excluding tests but retaining production error paths. The two uncovered
lines are store/audit failure returns in the admin handler. The Foundry proxy
changes are 126/126. See `changed-rust-coverage.json`. A full workspace LLVM run
was followed by an incremental run for the added admin-session/CSRF test; no
production lines changed between measurements.

## Limits

No live Azure tenant, AKS federation, Azure VM IMDS or production private endpoint
was exercised. Microsoft REST/RBAC references establish the read probe's documented
contract; mocks test gateway behavior. A read 403 can be legitimate for an
invocation-only role. Even both stages passing does not verify invocation, tools,
quota, another pod or future requests. These are scoped tests, not a zero-defect
claim.
