# Relayna Gateway root Makefile.

SHELL := /usr/bin/env bash
.SHELLFLAGS := -euo pipefail -c

CARGO ?= cargo
RUBY ?= ruby
PYTHON ?= python3

.DEFAULT_GOAL := help

.PHONY: help
help: ## Show this help
	@grep -E '^[a-zA-Z0-9_-]+:.*?## .*$$' $(MAKEFILE_LIST) | \
		awk 'BEGIN {FS = ":.*?## "}; {printf "\033[36m%-22s\033[0m %s\n", $$1, $$2}'

.PHONY: ensure-cargo
ensure-cargo:
	@if [[ ! -f Cargo.toml ]]; then \
		echo "No Cargo.toml found; skipping Rust workspace command."; \
		echo "Add Cargo.toml before relying on this skip for runtime, test, or build changes."; \
		exit 0; \
	fi

.PHONY: format
format: ## Check Rust formatting
	@if [[ -f Cargo.toml ]]; then \
		$(CARGO) fmt --all --check; \
	else \
		$(MAKE) ensure-cargo; \
	fi

.PHONY: format-fix
format-fix: ## Format Rust code in place
	@if [[ -f Cargo.toml ]]; then \
		$(CARGO) fmt --all; \
	else \
		$(MAKE) ensure-cargo; \
	fi

.PHONY: lint
lint: ## Run Clippy with warnings denied
	@if [[ -f Cargo.toml ]]; then \
		$(CARGO) clippy --workspace --all-targets --all-features -- -D warnings; \
	else \
		$(MAKE) ensure-cargo; \
	fi

.PHONY: test
test: ## Run Rust tests
	@if [[ -f Cargo.toml ]]; then \
		$(CARGO) test --workspace --all-features; \
	else \
		$(MAKE) ensure-cargo; \
	fi

.PHONY: admin-ui-test
admin-ui-test: ## Run admin portal static tests
	node tests/admin-ui.test.mjs

.PHONY: build
build: ## Build the Rust workspace
	@if [[ -f Cargo.toml ]]; then \
		$(CARGO) build --workspace --all-features; \
	else \
		$(MAKE) ensure-cargo; \
	fi

.PHONY: release-build
release-build: ## Build the Rust workspace in release mode
	@if [[ -f Cargo.toml ]]; then \
		$(CARGO) build --workspace --release --all-features; \
	else \
		$(MAKE) ensure-cargo; \
	fi

.PHONY: run-gateway
run-gateway: ## Run Relayna Gateway with local development defaults
	scripts/run-gateway.sh

.PHONY: check
check: ## Run format, lint, and test checks
	$(MAKE) format
	$(MAKE) lint
	$(MAKE) test
	$(MAKE) admin-ui-test

.PHONY: verify
verify: ## Run the repository Codex verification helper
	.codex/skills/code-change-verification/scripts/run.sh

.PHONY: metadata-check
metadata-check: ## Validate repository metadata files
	$(PYTHON) -m json.tool .github/codex/schemas/pr-labels.json >/dev/null
	$(RUBY) -ryaml -e 'Dir[".github/workflows/*.yml"].each { |path| YAML.load_file(path) }'
	@grep -q "Relayna Gateway" README.md
	@grep -q "docs/architecture.md" README.md
	@grep -q "Relayna Gateway" AGENTS.md
	@grep -q "Relayna Gateway" PLANS.md

.PHONY: docs-check
docs-check: ## Validate documentation anchors and optional MkDocs config
	@grep -q "Relayna Gateway" README.md
	@grep -q "docs/architecture.md" README.md
	mkdocs build --strict

.PHONY: clean
clean: ## Remove Rust and local cache artifacts
	rm -rf target
