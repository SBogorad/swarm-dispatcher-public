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
#
# Реестр применённого (public.schema_migrations) пишется здесь и читается
# scripts/apply_migrations.sh — раннером для ЖИВОГО тома (initdb-скрипты
# выполняются только на пустом томе, поэтому новые миграции на существующий том
# так не попадают, §5.3). Формат реестра у двух скриптов ОДИН: дрейф формы ловит
# tests/test_migration_ledger_runner.py.
set -eu

: "${POSTGRES_USER:?POSTGRES_USER required}"
: "${POSTGRES_DB:?POSTGRES_DB required}"

# Каталог миграций. В initdb-пути это смонтированный /sql/migrations; переопределение
# нужно стражам, которые прогоняют ЭТОТ ЖЕ скрипт на живой disposable-БД, а не
# повторяют его логику у себя (tests/test_schema_chain_completeness.py).
MIGRATIONS_DIR="${MIGRATIONS_DIR:-/sql/migrations}"

psql_q() { psql -X -q -v ON_ERROR_STOP=1 -U "$POSTGRES_USER" -d "$POSTGRES_DB" "$@"; }

psql_q <<'SQL'
create table if not exists public.schema_migrations (
  filename   text primary key,
  checksum   text not null,
  applied_at timestamptz not null default now(),
  applied_by text not null default session_user
);
do $$ begin
  -- Инвариант каталога (step2/test_roles.py::test_catalog_invariants): таблицы
  -- в public принадлежат не суперпользователю, а swarm_owner.
  if exists (select 1 from pg_roles where rolname = 'swarm_owner') then
    execute 'alter table public.schema_migrations owner to swarm_owner';
  end if;
end $$;
revoke all on table public.schema_migrations from public;
SQL

for f in "$MIGRATIONS_DIR"/*.sql; do
  [ -e "$f" ] || continue
  name="$(basename "$f")"
  sum="$(sha256sum "$f" | cut -d' ' -f1)"
  echo "[migrations] applying $name"
  psql_q -f "$f"
  psql_q -c "insert into public.schema_migrations (filename, checksum, applied_by)
             values ('$name', '$sum', session_user)
             on conflict (filename) do nothing"
done
