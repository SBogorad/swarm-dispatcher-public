#!/usr/bin/env bash
# F1 [CRITICAL] — артефакты должны быть видимы потребителю (общий volume), а не
# лежать в per-container tmpfs.
#
# Arbiter (по ТЗ WAVE4, F1):
#   1. поднять стек (postgres + worker-1/worker-2 с общим volume);
#   2. довести задачу до done (воркер пишет артефакт);
#   3. перезапустить воркер (docker restart);
#   4. прочитать артефакт по storage_path ИЗ ДРУГОГО контейнера (cli);
#   5. сверить содержимое с хэшем/записью в artifacts (не только «файл есть»).
#
# RED на per-container tmpfs: файл worker-1 не виден в cli.
# GREEN на общем volume swarm-artifacts.
#
# Запуск:  bash tests/test_f1_artifact_visibility.sh
set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

COMPOSE="docker compose -f docker-compose.yml"
PROJECT="f1arb"
COMPOSE_PROJECT_NAME="$PROJECT"

# Отдельные пароли (случайные; не печатаем).
export DB_PASSWORD="$(openssl rand -hex 16)"
for v in W1 W2 W_1 GPU_1 WORKER WATCHDOG PLANNER REVIEWER OPERATOR CLI; do
  export "SWARM_PW_$v=$(openssl rand -hex 16)"
done

log()  { echo "[f1] $*"; }
fail() { echo "[f1] FAIL: $*" >&2; cleanup; exit 1; }
cleanup() {
  $COMPOSE down -v --remove-orphans >/dev/null 2>&1 || true
}
trap cleanup EXIT

log "bringing up stack (project=$PROJECT)"
# F1: именованный volume должен быть создан заново, чтобы унаследовать
# владельца uid 1001 из образа (иначе root-owned -> Permission denied).
$COMPOSE down -v --remove-orphans >/dev/null 2>&1 || true
$COMPOSE up -d postgres >/dev/null 2>&1 || fail "compose up postgres"
# Ждём готовности схемы: initdb-скрипты (schema + roles + patches) выполняются
# после старта сервера, поэтому проверяем наличие submit_review с ретраями.
SCHEMA_OK=0
for i in $(seq 1 90); do
  if $COMPOSE exec -T postgres psql -U swarm -d swarm -tAc \
       "select count(*) from pg_proc where proname='submit_review'" 2>/dev/null | grep -q '^1$'; then
    SCHEMA_OK=1; break
  fi
  sleep 1
done
[ "$SCHEMA_OK" = "1" ] || fail "postgres not ready / schema missing"

# Поднимаем воркера, который реально завершит задачу. worker-2 по умолчанию
# с CRASH_AFTER_CLAIM=1 выходит(1) после claim (фикстура crash-recovery),
# поэтому для видимости артефакта достаточно worker-1.
# Образ cli тоже собираем заранее — иначе `compose run cli` печатает логи сборки
# в stdout и ломает парсинг хэша.
$COMPOSE build worker-1 cli >/dev/null 2>&1 || fail "build worker-1/cli"
$COMPOSE up -d worker-1 >/dev/null 2>&1 || fail "compose up worker-1"

# 2. Довести задачу до done. Планировщик в тестах — mock; но проще вставить
#    задачу напрямую и дать воркеру её выполнить.
TASK=$($COMPOSE exec -T postgres psql -U swarm -d swarm -tAc \
  "select gen_random_uuid()")
log "task=$TASK"
$COMPOSE exec -T postgres psql -U swarm -d swarm -v ON_ERROR_STOP=1 -c \
  "select add_task('$TASK', null, '{\"kind\":\"code.generate\",\"checks\":[{\"type\":\"cmd\",\"run\":\"true\"}],\"budget\":{\"attempts\":3,\"max_rework\":2}}'::jsonb, 100000, 0);
   select register_worker('w1', '{\"ram_gb\":16}'::jsonb);
   select promote_ready();" >/dev/null 2>&1 || fail "seed task"

# Ждём завершения (воркер: WORK_SECONDS=5, LEASE_SEC=10; mock LLM).
STATE=""
for i in $(seq 1 60); do
  STATE=$($COMPOSE exec -T postgres psql -U swarm -d swarm -tAc \
    "select state from tasks where id='$TASK'")
  [ "$STATE" = "review" ] || [ "$STATE" = "done" ] && break
  sleep 2
done
log "state after worker: $STATE"
[ "$STATE" = "review" ] || [ "$STATE" = "done" ] || fail "task did not progress (state=$STATE)"

# Запись артефакта: storage_path + sha256 ref.
read -r REF STORAGE <<<"$($COMPOSE exec -T postgres psql -U swarm -d swarm -tAc \
  "select ref || ' ' || storage_path from artifacts where task_id='$TASK' limit 1")"
log "ref=$REF"
log "storage_path=$STORAGE"
[ -n "${REF:-}" ] || fail "no artifact row for task"
case "$STORAGE" in
  volume://*|s3://*|http*) : ;;
  *) fail "storage_path is not a consumer-readable URI: $STORAGE" ;;
esac

# 3. Перезапуск воркера (артефакт не должен исчезнуть/стать недоступным).
$COMPOSE restart worker-1 >/dev/null 2>&1 || fail "restart worker-1"
sleep 2

# 4. Прочитать артефакт ИЗ ДРУГОГО контейнера (cli) и сверить хэш.
#    Разрешаем volume:// URI в путь внутри общего volume.
REL="${STORAGE#volume://}"          # <vol>/<relpath>
REL="${REL#*/}"                     # <relpath> внутри ARTIFACTS_DIR
FILECHK=$($COMPOSE run --rm -T --no-deps cli sh -c \
  "if [ -f /var/swarm/artifacts/'$REL' ]; then sha256sum /var/swarm/artifacts/'$REL' | cut -d' ' -f1; else echo MISSING; fi" \
  2>/dev/null | tr -d '\r' | tail -1)
log "sha256 read from cli container: $FILECHK"
[ "$FILECHK" != "MISSING" ] || fail "artifact not visible from another container (per-container tmpfs?)"
EXPECT="${REF#sha256:}"
[ "$FILECHK" = "$EXPECT" ] || fail "content hash mismatch: got $FILECHK want $EXPECT"

log "PASS: artifact visible across containers and content matches ref"
cleanup
trap - EXIT
