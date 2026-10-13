#!/usr/bin/env bash
# Одноразовый Postgres для ручных/интеграционных проверок — В ОТДЕЛЬНОМ контейнере.
#
# Зачем этот файл (инцидент 2026-10-13): роли Postgres глобальны для кластера,
# а `step2/roles.sql` и `step2b/roles_2b.sql` делают `alter role ... password`.
# Если поднять scratch-БД ВНУТРИ боевого кластера и применить к ней эти файлы с
# тестовыми паролями (`-v w1_pw=c2_w1`), пароли боевых ролей молча меняются —
# и весь прод-контур падает с `password authentication failed`. Именно так был
# уронен прод. Правильный путь — scratch-контейнер: роли в нём изолированы.
#
# Использование:
#   scripts/scratch_pg.sh start [db]     # поднять контейнер + применить схему/роли/миграции
#   scripts/scratch_pg.sh dsn  [db]      # напечатать DSN (host/port/user/password/dbname)
#   scripts/scratch_pg.sh psql [db] [-- args...]  # psql внутрь scratch-контейнера
#   scripts/scratch_pg.sh stop [db]      # удалить контейнер
#
# Пароли ролей внутри scratch-контейнера — фиксированные `c2_*`; они НЕ имеют
# отношения к боевым (другой кластер). Боевой .env этот скрипт не читает.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ACTION="${1:-start}"
DB="${2:-scratch}"
DB_NAME="${DB:-scratch}"
DB_NAME="${DB_NAME//[^a-zA-Z0-9_]/_}"
CONTAINER="swarm_scratch_pg_${DB_NAME}"
# Фиксированные пароли ТОЛЬКО для изолированного scratch-кластера.
ADMIN_PW="c2_admin"
W1_PW="c2_w1"; W2_PW="c2_w2"; W_1_PW="c2_w_1"; GPU_1_PW="c2_gpu_1"
WORKER_PW="c2_worker"; WATCHDOG_PW="c2_watchdog"; PLANNER_PW="c2_planner"
REVIEWER_PW="c2_reviewer"; OPERATOR_PW="c2_operator"; CLI_PW="c2_cli"

die() { echo "ERROR: $*" >&2; exit 1; }

pg_exec() { docker exec -i -e PGPASSWORD="$ADMIN_PW" "$CONTAINER" psql -X -v ON_ERROR_STOP=1 -U postgres -d "$DB_NAME" "$@"; }

port() { docker port "$CONTAINER" 5432/tcp 2>/dev/null | awk -F: 'NR==1{print $NF}'; }

case "$ACTION" in
  start)
    command -v docker >/dev/null || die "docker не найден"
    if docker inspect "$CONTAINER" >/dev/null 2>&1; then
      die "контейнер $CONTAINER уже существует — сначала: scripts/scratch_pg.sh stop $DB"
    fi
    docker run -d --name "$CONTAINER" \
      -e POSTGRES_DB="$DB_NAME" -e POSTGRES_USER=postgres -e "POSTGRES_PASSWORD=$ADMIN_PW" \
      -p 127.0.0.1::5432 postgres:16-alpine >/dev/null
    local_ready=0
    for _ in $(seq 1 60); do
      if docker exec -e PGPASSWORD="$ADMIN_PW" "$CONTAINER" psql -X -U postgres -d "$DB_NAME" -c 'select 1' >/dev/null 2>&1; then
        local_ready=1; break
      fi
      sleep 1
    done
    ((local_ready == 1)) || die "postgres не поднялся в $CONTAINER"
    # Порядок ровно как в боевой цепочке (compose initdb.d).
    pg_exec < "$REPO_ROOT/schema/schema_v0.sql" >/dev/null
    pg_exec -v worker_pw="$WORKER_PW" -v watchdog_pw="$WATCHDOG_PW" -v planner_pw="$PLANNER_PW" \
      -v reviewer_pw="$REVIEWER_PW" -v operator_pw="$OPERATOR_PW" -v cli_pw="$CLI_PW" \
      < "$REPO_ROOT/step2/roles.sql" >/dev/null
    pg_exec -v w1_pw="$W1_PW" -v w2_pw="$W2_PW" -v w_1_pw="$W_1_PW" -v gpu_1_pw="$GPU_1_PW" \
      < "$REPO_ROOT/step2b/roles_2b.sql" >/dev/null
    for p in roles_patch_planner_status.sql roles_patch_tool_call_audit.sql; do
      [ -f "$REPO_ROOT/step2/$p" ] && pg_exec < "$REPO_ROOT/step2/$p" >/dev/null
    done
    for mig in "$REPO_ROOT"/schema/migrations/*.sql; do
      [ -e "$mig" ] || continue
      pg_exec < "$mig" >/dev/null
    done
    echo "scratch postgres готов: container=$CONTAINER db=$DB_NAME port=$(port)"
    echo "DSN: $(PORT="$(port)" bash -c 'printf "host=127.0.0.1 port=%s user=postgres password=%s dbname=%s" "$PORT" "c2_admin" "'"$DB_NAME"'"')"
    ;;
  dsn)
    P="$(port)"; [ -n "$P" ] || die "контейнер $CONTAINER не запущен"
    printf 'host=127.0.0.1 port=%s user=postgres password=%s dbname=%s\n' "$P" "$ADMIN_PW" "$DB_NAME"
    ;;
  psql)
    shift 2 || true
    pg_exec "$@"
    ;;
  stop)
    docker rm -f "$CONTAINER" >/dev/null 2>&1 && echo "удалён $CONTAINER" || echo "не найден $CONTAINER"
    ;;
  *)
    die "неизвестное действие: $ACTION (start|dsn|psql|stop)"
    ;;
esac
