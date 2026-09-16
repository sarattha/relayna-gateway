# Service type presets

## Purpose

Operators choose a supported service type before configuring a registered service.
The create drawer supplies editable route, method, timeout and identity defaults;
Accessa app/channel inputs suggest the correct public route. Custom supports
manual configuration. Existing services and editing retain saved settings.

## Compatibility and decisions

Latest released boundary is v0.1.36. This adds UI creation templates only, using
existing service API fields; no persisted type, migration, runtime or protocol
change. Relayna HTTP is an HTTP upstream, not a task-execution implementation.
Presets do not guess Entra audience/scopes or upstream addresses. Switching
presets explicitly replaces template-controlled defaults, preserving user-owned
name, upstream, credentials and pricing. Manually edited routes are preserved
when the name/app/channel changes; selecting another preset resets the route.

## Progress

- [x] Inspect service form, validation, routing and shared UI guidance.
- [x] Add selector, guidance, route suggestions and conditional Accessa fields.
- [x] Add behavioral tests and run UI plus mandatory verification.
- [x] Verify desktop/mobile creation in browser; screenshots prepared for draft PR #119.

## Surprises & Discoveries

The service has no persisted type discriminator. Templates must map directly to
existing transport/identity fields rather than creating unsupported categories.

## Validation

Exercise every preset, required Accessa identity, preset switching, manual route
preservation and preservation of credentials/upstream/pricing. Verify generated
assets via npm build/test, run the mandatory stack because tests change, then
inspect the rendered create drawer and saved service through the local demo.

## Outcomes & Retrospective

Implemented seven editable creation presets using existing fields. Browser checks
saved HTTP and Accessa services and inspected desktop/mobile forms. Existing
service editing and API contracts remain unchanged.

Verification completed: UI tests/build, gateway build and strict docs build pass.
The complete mandatory stack passes, including 359 Nextest tests and all security
scanners (Semgrep: zero findings).
