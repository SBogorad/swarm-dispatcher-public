# sandbox/Dockerfile — образ песочницы для shell tools (ТЗ v0.4 §3).
#
# Один образ — много контейнеров: `docker run ... agent-sandbox:1.85 sleep infinity`.
# Изоляция задаётся флагами запуска (см. crates/swarm-worker/src/sandbox.rs):
#   --cap-drop ALL --read-only --network none --security-opt no-new-privileges:true
#   --tmpfs /workspace --memory 2g --cpus 2 -u 1001:1001
#
# Сборка:  docker build -t agent-sandbox:1.85 sandbox/

FROM ubuntu:24.04

ENV DEBIAN_FRONTEND=noninteractive

# Минимальный набор инструментов. curl — только на время установки rustup,
# затем удаляется (его нет в allowlist и он не нужен в песочнице).
RUN apt-get update && apt-get install -y --no-install-recommends \
        bash coreutils git ca-certificates build-essential curl \
    && rm -rf /var/lib/apt/lists/*

# Непривилегированный пользователь (uid совпадает с -u 1001:1001 в worker).
RUN useradd -u 1001 -m -s /bin/bash swarm

# Rust toolchain ставим ОТ ИМЕНИ swarm — иначе /root/.cargo недоступен пользователю 1001.
USER 1001
ENV HOME=/home/swarm
ENV RUSTUP_HOME=/home/swarm/.rustup
ENV CARGO_HOME=/home/swarm/.cargo
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
      | sh -s -- -y --default-toolchain 1.85 --profile minimal \
    && /home/swarm/.cargo/bin/cargo --version \
    && /home/swarm/.cargo/bin/rustc --version
ENV PATH="/home/swarm/.cargo/bin:${PATH}"

# curl больше не нужен (и не должен быть доступен из песочницы).
USER root
RUN apt-get purge -y curl && apt-get autoremove -y \
    && rm -rf /var/lib/apt/lists/*
USER 1001

# Корень образа read-only, поэтому кэш/сборка должны жить в writable tmpfs.
# Воркер переопределяет эти переменные на /workspace (см. build_run_args).
ENV CARGO_TARGET_DIR=/workspace/target

WORKDIR /workspace
CMD ["sleep", "infinity"]
