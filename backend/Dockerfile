FROM rust:1.85-bookworm AS builder
WORKDIR /app
COPY Cargo.toml Cargo.toml
COPY src src
COPY migrations migrations
RUN cargo build --release --bin live-tron-api --bin migrate

FROM debian:bookworm-slim AS runtime
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --create-home app
WORKDIR /app
COPY --from=builder /app/target/release/live-tron-api /usr/local/bin/live-tron-api
COPY --from=builder /app/target/release/migrate /usr/local/bin/live-tron-migrate
COPY --from=builder /app/migrations ./migrations
USER app
EXPOSE 8080
ENV LIVE_TRON_BIND=0.0.0.0:8080
ENTRYPOINT ["live-tron-api"]
