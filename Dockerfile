# syntax=docker/dockerfile:1.7

FROM rust:1-trixie AS builder
WORKDIR /app

RUN apt-get update \
    && apt-get install -y --no-install-recommends pkg-config libssl-dev ca-certificates cmake \
    && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY vendor ./vendor

RUN cargo build --locked --release --bin gateway-api

RUN groupadd --system --gid 10001 relayna \
    && useradd --system --uid 10001 --gid 10001 --home-dir /nonexistent --no-create-home relayna

# Debian 13/Distroless has not yet shipped the zlib fixes. Use Debian's exact
# patched package until the pinned base includes it. Keep its genuine control
# metadata and payload checksums for vulnerability scanning and the SPDX SBOM.
FROM builder AS runtime-zlib
ARG TARGETARCH
RUN set -eu; \
    case "$TARGETARCH" in \
      amd64) checksum=8506ce550a81ac14b047608e0df121037d9cbb10c3cc28446f68cf8c1d9d31ce ;; \
      arm64) checksum=aaf9e8cfc7d86771144c660d544806c886a5ae406f97bd50b62588a0718229ab ;; \
      *) echo "Unsupported runtime architecture: $TARGETARCH" >&2; exit 1 ;; \
    esac; \
    curl --fail --location --retry 3 --output /tmp/zlib.deb \
      "https://deb.debian.org/debian/pool/main/z/zlib/zlib1g_1.3.dfsg+really1.3.2-4_${TARGETARCH}.deb"; \
    printf '%s  /tmp/zlib.deb\n' "$checksum" | sha256sum --check -; \
    test "$(dpkg-deb --field /tmp/zlib.deb Package)" = zlib1g; \
    test "$(dpkg-deb --field /tmp/zlib.deb Version)" = '1:1.3.dfsg+really1.3.2-4'; \
    test "$(dpkg-deb --field /tmp/zlib.deb Architecture)" = "$(dpkg --print-architecture)"; \
    dpkg-deb --extract /tmp/zlib.deb /patched-zlib; \
    dpkg-deb --control /tmp/zlib.deb /zlib-control; \
    mkdir -p /patched-zlib/var/lib/dpkg/status.d; \
    cp /zlib-control/control /patched-zlib/var/lib/dpkg/status.d/zlib1g; \
    cp /zlib-control/md5sums /patched-zlib/var/lib/dpkg/status.d/zlib1g.md5sums

# This tool is mounted only while constructing the image; it is never shipped.
FROM busybox:1.37.0-musl@sha256:f6a95d611e0a8780fabdb2aec43e31275b2720dfefa3e41b16835bfc42454478 AS runtime-tools

# Includes glibc, libgcc, OpenSSL and CA roots, without distribution utilities.
# Pin the multi-platform index; refresh deliberately and scan every candidate.
FROM gcr.io/distroless/cc-debian13:nonroot@sha256:08476c244888bfb425bae1ae4dd7c17f093e972aab8426883d97f3c6f280f7c1 AS runtime
USER 0
# Remove the actual old library and its documentation, not just its inventory.
RUN --mount=type=bind,from=runtime-tools,source=/bin/busybox,target=/tmp/busybox,ro \
    ["/tmp/busybox", "sh", "-ec", "test -f /var/lib/dpkg/status.d/zlib1g; /tmp/busybox rm -f /usr/lib/*/libz.so.1 /usr/lib/*/libz.so.1.*; /tmp/busybox rm -rf /usr/share/doc/zlib1g"]
COPY --from=runtime-zlib /patched-zlib/ /
WORKDIR /app

COPY --from=builder /etc/passwd /etc/group /etc/
COPY --from=builder /app/target/release/gateway-api /usr/local/bin/relayna-gateway

# Keep large request/KDF allocations independently reclaimable instead of
# allowing glibc's dynamic threshold to retain them in fragmented heap arenas.
ENV MALLOC_MMAP_THRESHOLD_=131072 \
    GATEWAY_BIND_ADDR=0.0.0.0:8080 \
    GATEWAY_CONTROL_BIND_ADDR=0.0.0.0:8081 \
    LOG_LEVEL=gateway_api=info,gateway_proxy=info

EXPOSE 8080 8081

USER 10001:10001
ENTRYPOINT ["/usr/local/bin/relayna-gateway"]
