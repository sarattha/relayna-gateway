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
run_step cargo audit --ignore RUSTSEC-2023-0071 --ignore RUSTSEC-2024-0437 --ignore RUSTSEC-2026-0173
run_step cargo deny check
run_step cargo machete
run_step cargo nextest run --workspace --all-features
run_step trivy fs --severity HIGH,CRITICAL --exit-code 1 --skip-dirs target --skip-dirs site .
run_step gitleaks detect --source . --redact
run_step semgrep scan --config .semgrep.yml

echo "code-change-verification: all commands passed."
