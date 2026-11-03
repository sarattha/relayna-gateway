# Relayna dependency patch

This is the Apache-2.0 `pingora-header-serde` 0.9.0 crates.io source. Upstream
Rust sources are unchanged. Only Cargo.toml's zstd requirements change:

- zstd 0.13.1 → 0.14.3
- zstd-safe 7.1.0 → 8.2.0

Pingora 0.9 fixes RUSTSEC-2026-0388/0389/0390, but its header codec still pins
the old compression bindings. This patch allows the fixed zstd dependency
family to resolve without retaining vulnerable copies. The workspace patches
this crate and the production Dockerfile includes it. UPSTREAM.json records
the original package archive and untouched source hashes; LICENSE is retained.
Remove the patch after a released Pingora header codec accepts the fixed
compression dependency family.

Verify the unchanged codec's own tests with:

    cargo test --manifest-path vendor/pingora-header-serde/Cargo.toml
