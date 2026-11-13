# Relayna dependency patch

This is the MIT OR Apache-2.0 `sqlx-sqlite` 0.8.6 crates.io source. All
upstream Rust sources and both license files are unchanged. The only upstream
manifest change is the `libsqlite3-sys` requirement:

- `^0.30.1` → `>=0.39.0, <0.40.0`

The workspace lockfile resolves 0.39.1, which contains the SQLite fix for
[RUSTSEC-2026-0459](https://rustsec.org/advisories/RUSTSEC-2026-0459/).
Gateway uses SQLx's PostgreSQL driver; SQLite is not enabled in its runtime
feature graph. Cargo nevertheless includes optional drivers in the lockfile,
which is checked by the repository's dependency audit.

SQLx explicitly treats SQLite binding upgrades as semver-exempt. Its latest
published 0.9.0 driver still caps the bindings below 0.38.0, while upstream
main accepts versions below 0.40.0. This narrow dependency backport avoids an
unrelated migration of the PostgreSQL API. Remove the patch when a released
SQLx driver accepts fixed SQLite bindings.

`UPSTREAM.json` records the original package archive, its SHA-256 checksum,
Git revision, and the hashes of all unchanged Rust source files.

From the repository root, verify the bundled bindings and upstream driver
tests with:

```sh
cargo test --manifest-path vendor/sqlx-sqlite/Cargo.toml --features bundled --config 'patch.crates-io.sqlx-sqlite.path="vendor/sqlx-sqlite"'
python3 -c 'from pathlib import Path; Path("vendor/sqlx-sqlite/Cargo.lock").unlink(missing_ok=True)'
```

The explicit self-patch also directs the standalone package's SQLx test
dependency to this copy. It does not enable SQLite in Gateway's runtime.
