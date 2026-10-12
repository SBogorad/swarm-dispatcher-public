#!/bin/sh
# 05-migrations.sh — применяет schema/migrations/*.sql после ролей.
#
# Запускается официальным образом postgres из /docker-entrypoint-initdb.d/
# только при пустой инициализации данных. Порядок — лексикографический по имени
# файла (001_, 002_, ...).
#
# Раньше миграции (001_llm_calls.sql) применялись только вручную, поэтому на
# свежей БД таблица llm_calls отсутствовала, а workers молча теряли метрики
# (Issue #9, #11). Теперь они применяются автоматически при первом старте.
set -eu

: "${POSTGRES_USER:?POSTGRES_USER required}"
: "${POSTGRES_DB:?POSTGRES_DB required}"

for f in /sql/migrations/*.sql; do
  [ -e "$f" ] || continue
  echo "[migrations] applying $(basename "$f")"
  psql -v ON_ERROR_STOP=1 \
       -U "$POSTGRES_USER" -d "$POSTGRES_DB" \
       -f "$f"
done
