# syntax=docker/dockerfile:1.6

FROM rust:1-slim-bookworm AS builder
WORKDIR /build
COPY Cargo.toml ./
COPY src ./src
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/build/target \
    cargo build --release \
 && cp target/release/tbk-custom-ranks-bridge /tbk-custom-ranks-bridge

FROM debian:bookworm-slim AS runtime
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates tini \
 && rm -rf /var/lib/apt/lists/* \
 && useradd --system --uid 10001 --home-dir /app --create-home tbk \
 && mkdir -p /data \
 && chown tbk:tbk /data /app
COPY --from=builder /tbk-custom-ranks-bridge /usr/local/bin/tbk-custom-ranks-bridge
WORKDIR /app
USER tbk
EXPOSE 8787
ENTRYPOINT ["/usr/bin/tini", "--"]
CMD ["tbk-custom-ranks-bridge", "--config", "/app/config.toml"]
