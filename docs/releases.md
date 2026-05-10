# Releases

Relayna Gateway uses `vMAJOR.MINOR.PATCH` Git tags. Version `0.0.3` is the current release target.

## Release Checklist

1. Update workspace crate versions.
2. Update `CHANGELOG.md` with release notes.
3. Run the full verification stack:

   ```bash
   cargo fmt --all --check
   cargo clippy --workspace --all-targets --all-features -- -D warnings
   cargo test --workspace --all-features
   node tests/admin-ui.test.mjs
   mkdocs build --strict
   ```

4. Build the release image:

   ```bash
   docker build -t relayna-gateway:0.0.3 .
   ```

5. Commit the release changes.
6. Create and push the tag:

   ```bash
   git tag -a v0.0.3 -m "Release v0.0.3"
   git push origin v0.0.3
   ```

The GitHub release workflow extracts release notes from the matching `CHANGELOG.md` section and publishes the Docker image to GitHub Container Registry.

For `v0.0.3`, the workflow publishes:

```text
ghcr.io/sarattha/relayna-gateway:0.0.3
ghcr.io/sarattha/relayna-gateway:0.0
ghcr.io/sarattha/relayna-gateway:latest
```
