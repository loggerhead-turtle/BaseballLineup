# ---- build stage ----
FROM rust:1-bookworm AS builder
WORKDIR /app

# Build against the committed lockfile.
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY migrations ./migrations
RUN cargo build --release

# ---- runtime stage ----
FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app

COPY --from=builder /app/target/release/baseball-lineup /app/baseball-lineup
# Static front end is served from ./static at runtime.
COPY static ./static

# The DB file and uploaded logos live on the mounted volume at /data.
ENV BIND_ADDR=0.0.0.0:8080 \
    DATABASE_URL=/data/lineup.db \
    UPLOADS_DIR=/data/uploads \
    STATIC_DIR=/app/static \
    RUST_LOG=info,tower_http=info

EXPOSE 8080
CMD ["/app/baseball-lineup"]
