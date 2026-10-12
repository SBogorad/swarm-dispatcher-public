# Демо-контур «Swarm Ops Console» — НАСТОЯЩЕЕ ядро + окно наблюдения.
#
# Один образ несёт и реальные Rust-бинарники (swarm-planner, swarm-worker,
# swarm-watchdog, swarm-cli), и Python-обвязку (мост/консоль/ревьюер).
# База — ТОТ ЖЕ образ ядра, что в боевом контуре (swarm-core:local), поэтому
# бинари в демо и в бою — одни и те же.
#
# Сборка образа ядра (один раз, вне compose — он тяжёлый):
#   docker build -t swarm-core:local .
#
# Потом:  ./scripts/start_demo.sh
FROM swarm-core:local AS core

FROM python:3.12-slim-trixie

ENV PYTHONUNBUFFERED=1 \
    PYTHONDONTWRITEBYTECODE=1 \
    PIP_NO_CACHE_DIR=1

# libssl3t64/ca-certificates нужны динамически слинкованным Rust-бинарникам
# (та же база trixie, что у образа ядра — иначе "GLIBC not found").
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates libssl3t64 \
    && rm -rf /var/lib/apt/lists/*

# Реальные бинарники движка — из образа ядра, без пересборки.
COPY --from=core /usr/local/bin/swarm-planner  /usr/local/bin/
COPY --from=core /usr/local/bin/swarm-worker   /usr/local/bin/
COPY --from=core /usr/local/bin/swarm-watchdog /usr/local/bin/
COPY --from=core /usr/local/bin/swarm-cli      /usr/local/bin/

WORKDIR /app

COPY demo/requirements.txt /app/requirements.txt
RUN pip install --no-cache-dir -r /app/requirements.txt

COPY demo/ /app/demo/

ENV PYTHONPATH=/app/demo
WORKDIR /app/demo

# По умолчанию — мост (переопределяется в compose).
CMD ["uvicorn", "bridge.api:app", "--host", "0.0.0.0", "--port", "8080"]
