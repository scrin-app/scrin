# scrin-server image: rendezvous + iroh relay + browser gateway.
# Build from the repository root:  docker build -f deploy/Dockerfile -t scrin-server .
# Multi-stage: cargo-chef caches dependency builds; the runtime is distroless (glibc, nonroot).

ARG RUST_VERSION=1.99

FROM rust:${RUST_VERSION}-bookworm AS chef
RUN cargo install cargo-chef --locked --version ^0.1
WORKDIR /src

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json --bin scrin-server

FROM chef AS builder
COPY --from=planner /src/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json -p scrin-server
COPY . .
RUN cargo build --release --locked -p scrin-server \
 && install -D -m 0755 target/release/scrin-server /out/scrin-server \
 && mkdir -p /out/data

FROM gcr.io/distroless/cc-debian13:nonroot
LABEL org.opencontainers.image.title="scrin-server" \
      org.opencontainers.image.description="scrin rendezvous, relay and WebTransport gateway" \
      org.opencontainers.image.licenses="AGPL-3.0-only" \
      org.opencontainers.image.source="https://github.com/scrin-app/scrin"
COPY --from=builder /out/scrin-server /usr/local/bin/scrin-server
COPY --from=builder --chown=65532:65532 /out/data /data
ENV SCRIN_DATA_DIR=/data \
    SCRIN_DB=/data/scrin.db \
    SCRIN_LISTEN=0.0.0.0:443 \
    SCRIN_WT_LISTEN=0.0.0.0:443
VOLUME ["/data"]
EXPOSE 443/tcp 443/udp 80/tcp 7842/udp
USER nonroot:nonroot
ENTRYPOINT ["/usr/local/bin/scrin-server"]
