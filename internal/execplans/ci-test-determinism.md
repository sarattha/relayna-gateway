# CI test determinism

## Purpose and scope

Fix PR #119's Rust check without changing runtime behavior. The reported store
assertion compares nanosecond clock precision with PostgreSQL microseconds.
Full verification also exposed concurrent budget tests reseeding one another's
Redis counters. Both changes are confined to tests and preserve released APIs.

## Progress

- [x] Read failed GitHub Actions job 104864991437.
- [x] Reproduce timestamp assertion locally with explicit nanoseconds.
- [x] Correct expected database precision; focused store test passes.
- [x] Scope budget rehydration test helper to each test's unique key.
- [x] Run focused integration tests and complete mandatory verification.
- [x] Push fixes and inspect replacement CI checks (Linux Rust job running).

## Surprises & Discoveries

macOS clock precision hid the timestamp assertion defect. An explicit fractional
second reproduces it independently of the host clock. Three concurrent budget
tests called a helper that seeded all database keys, allowing stale snapshots to
overwrite another test's reconciled counters.

## Decision Log

Use a nanosecond fixture and a microsecond expected value. Keep strict equality.
Filter only the test rehydration helper by key; leave production seeding intact.
Existing assertions continue to cover persisted spend, reservation preservation
and exclusion of unbudgeted keys.

## Validation

The nanosecond fixture failed before the expectation correction and passed
afterward. Run the budget integration suite, then the full repository script
with PostgreSQL and Redis enabled. Verify the pushed head's Rust CI result.

## Outcomes & Retrospective

Focused timestamp and all eight control-state integration tests pass. The full
mandatory script passes, including 359 Nextest tests and all security scans.
Fixes were pushed to PR #119; GitHub security, docs, metadata and Admin Portal
checks passed. The Linux Rust job was still running when this record was saved.
