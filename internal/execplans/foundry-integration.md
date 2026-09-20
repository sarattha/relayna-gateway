# Azure Foundry provider and registered agents

This living ExecPlan follows ../../PLANS.md.

## Purpose / Big Picture

Operators can create an Azure Foundry provider connection, then register an existing agent or expose its project Responses endpoint through a Relayna service. Relayna authenticates callers with virtual keys and optional Entra profiles, enforces existing service policy, and acquires its own Azure access token. JSON and server-sent events retain Azure output and citations. The gateway does not author agents or execute their tools.

## Progress

- [x] (2026-09-20) Read architecture, compatibility guidance, and current Microsoft Foundry API documentation.
- [x] Add typed provider identity and service binding, persistence and validation.
- [x] Implement renewable Azure credentials and governed Responses forwarding.
- [x] Add consistent operator UI for both modes.
- [x] Verify with unit, database, mock Foundry end-to-end and Computer Use UI tests.
- [x] Add illustrated documentation and run full verification stack.

## Surprises & Discoveries

- Pingora prepares upstream headers before rewriting request bodies. Injecting an agent reference requires chunked upstream framing; retaining the caller Content-Length truncated JSON. The real mock E2E reproduces and verifies this fix.
- Foundry completed SSE events nest usage under response.usage. A bounded per-line observer records these counts without delaying chunks; oversized events intentionally leave usage unavailable.
- UI validation caught generic service credential/health labels and unsupported Accessa/Studio controls. Foundry uses Azure identity labels and only its applicable settings.

Existing registered services already provide the correct caller admission and streaming path. Their static credentials and generic URI rewriting cannot directly support Azure project endpoints, so Foundry needs an explicit typed binding. Foundry Responses state can expose conversations under a shared upstream identity; this integration exposes stateless POST responses and rejects server-side conversation references.

## Decision Log

- 2026-09-20: Preserve released v0.1.37 services, LiteLLM and route behavior. Add nullable configuration columns and a new provider kind; no rewrite of released contracts.
- 2026-09-20: Use project Responses API with a pinned name and optional version in registered-agent mode. Passthrough accepts the project Responses request shape. Both modes force store=false and reject background/state references. Stateful history must be submitted as explicit input messages. Do not expose Azure management, files or conversation APIs.
- 2026-09-20: Keep registered Foundry services in the existing internal-service policy namespace so service permissions remain authoritative. The provider connection is explicitly Azure Foundry.
- 2026-09-20: Support client secret and Kubernetes workload identity using Entra client credentials, plus Azure VM managed identity. Credentials remain server-side; redirects are never followed for token requests. Mock token/Foundry endpoints use loopback only.

## Outcomes & Retrospective

Implemented both bounded Responses modes and three Azure identity methods, with four real operator screenshots. Full verification passed (384 nextest tests), React 41 tests/100% coverage, changed Rust 510/517 lines (98.646%), strict docs and release metadata checks. No live Azure subscription was exercised; tenant RBAC/network/agent readiness remains a deployment smoke test. Details: ../test-reports/foundry-integration/verification.md.

## Context and Orientation

Repository root: /Users/jobz/Works/relayna-gateway. gateway-core owns provider_configs.rs and services.rs, gateway-store/src/postgres.rs owns persistence, gateway-proxy/src/pingora_plane.rs owns admission and streaming, and gateway-api/admin-ui/src owns the React/TypeScript operator UI. A virtual key is a Relayna caller credential, not an Azure credential. A registered service is a named governed route. Azure tokens belong to the configured provider identity.

## Compatibility Boundary

Latest release v0.1.37. Add nullable provider/service configuration with forward SQL migration. Existing rows keep NULL and their current behavior. No automatic conversion of existing services or keys. Foundry cannot be used to bypass service policy or profile assignments. Provider secrets are never returned by admin APIs.

## Plan of Work

Create gateway-core/src/foundry.rs for identity, binding and request validation. Add Foundry provider kind and service configuration, store validated JSON, and resolve enabled provider connections at request time. Add a bounded token cache and per-request Foundry context in gateway-proxy; retain bounded request buffering and existing streamed response forwarding. Add provider/service editor fields using existing design conventions. Test the full path with real PostgreSQL/Redis and local token/Foundry mocks. Document exact API scope, Azure role/identity setup, both UI flows and limitations with real screenshots.

## Validation and Acceptance

Tests must prove both modes forward to the correct Azure project endpoint, registered agent overrides cannot escape, invalid/disabled/unassigned keys make no upstream request, client credentials are stripped, token failures do not leak responses, expired tokens renew, disabled providers stop routing, JSON/SSE citations survive, limits and service policy apply, and unsupported paths/state fail closed. Verify create/edit/reload and validation in the UI with Computer Use. Run npm tests, TypeScript checks, React coverage, build embedded assets, strict MkDocs, and the mandatory verification script with DATABASE_URL and REDIS_URL set to local test services. Report measured coverage, not a blanket guarantee.

## Idempotence and Recovery

Migrations run through SQLx tracking. Tests create isolated names/UUIDs and clean up their own rows. Do not modify user Azure resources. Restart only the known local demo gateway after rebuilding. Existing services remain usable if Foundry configuration is absent or disabled. Do not drop persisted columns to roll back; disable Foundry services and revert runtime changes if necessary.

## Interfaces and Dependencies

Provider kind azure-foundry carries identity method, tenant/client identifiers and deployment-owned workload token file; client secrets use the existing write-only credential field. Service foundry binding references provider UUID and mode, agent name/version. Runtime POST /services/{name}/responses forwards to {project_endpoint}/openai/v1/responses. Scope https://ai.azure.com/.default. Token HTTP requests are bounded, timed out and redirect-disabled. No customer bearer is used as an upstream token.
