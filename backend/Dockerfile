# syntax=docker/dockerfile:1

FROM rust:1.93-bookworm AS builder

WORKDIR /build

# Build deps (sqlx-sqlite uses libsqlite3-sys which bundles sqlite by default; keep SSL/certs for any HTTPS at runtime).
RUN apt-get update \
  && apt-get install -y --no-install-recommends ca-certificates pkg-config libssl-dev \
  && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY migrations ./migrations

RUN cargo build --release

FROM debian:bookworm-slim

RUN apt-get update \
  && apt-get install -y --no-install-recommends ca-certificates curl \
  && rm -rf /var/lib/apt/lists/*

RUN useradd -m -u 10001 app \
  && mkdir -p /data \
  && chown -R app:app /data

WORKDIR /app
COPY --from=builder /build/target/release/backend /app/backend

USER app

ENV RUST_LOG=info
ENV BIND_ADDR=0.0.0.0:8080
# Use a writable volume location by default.
ENV DATABASE_URL=sqlite:///data/happi.db?mode=rwc

EXPOSE 8080
VOLUME ["/data"]

CMD ["/app/backend"]
