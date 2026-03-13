# syntax=docker/dockerfile:1.7

FROM rust:1.93-bookworm AS builder

WORKDIR /build

# Build deps (sqlx-sqlite uses libsqlite3-sys which bundles sqlite by default; keep SSL/certs for any HTTPS at runtime).
RUN apt-get update \
  && apt-get install -y --no-install-recommends ca-certificates pkg-config libssl-dev \
  && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./

RUN --mount=type=cache,target=/usr/local/cargo/registry \
  --mount=type=cache,target=/usr/local/cargo/git \
  mkdir -p src \
  && printf '%s\n' 'fn main() {}' > src/main.rs \
  && cargo fetch --locked \
  && rm -rf src

COPY src ./src
COPY migrations ./migrations

RUN --mount=type=cache,target=/usr/local/cargo/registry \
  --mount=type=cache,target=/usr/local/cargo/git \
  --mount=type=cache,target=/build/target \
  cargo build --release \
  && cp /build/target/release/backend /build/backend

FROM debian:bookworm-slim

RUN apt-get update \
  && apt-get install -y --no-install-recommends ca-certificates curl \
  && rm -rf /var/lib/apt/lists/*

RUN useradd -m -u 10001 app \
  && mkdir -p /data \
  && chown -R app:app /data

WORKDIR /app
COPY --from=builder /build/backend /app/backend

USER app

ENV RUST_LOG=info
ENV BIND_ADDR=0.0.0.0:8080
# Use a writable volume location by default.
ENV DATABASE_URL=sqlite:///data/happi.db?mode=rwc

EXPOSE 8080
VOLUME ["/data"]

CMD ["/app/backend"]
