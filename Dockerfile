# Stage 1: builder
# ВНИМАНИЕ: образ поднят с 1.80 до 1.98.
# Причина: транзитивная зависимость sha2 0.11 (через postgres-protocol 0.6.12 ->
# digest 0.11 -> block-buffer 0.12) требует Cargo feature `edition2024`,
# которая стабилизирована только с Rust 1.85. На 1.80 сборка падает с ошибкой
# "feature `edition2024` is required". 1.98 соответствует локально проверенному toolchain.
FROM rust:1.98-slim AS builder

WORKDIR /build
COPY Cargo.toml Cargo.lock* ./
COPY crates ./crates
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/* \
    && cargo build --release --locked --bins

# Stage 2: runtime
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates libssl3 \
    && rm -rf /var/lib/apt/lists/* \
    && useradd -u 1001 -m -s /bin/bash swarm

USER 1001
WORKDIR /app

COPY --from=builder /build/target/release/swarm-watchdog /usr/local/bin/
COPY --from=builder /build/target/release/swarm-worker   /usr/local/bin/
COPY --from=builder /build/target/release/swarm-cli      /usr/local/bin/
COPY --from=builder /build/target/release/swarm-planner  /usr/local/bin/

# Для worker: изоляция
# cap_drop, read_only, no-new-privileges задаются в compose
CMD ["swarm-watchdog"]
