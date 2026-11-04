# syntax=docker/dockerfile:1

# Build the headless server and the browser client, then ship only the runtime
# pieces. The server is built without the TUI feature: a container has no
# terminal, and this keeps crossterm/ratatui out of the image.

ARG RUST_VERSION=1.99
ARG DEBIAN_RELEASE=bookworm

FROM rust:${RUST_VERSION}-${DEBIAN_RELEASE} AS builder

WORKDIR /app

RUN rustup target add wasm32-unknown-unknown \
    && cargo install trunk --locked

COPY . .

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/root/.cache/trunk \
    --mount=type=cache,target=/app/web/target \
    cd web && trunk build --release

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/app/target \
    cargo build --locked --release -p localtt-server --bin localtt --no-default-features \
    && strip target/release/localtt \
    && cp target/release/localtt /usr/local/bin/localtt

FROM debian:${DEBIAN_RELEASE}-slim AS runtime

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --create-home --home-dir /data \
        --shell /usr/sbin/nologin localtt

COPY --from=builder /usr/local/bin/localtt /usr/local/bin/localtt

# The adapter resolves its shipped SRD snapshot from the compile-time manifest
# directory, so the data has to keep that exact path in the runtime image.
COPY --from=builder /app/crates/localtt-adapter/data /app/crates/localtt-adapter/data

COPY --from=builder /app/web/dist /usr/local/share/localtt/web
COPY docker-entrypoint.sh /usr/local/bin/docker-entrypoint.sh

RUN chmod 0755 /usr/local/bin/docker-entrypoint.sh

VOLUME ["/data"]
EXPOSE 8787
USER localtt

ENV LOCALTT_DATA_DIR=/data \
    LOCALTT_WEB_DIR=/usr/local/share/localtt/web \
    LOCALTT_BIND=0.0.0.0:8787

ENTRYPOINT ["/usr/local/bin/docker-entrypoint.sh"]
