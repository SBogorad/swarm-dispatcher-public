#!/usr/bin/env bash
# run_mutations_sql.sh — мутационное тестирование SQL-инвариантов ядра.
#
# Волна 5.2: M1–M6 покрывали только Rust (allowlist/sandbox/agent_loop). Но ядро
# проекта — SQL-функции, и их защитные инварианты (fencing, self-review, cycle,
# идемпотентность событий) мутационно НЕ проверялись. Этот скрипт закрывает
# слепую зону: каждая мутация ломает ровно одну SQL-защиту, ожидается, что
# соответствующий Python-арбитр ПАДАЕТ. Выживший мутант = дыра в покрытии.
#
# Использование:
#   TEST_DATABASE_URL="host=... port=... user=... password=... dbname=v0..." \
#     ./scripts/run_mutations_sql.sh
#
# Скрипт сам восстанавливает схему (backup + trap) и падает (exit != 0) при
# выжившем мутанте.

set -uo pipefail
cd "$(dirname "$0")/.." || exit 1

SCHEMA="schema/schema_v0.sql"
: "${TEST_DATABASE_URL:?set TEST_DATABASE_URL (disposable DB, prefix test_/v0/c2_)}"

# Разбор DSN в psql-совместимый набор переменных.
pg_psql() {
  # $@ — аргументы psql; DSN берём из окружения.
  psql "$TEST_DATABASE_URL" -X -q -v ON_ERROR_STOP=1 "$@"
}

# Pre-condition: БД — тестовая (не снесём рабочую), схема применяется.
case "$TEST_DATABASE_URL" in
  *password=*dbname=*) : ;;
esac
if [[ "$TEST_DATABASE_URL" != *"dbname=v0"* && "$TEST_DATABASE_URL" != *"dbname=test_"* && "$TEST_DATABASE_URL" != *"dbname=c2_"* ]]; then
  echo "BLOCKED: refuse non-disposable database (need dbname=v0*/test_*/c2_*): $TEST_DATABASE_URL" >&2
  exit 2
fi
if ! psql "$TEST_DATABASE_URL" -X -Atc "select 1" >/dev/null 2>&1; then
  echo "BLOCKED: cannot connect to TEST_DATABASE_URL" >&2
  exit 2
fi

# Python-арбитры, которые должны ловить SQL-мутации.
PY="$(pwd)/.venv/bin/python"
[[ -x "$PY" ]] || { echo "BLOCKED: .venv/bin/python not found" >&2; exit 2; }

# Бэкап схемы в git-независимом месте; восстановление вызывается многократно.
BACKUP="${TMPDIR:-/tmp}/swarm_schema_backup_$$.sql"
cp "$SCHEMA" "$BACKUP"
restore() { cp -f "$BACKUP" "$SCHEMA"; }
cleanup() { restore; rm -f "$BACKUP"; }
trap cleanup EXIT INT TERM

caught=0; survived=0
RESULTS=()

# Арбитры (Python-наборы). Каждый прогоняется на живой БД с PGDSN/W4_*.
export PGDSN="$TEST_DATABASE_URL"
export W4_ADMIN_DSN="${W4_ADMIN_DSN:-$TEST_DATABASE_URL}"
export W4_W1_PW="${W4_W1_PW:-c2_w1}" W4_W2_PW="${W4_W2_PW:-c2_w2}"
export SWARM_PW_W1=${SWARM_PW_W1:-c2_w1} SWARM_PW_W2=${SWARM_PW_W2:-c2_w2}

run_arbiters() {
  # Набор арбитров покрывает именно те инварианты, которые мутируем:
  # test_v0.py — FSM/fencing/idempotency/cycle; review-gate — self-review;
  # fail_unblock — fail/unblock. Всё это — функции схемы (не RBAC).
  "$PY" -m pytest -q tests/test_v0.py tests/test_review_gate_f2.py \
        tests/test_fail_unblock_f4.py >/dev/null 2>&1
}

# Чистое пересоздание схемы в disposable-БД.
#
# SQL-мутации проверяют ФУНКЦИИ СХЕМЫ (assert_lease/submit_review/add_dep/
# append_event/verify_chain), а не RBAC — поэтому роли не нужны. Достаточно
# пересоздать public и налить schema. `task_status` (из step2/roles.sql) не входит
# в набор арбитров ниже, поэтому зависимость от psql-переменных ролей убрана.
reset_db() {
  psql "$TEST_DATABASE_URL" -X -q -v ON_ERROR_STOP=1 \
    -c 'drop schema public cascade; create schema public;' >/dev/null 2>&1 || return 1
  psql "$TEST_DATABASE_URL" -X -q -v ON_ERROR_STOP=1 -f "$SCHEMA" >/dev/null 2>&1 || return 1
  return 0
}

# Арбитр до мутации должен быть зелёным (иначе мутация неинформативна).
# Важно: reset_db() пересоздаёт public вместе с roles — иначе task_status пропадёт.
if ! reset_db; then
  echo "BLOCKED: cannot reset schema (drop public + reload)" >&2
  exit 2
fi
if ! run_arbiters; then
  echo "BLOCKED: baseline arbiters are not green; fix before mutating" >&2
  exit 2
fi
echo "pre-conditions OK: db=$TEST_DATABASE_URL baseline=green"

# Чистое пересоздание схемы в disposable-БД уже определено выше (reset_db).

run_mutation() {
  local id="$1" desc="$2" old_str="$3" new_str="$4"
  restore               # чистое исходное состояние схемы
  # Замена через python (надёжно с любыми символами, в т.ч. '|' и кавычками).
  if ! python3 - "$SCHEMA" "$old_str" "$new_str" <<'PY'
import sys
path, old, new = sys.argv[1], sys.argv[2], sys.argv[3]
s = open(path, encoding="utf-8").read()
if old not in s:
    sys.exit(3)
s = s.replace(old, new, 1)
open(path, "w", encoding="utf-8").write(s)
PY
  then
    RESULTS+=("$id  INVALID   $desc  (паттерн мутации не найден в схеме)")
    restore; return
  fi
  # Чистое пересоздание схемы с мутантом.
  if ! reset_db; then
    RESULTS+=("$id  CAUGHT    $desc  (мутант сломал DDL)")
    caught=$((caught + 1)); restore; return
  fi
  if run_arbiters; then
    RESULTS+=("$id  SURVIVED  $desc  (арбитры НЕ заметили)")
    survived=$((survived + 1))
  else
    RESULTS+=("$id  CAUGHT    $desc  (арбитры упали, как ожидалось)")
    caught=$((caught + 1))
  fi
  restore
}

# S1 — assert_lease: убрать выбрасывание lease_lost (fencing становится no-op).
run_mutation S1 "assert_lease: снять fenced-отказ (lease_lost)" \
  "raise exception 'lease_lost' using errcode = 'LS001';" \
  "null;"

# S2 — submit_review: убрать запрет self-review.
run_mutation S2 "submit_review: разрешить self-review" \
  "if v_creator = p_reviewer then raise exception 'self_review' using errcode = 'RV002'; end if;" \
  "if false then raise exception 'self_review' using errcode = 'RV002'; end if;"

# S3 — add_dep: убрать проверку self-dependency (cycle).
run_mutation S3 "add_dep: снять проверку self-dependency (cycle)" \
  "if p_task = p_dep then raise exception 'cycle_detected' using errcode = 'GR001'; end if;" \
  "if false then raise exception 'cycle_detected' using errcode = 'GR001'; end if;"

# S4 — append_event: убрать идемпотентность по event_id.
run_mutation S4 "append_event: снять идемпотентность по event_id" \
  "if exists (select 1 from events where task_id = p_task and event_id = p_event_id) then" \
  "if false then"

# S5 — verify_chain: ослепить детектор подделки (prev_hash-ветка).
run_mutation S5 "verify_chain: не проверять связь prev_hash" \
  "if e.prev_hash is distinct from v_prev then return false; end if;" \
  "if false then return false; end if;"

echo
echo "==================== РЕЗУЛЬТАТЫ SQL-МУТАЦИЙ S1–S5 ===================="
for r in "${RESULTS[@]}"; do echo "$r"; done
echo "----------------------------------------------------------------"
echo "поймано: $caught / $((caught + survived))   выжило: $survived"
echo

[ "$survived" -eq 0 ]
