# syntax=docker/dockerfile:1.7

FROM rust:1-bookworm AS builder
WORKDIR /app

RUN apt-get update \
    && apt-get install -y --no-install-recommends pkg-config libssl-dev ca-certificates cmake \
    && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./
COPY crates ./crates

RUN cargo build --release --bin gateway-api

FROM debian:bookworm-slim AS runtime
WORKDIR /app

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --create-home --home-dir /home/relayna relayna

COPY --from=builder /app/target/release/gateway-api /usr/local/bin/relayna-gateway

ENV GATEWAY_BIND_ADDR=0.0.0.0:8080 \
    GATEWAY_CONTROL_BIND_ADDR=0.0.0.0:8081 \
    LOG_LEVEL=gateway_api=info,gateway_proxy=info

EXPOSE 8080 8081

USER relayna
ENTRYPOINT ["/usr/local/bin/relayna-gateway"]
