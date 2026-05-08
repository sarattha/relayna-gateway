---
name: code-change-verification
description: Run the mandatory Relayna Gateway verification stack when changes affect Rust runtime code, tests, migrations, packaging, or build/test behavior.
---

# Code Change Verification

## Overview

Use this skill before marking work complete when changes affect Rust runtime
code, tests, database migrations, packaging, Docker, or build/test
configuration in the Relayna Gateway repository. The goal is to finish with
formatting, linting, and tests passing for the gateway workspace.

You can skip this skill for docs-only or repository metadata changes unless the
user asks for the full verification stack.

## Quick Start

1. Keep this skill at `./.codex/skills/code-change-verification` so it loads
   with the repository.
2. macOS/Linux: run `bash .codex/skills/code-change-verification/scripts/run.sh`.
3. Windows: run
   `powershell -ExecutionPolicy Bypass -File .codex/skills/code-change-verification/scripts/run.ps1`.
4. If any command fails, fix the issue and rerun the script.
5. Mark runtime work complete only when all required commands succeed.

## Manual Workflow

Run from the repository root in this order once `Cargo.toml` exists:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
```

For release-sensitive changes, also run:

```bash
cargo build --workspace --all-features
```

Do not skip failing steps. Stop, fix the failure, and rerun the full relevant
stack so the required commands pass in order.

## Scope Guidance

Run the full gateway stack for changes under `crates/`, `tests/`, `benches/`,
database migrations, `Cargo.toml`, `Cargo.lock`, `.cargo/`, Dockerfiles,
Makefile wrappers, or CI workflows that affect Rust build/test behavior.

For docs-only or repository metadata changes, record that the Rust verification
stack was not required.

If the repository has not yet added `Cargo.toml`, the helper scripts report that
the Rust workspace does not exist and skip the cargo commands. Do not use that
skip as a substitute for verification once runtime code exists.

## Resources

### `scripts/run.sh`

Executes the gateway verification sequence with fail-fast semantics from the
repository root.

### `scripts/run.ps1`

Windows-friendly wrapper that runs the same verification sequence with
fail-fast semantics from PowerShell.
