#!/bin/sh
# 02-roles.sh — применяет roles.sql после 01-schema.sql.
#
# Запускается официальным образом postgres из /docker-entrypoint-initdb.d/
# только при пустом каталоге данных (первая инициализация).
# Пароли ролей берутся из env (SWARM_PW_*), заданы в docker-compose.yml.
set -eu

: "${POSTGRES_USER:?POSTGRES_USER required}"
: "${POSTGRES_DB:?POSTGRES_DB required}"

psql -v ON_ERROR_STOP=1 \
     -U "$POSTGRES_USER" -d "$POSTGRES_DB" \
     -v worker_pw="$SWARM_PW_WORKER" \
     -v watchdog_pw="$SWARM_PW_WATCHDOG" \
     -v planner_pw="$SWARM_PW_PLANNER" \
     -v reviewer_pw="$SWARM_PW_REVIEWER" \
     -v operator_pw="$SWARM_PW_OPERATOR" \
     -v cli_pw="$SWARM_PW_CLI" \
     -f /sql/roles.sql
