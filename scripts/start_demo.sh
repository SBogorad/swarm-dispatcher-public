#!/usr/bin/env bash
# Запуск демонстрационного контура «Swarm Ops Console» одной командой.
#
#   ./scripts/start_demo.sh            # поднять и открыть браузер
#   ./scripts/start_demo.sh --no-open  # без открытия браузера
#   ./scripts/start_demo.sh --down     # остановить и удалить (включая БД)
#
# В контуре работают НАСТОЯЩИЕ бинарники движка (swarm-planner, swarm-worker,
# swarm-watchdog) из образа ядра `swarm-core:local`. Python — только витрина
# (мост, консоль, ревьюер).
#
# Боевой docker-compose.yml не затрагивается: отдельный docker-compose.demo.yml.
set -euo pipefail

cd "$(dirname "$0")/.."

COMPOSE_FILE="docker-compose.demo.yml"
CORE_IMAGE="swarm-core:local"
UI_PORT="${DEMO_UI_PORT:-8501}"
API_PORT="${DEMO_API_PORT:-8080}"
OPEN=1

for arg in "$@"; do
  case "$arg" in
    --no-open) OPEN=0 ;;
    --down)
      docker compose -f "$COMPOSE_FILE" down -v
      echo "[demo] стенд остановлен"
      exit 0
      ;;
    -h|--help)
      grep '^#' "$0" | sed 's/^# \{0,1\}//'
      exit 0
      ;;
  esac
done

if ! docker info >/dev/null 2>&1; then
  echo "ОШИБКА: docker недоступен (демон не запущен или нет прав)" >&2
  exit 1
fi

# Образ ядра с настоящими бинарниками. Собирается один раз (тяжёлый).
if ! docker image inspect "$CORE_IMAGE" >/dev/null 2>&1; then
  echo "[demo] образ ядра $CORE_IMAGE не найден — собираю из исходников…"
  docker build -t "$CORE_IMAGE" .
fi

echo "[demo] сборка образа контура (Python-витрина + бинарники ядра)…"
docker compose -f "$COMPOSE_FILE" build demo-bridge

echo "[demo] запуск (postgres + сторож + 3 воркера + ревьюер + мост + консоль)…"
docker compose -f "$COMPOSE_FILE" up -d postgres watchdog \
  worker-1 worker-2 worker-3 demo-reviewer demo-bridge demo-ui

echo "[demo] ждём готовности моста…"
for _ in $(seq 1 90); do
  if curl -fsS "http://127.0.0.1:${API_PORT}/healthz" >/dev/null 2>&1; then
    break
  fi
  sleep 1
done

echo
echo "  ┌───────────────────────────────────────────────────────────┐"
echo "  │  Swarm Ops Console — координация агентов                  │"
echo "  │  Консоль: http://localhost:${UI_PORT}                          │"
echo "  │  Мост:    http://localhost:${API_PORT}/healthz                 │"
echo "  │  Остановка: ./scripts/start_demo.sh --down                │"
echo "  └───────────────────────────────────────────────────────────┘"
echo
echo "  Введите цель и нажмите «Запустить рой»: настоящий swarm-planner"
echo "  разложит её в граф задач (LLM), настоящие swarm-worker разберут"
echo "  их по арендам, а консоль покажет канбан, узлов роя и неизменяемый"
echo "  журнал (хэш-цепочку)."

if [[ "$OPEN" == "1" ]]; then
  URL="http://localhost:${UI_PORT}"
  if command -v xdg-open >/dev/null 2>&1; then
    xdg-open "$URL" >/dev/null 2>&1 || true
  elif command -v open >/dev/null 2>&1; then
    open "$URL" >/dev/null 2>&1 || true
  fi
fi
