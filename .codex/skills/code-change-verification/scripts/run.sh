#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT=""

if command -v git >/dev/null 2>&1; then
  REPO_ROOT="$(git -C "${SCRIPT_DIR}" rev-parse --show-toplevel 2>/dev/null || true)"
fi

if [[ -z "${REPO_ROOT}" ]]; then
  REPO_ROOT="$(cd "${SCRIPT_DIR}/../../../.." && pwd)"
fi

cd "${REPO_ROOT}"

if [[ ! -f Cargo.toml ]]; then
  echo "code-change-verification: no Cargo.toml found; skipping Rust workspace checks."
  echo "Add Cargo.toml before relying on this skip for runtime, test, or build changes."
  exit 0
fi

run_step() {
  echo "Running $*..."
  "$@"
}

run_step cargo fmt --all --check
run_step cargo clippy --workspace --all-targets --all-features -- -D warnings
run_step cargo test --workspace --all-features

echo "code-change-verification: all commands passed."
