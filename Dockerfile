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

# Includes glibc, libgcc, OpenSSL and CA roots, without distribution utilities.
# Pin the multi-platform index; refresh deliberately and scan every candidate.
FROM gcr.io/distroless/cc-debian13:nonroot@sha256:08476c244888bfb425bae1ae4dd7c17f093e972aab8426883d97f3c6f280f7c1 AS runtime
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
