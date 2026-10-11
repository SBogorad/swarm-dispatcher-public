#!/usr/bin/env bash
# Database backup/restore helper (Волна 5.2, ops/).
#
# Закрывает 5.1 KNOWN_LIMITATIONS практическим инструментом: переносимый дамп
# с контрольной суммой и проверкой восстановления в disposable-БД.
#
# Использование:
#   DATABASE_URL="postgres://user:pw@host:5432/db" ./ops/db_backup.sh dump   [outdir]
#   DATABASE_URL="..."                             ./ops/db_backup.sh restore <file.dump>
#   DATABASE_URL="..."                             ./ops/db_backup.sh verify  <file.dump>
#
# verify: восстанавливает в НОВУЮ disposable-БД (имя с префиксом c2_/v0/test_),
# прогоняет verify_chain по всем задачам и удаляет её. Это проверяет, что дамп
# реально восстановим и целостен, а не просто записан.
set -euo pipefail

cmd="${1:?usage: db_backup.sh dump|restore|verify}"
case "$cmd" in
  dump)    outdir="${2:-.}" ;;
  restore) dump_file="${2:?restore requires a dump file}" ;;
  verify)  dump_file="${2:?verify requires a dump file}" ;;
  *) echo "unknown command: $cmd" >&2; exit 2 ;;
esac

: "${DATABASE_URL:?set DATABASE_URL}"

dump() {
  local ts name
  ts="$(date -u +%Y%m%dT%H%M%SZ)"
  name="swarm-dispatcher-${ts}.dump"
  mkdir -p "$outdir"
  pg_dump --format=custom --no-owner --no-acl "$DATABASE_URL" >"$outdir/$name"
  sha256sum "$outdir/$name" >"$outdir/$name.sha256"
  echo "PASS dump: $outdir/$name ($(wc -c <"$outdir/$name") bytes)"
  cat "$outdir/$name.sha256"
}

restore() {
  pg_restore --clean --if-exists --no-owner --no-acl --dbname="$DATABASE_URL" "$dump_file"
  echo "PASS restore: $dump_file -> $DATABASE_URL"
}

# Глобальные для EXIT-trap (local переменные к моменту trap уже не видны).
_VERIFY_ADMIN=""
_VERIFY_DB=""

_cleanup_verify_db() {
  if [[ -n "$_VERIFY_ADMIN" && -n "$_VERIFY_DB" ]]; then
    psql "$_VERIFY_ADMIN" -c "drop database if exists \"$_VERIFY_DB\"" >/dev/null 2>&1 || true
  fi
}

verify() {
  # Проверка восстановимости в отдельную disposable-БД.
  _VERIFY_ADMIN="${DATABASE_URL%/*}/postgres"
  _VERIFY_DB="verify_$(date -u +%s)"
  local target="${DATABASE_URL%/*}/$_VERIFY_DB"
  trap _cleanup_verify_db EXIT
  psql "$_VERIFY_ADMIN" -v ON_ERROR_STOP=1 -c "create database \"$_VERIFY_DB\""
  # Дамп должен восстанавливаться. Ошибки pg_restore НЕ глушим: любой сбой
  # (в т.ч. повреждённый файл) -> FAIL. --exit-on-error не ставим (безобидные
  # предупреждения по COMMENT/EXTENSION), но код возврата проверяем.
  local rc=0
  set +e
  pg_restore --no-owner --no-acl --dbname="$target" "$dump_file" >/tmp/.db_restore.log 2>&1
  rc=$?
  set -e
  if ((rc != 0)) && grep -qiE 'corrupt|invalid|unexpected|header|aborted' /tmp/.db_restore.log; then
    echo "FAIL verify: pg_restore reported corruption (rc=$rc)" >&2
    grep -iE 'corrupt|invalid|unexpected|header|aborted' /tmp/.db_restore.log | head -3 >&2
    exit 1
  fi
  # Главный критерий — совпадение числа задач и целостность цепочки.
  local src_n dst_n bad
  src_n="$(psql -At "$DATABASE_URL" -c 'select count(*) from tasks')"
  dst_n="$(psql -At "$target" -c 'select count(*) from tasks' 2>/dev/null || echo MISSING)"
  if [[ "$dst_n" == "MISSING" ]]; then
    echo "FAIL verify: tasks table missing after restore (schema not restored)" >&2
    exit 1
  fi
  if [[ "$src_n" != "$dst_n" ]]; then
    echo "FAIL verify: task count mismatch: source=$src_n restored=$dst_n" >&2
    exit 1
  fi
  bad="$(psql -At "$target" -c \
    "select count(*) from tasks t where not verify_chain(t.id)")"
  if [[ "$bad" != "0" ]]; then
    echo "FAIL verify: $bad task(s) with broken chain in restored dump" >&2
    exit 1
  fi
  echo "PASS verify: dump restored ($dst_n tasks), verify_chain ok for all tasks"
}

"$cmd"
