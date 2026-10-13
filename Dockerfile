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
# База СОВПАДАЕТ с builder (trixie): иначе динамически слинкованный бинарь
# требует GLIBC из trixie (2.38/2.39+), а bookworm-slim даёт только 2.36 —
# контейнер падает с "version `GLIBC_2.38' not found".
FROM debian:trixie-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates libssl3t64 \
    && rm -rf /var/lib/apt/lists/* \
    && useradd -u 1001 -m -s /bin/bash swarm \
    && mkdir -p /var/swarm/artifacts \
    && chown -R 1001:1001 /var/swarm

USER 1001
WORKDIR /app

COPY --from=builder /build/target/release/swarm-watchdog /usr/local/bin/
COPY --from=builder /build/target/release/swarm-worker   /usr/local/bin/
COPY --from=builder /build/target/release/swarm-cli      /usr/local/bin/
COPY --from=builder /build/target/release/swarm-planner  /usr/local/bin/
COPY --from=builder /build/target/release/swarm-reviewer /usr/local/bin/

# F1: /var/swarm/artifacts уже создан выше (владелец 1001), поэтому именованный
# volume, смонтированный сюда впервые, наследует uid 1001. Без этого volume
# создаётся root-owned, и worker (USER 1001) не может писать артефакт
# (tmpfs раньше давал uid=1001 через опции монтирования — у volume их нет).

# Для worker: изоляция
# cap_drop, read_only, no-new-privileges задаются в compose
CMD ["swarm-watchdog"]
