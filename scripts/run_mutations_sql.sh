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
# F8: мутации S6/S7 целятся в тело триггера/reclaim в миграции 011 (она
# перекрывает schema_v0.sql). Файл обязан быть под тем же backup/restore, иначе
# прерванный прогон оставит мутанта в миграции.
MIG_F8="schema/migrations/011_budget_sync_and_root_cap.sql"
# S8: мутация identity-проверки целится в step2b/roles_2b.sql — там живут
# перекрытые тела fail_task/fail_task_state (в них и стоит assert_worker_identity).
# Файл обязан быть под тем же backup/restore и git-clean fail-fast, иначе
# прерванный прогон оставит мутанта в перекрытии воркера.
ROLES_2B="step2b/roles_2b.sql"
# S10/S11: мутации handoff-фильтра и аренды целятся в миграцию 013. Она обязана
# быть под тем же backup/restore и git-clean fail-fast, иначе мутант переживает
# прогон: S10 снимает `assert_lease`, S11 — фильтр `result_ref`, и оба остаются
# в файле (проверено прогоном на disposable-БД 2026-10-16: после 11/11 CAUGHT
# рабочее дерево было грязным). На чистом CI это маскировалось свежим
# checkout, но локальный прогон оставлял мутанта в рабочей копии.
MIG_013="schema/migrations/013_task_parent_artifacts.sql"
# S12/S13/S14 (FIND-NEW-02): мутации identity в ПЕРЕКРЫВАЮЩИХ миграциях.
# 011 (MIG_F8) пересоздаёт claim_task/complete_task, 014 — task_rework_feedback.
# Эти файлы обязаны быть под тем же backup/restore и git-clean fail-fast: снятие
# identity-строки в перекрытии — именно тот дефект, который арбитры НЕ ловили
# (проверено зондом на disposable-БД: claim_task и complete_task в 011 дали
# SURVIVED), и который уронил бы защиту тихо.
MIG_014="schema/migrations/014_identity_in_override_migrations.sql"
: "${TEST_DATABASE_URL:?set TEST_DATABASE_URL (disposable DB, prefix test_/v0/c2_)}"

# Fail-fast: схема и целевая миграция должны быть в git-чистом состоянии. Если
# предыдущий прогон был прерван (SIGKILL/OOM) и не успел восстановить файл,
# trap — best-effort; не запускаем мутации на уже-мутированном файле.
if command -v git >/dev/null 2>&1 && git rev-parse --git-dir >/dev/null 2>&1; then
  for f in "$SCHEMA" "$MIG_F8" "$ROLES_2B" "$MIG_013" "$MIG_014"; do
    if ! git diff --quiet -- "$f"; then
      echo "BLOCKED: $f has uncommitted changes (leftover mutation?); restore it first" >&2
      echo "  hint: git checkout -- $f" >&2
      exit 2
    fi
  done
fi

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
BACKUP_MIG="${TMPDIR:-/tmp}/swarm_mig011_backup_$$.sql"
BACKUP_2B="${TMPDIR:-/tmp}/swarm_roles2b_backup_$$.sql"
BACKUP_013="${TMPDIR:-/tmp}/swarm_mig013_backup_$$.sql"
BACKUP_014="${TMPDIR:-/tmp}/swarm_mig014_backup_$$.sql"
cp "$SCHEMA" "$BACKUP"
[ -f "$MIG_F8" ] && cp "$MIG_F8" "$BACKUP_MIG"
[ -f "$ROLES_2B" ] && cp "$ROLES_2B" "$BACKUP_2B"
[ -f "$MIG_013" ] && cp "$MIG_013" "$BACKUP_013"
[ -f "$MIG_014" ] && cp "$MIG_014" "$BACKUP_014"
restore() {
  cp -f "$BACKUP" "$SCHEMA"
  [ -f "$BACKUP_MIG" ] && cp -f "$BACKUP_MIG" "$MIG_F8"
  [ -f "$BACKUP_2B" ] && cp -f "$BACKUP_2B" "$ROLES_2B"
  [ -f "$BACKUP_013" ] && cp -f "$BACKUP_013" "$MIG_013"
  [ -f "$BACKUP_014" ] && cp -f "$BACKUP_014" "$MIG_014"
}
cleanup() { restore; rm -f "$BACKUP" "$BACKUP_MIG" "$BACKUP_2B" "$BACKUP_013" "$BACKUP_014"; }
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
  # fail_unblock — fail/unblock; test_f8 — синхронный учёт расхода и
  # бюджет-осознанный reclaim (F8).
  #
  # S8 (identity в fail_task/fail_task_state): добавлены
  # test_worker_identity_f3.py — прямой behavioural-кейс «w1 вызывает
  # fail_task/fail_task_state по чужой задаче w2 → WK003», и
  # test_no_duplicate_function_bodies.py — структурный guard, что перекрытие
  # вообще содержит identity-строку. Без них S8 выживал: старый
  # test_worker_identity_f3 проверял только register_worker и не вызывал
  # fail_task/fail_task_state от чужого воркера.
  #
  # S10 (handoff): test_dag_handoff_iteration3.py — единственный потребитель
  # task_parent_artifacts. Без явного добавления сюда мутация S10 выживала
  # (проверено прогоном 38840603168): список арбитров ручной, и тест просто не
  # исполнялся. Тот же класс, что «проверка, которой нет».
  "$PY" -m pytest -q tests/test_v0.py tests/test_review_gate_f2.py \
        tests/test_fail_unblock_f4.py tests/test_f8_orphan_llm_spend.py \
        tests/test_worker_identity_f3.py \
        tests/test_no_duplicate_function_bodies.py \
        tests/test_dag_handoff_iteration3.py \
        tests/test_migration_identity_preserved.py >/dev/null 2>&1
}

# Чистое пересоздание схемы в disposable-БД.
#
# SQL-мутации проверяют ФУНКЦИИ СХЕМЫ, но после F8 тела функций стали
# SECURITY DEFINER от имени swarm_owner (миграция 011) и вызывают
# assert_worker_identity (roles_2b). Поэтому reset_db наливает ПОЛНУЮ
# прод-цепочку (как acceptance-harness): schema_v0 → roles → roles_2b →
# патчи → миграции. Это даёт правильных владельцев и права для SECURITY
# DEFINER; арбитры при этом ходят суперпользователем (PGDSN).
#
# ⚠️ roles.sql/roles_2b.sql делают `alter role ... password` (кластер-глобально),
# поэтому запускать ТОЛЬКО на изолированном контейнере (CI mut_pg / scratch_pg).
reset_db() {
  psql "$TEST_DATABASE_URL" -X -q -v ON_ERROR_STOP=1 \
    -c 'drop schema public cascade; create schema public;' >/dev/null 2>&1 || return 1
  psql "$TEST_DATABASE_URL" -X -q -v ON_ERROR_STOP=1 -f "$SCHEMA" >/dev/null 2>&1 || return 1
  psql "$TEST_DATABASE_URL" -X -q -v ON_ERROR_STOP=1 \
    -v worker_pw=c2_worker -v watchdog_pw=c2_watchdog -v planner_pw=c2_planner \
    -v reviewer_pw=c2_reviewer -v operator_pw=c2_operator -v cli_pw=c2_cli \
    -f step2/roles.sql >/dev/null 2>&1 || return 1
  psql "$TEST_DATABASE_URL" -X -q -v ON_ERROR_STOP=1 \
    -v w1_pw=c2_w1 -v w2_pw=c2_w2 -v w_1_pw=c2_w_1 -v gpu_1_pw=c2_gpu_1 \
    -f step2b/roles_2b.sql >/dev/null 2>&1 || return 1
  local p
  for p in step2/roles_patch_planner_status.sql step2/roles_patch_tool_call_audit.sql; do
    [ -f "$p" ] && { psql "$TEST_DATABASE_URL" -X -q -v ON_ERROR_STOP=1 -f "$p" >/dev/null 2>&1 || return 1; }
  done
  local mig
  for mig in schema/migrations/*.sql; do
    [ -e "$mig" ] || continue
    psql "$TEST_DATABASE_URL" -X -q -v ON_ERROR_STOP=1 -f "$mig" >/dev/null 2>&1 || return 1
  done
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
  local id="$1" desc="$2" old_str="$3" new_str="$4" file="${5:-$SCHEMA}"
  restore               # чистое исходное состояние схемы
  # Замена через python (надёжно с любыми символами, в т.ч. '|' и кавычками).
  if ! python3 - "$file" "$old_str" "$new_str" <<'PY'
import sys
path, old, new = sys.argv[1], sys.argv[2], sys.argv[3]
s = open(path, encoding="utf-8").read()
if old not in s:
    sys.exit(3)
s = s.replace(old, new, 1)
open(path, "w", encoding="utf-8").write(s)
PY
  then
    RESULTS+=("$id  INVALID   $desc  (паттерн мутации не найден в $file)")
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

# S6 — llm_calls_spend_sync: снять синхронный учёт расхода (F8).
# Мутация: триггер ledger перестаёт увеличивать tasks.spent_tokens.
# Ожидание: test_f8_orphan_llm_spend.py краснеет (spent_tokens = 0 при
# потраченном вызове) — иначе бюджет снова не ограничивает реальный расход.
# Целевой файл — миграция 011 (там живёт тело триггера).
run_mutation S6 "llm_calls_spend_sync: не учитывать расход из ledger (F8)" \
  "set spent_tokens = spent_tokens + coalesce(new.prompt_tokens, 0)" \
  "set spent_tokens = spent_tokens + 0 * coalesce(new.prompt_tokens, 0)" \
  "schema/migrations/011_budget_sync_and_root_cap.sql"

# S7 — reclaim_expired: снять бюджет-осознанный реклейм (F8).
# Мутация: оплаченная и оборванная аренда возвращается в ready, а не в
# budget_exhausted. Ожидание: test_f8_orphan_llm_spend.py краснеет.
# Целевой файл — миграция 011 (она перекрывает тело из schema_v0.sql).
run_mutation S7 "reclaim_expired: игнорировать исчерпанный бюджет (F8)" \
  "if r.spent_tokens >= r.budget_cap then" \
  "if false then" \
  "schema/migrations/011_budget_sync_and_root_cap.sql"

# S8 — fail_task/fail_task_state: снять identity-проверку (FIND-NEW-01).
# Мутация: из перекрытого тела fail_task в roles_2b.sql убрать
# `perform assert_worker_identity(p_worker);`. Строка `assert_worker_identity`
# встречается в файле 6 раз, поэтому паттерн расширен до сигнатуры fail_task —
# иначе `replace(..., 1)` попал бы в register_worker.
# Ожидание: test_worker_identity_f3.py краснеет (w1 с p_worker='w2' получает не
# WK003, а проходит в assert_lease), а test_no_duplicate_function_bodies.py —
# на структурном анкере «перекрытие содержит identity-строку».
run_mutation S8 "fail_task: снять identity-проверку воркера (FIND-NEW-01)" \
  "create or replace function fail_task(p_task uuid, p_worker text, p_epoch bigint,
                                     p_result_ref text, p_reason text) returns text
language plpgsql as \$\$
begin
  perform assert_worker_identity(p_worker);
  perform assert_lease(p_task, p_worker, p_epoch);" \
  "create or replace function fail_task(p_task uuid, p_worker text, p_epoch bigint,
                                     p_result_ref text, p_reason text) returns text
language plpgsql as \$\$
begin
  perform assert_lease(p_task, p_worker, p_epoch);" \
  "$ROLES_2B"

# S9 — complete_task: снять бюджетный гейт (Issue #20).
# Мутация: перерасход больше не переводит задачу в budget_exhausted.
# Целевой файл — миграция 011: именно она применяется последней и её
# `create or replace` перекрывает тела из schema_v0.sql и roles_2b.sql, т.е.
# мутировать канон в schema_v0.sql было бы no-op (мутант «выжил» бы ложно).
# Ожидание: test_v0.py::test_budget_exhaustion_blocks_on_complete краснеет.
run_mutation S9 "complete_task: игнорировать перерасход бюджета (Issue #20)" \
  "v_new := case when v_spent > v_cap then 'budget_exhausted' else 'review' end;" \
  "v_new := 'review';" \
  "schema/migrations/011_budget_sync_and_root_cap.sql"

# S10 — task_parent_artifacts (Итерация 3, DAG handoff): снять проверку живой
# аренды. Мутация: RPC отдаёт артефакты родительских задач вообще без
# `perform assert_lease(...)` — то есть любой воркер читает вход чужой задачи
# (утечка плана работ графа и содержимого артефактов).
# Ожидание: tests/test_dag_handoff_iteration3.py краснеет на
# test_iteration3_requires_live_lease (вместо LS001 — успешное чтение).
run_mutation S10 "task_parent_artifacts: снять проверку живой аренды (handoff утечка)" \
  "perform assert_lease(p_task, p_worker, p_epoch);" \
  "null;" \
  "schema/migrations/013_task_parent_artifacts.sql"

# S11 — task_parent_artifacts (Вопрос 4 аудита): фильтр финального артефакта.
# Мутация: убрать `and a.ref = pt.result_ref`. Тогда дочерняя задача получает
# ВСЕ артефакты родителя, включая черновики, отвергнутые на ревью (реворк):
# граф решает свою часть по мусору. Ожидание: краснеет
# test_iteration3_only_final_result_ref_returned и
# test_iteration3_done_without_result_ref_returns_nothing.
run_mutation S11 "task_parent_artifacts: вернуть отвергнутые черновики родителя (handoff без result_ref)" \
  "and a.ref = pt.result_ref" \
  "" \
  "schema/migrations/013_task_parent_artifacts.sql"

# S12 — complete_task в перекрывающей миграции 011: снять identity.
# Мутация: из тела complete_task в 011 убрать `perform assert_worker_identity`.
# Это ФИНАЛЬНОЕ определение (011 применяется последней), поэтому снятие строки
# снимает защиту в живом контуре. До этой ночи мутант ВЫЖИВАЛ: ни один арбитр
# не вызывал complete_task от чужого воркера (проверено зондом на disposable-БД).
# Ожидание: краснеет test_worker_identity_f3.py
# (test_f3_complete_task_foreign_worker_rejected) и/или статический страж.
run_mutation S12 "complete_task (011): снять identity в перекрытии (FIND-NEW-02)" \
  "  perform assert_worker_identity(p_worker);
  perform assert_lease(p_task, p_worker, p_epoch);
  if p_tokens < 0 then" \
  "  perform assert_lease(p_task, p_worker, p_epoch);
  if p_tokens < 0 then" \
  "$MIG_F8"

# S13 — claim_task в перекрывающей миграции 011: снять identity.
# Ожидание: краснеет test_f3_claim_task_foreign_worker_rejected (или страж).
run_mutation S13 "claim_task (011): снять identity в перекрытии (FIND-NEW-02)" \
  "  perform assert_worker_identity(p_worker);
  update workers set last_seen = now() where id = p_worker;" \
  "  update workers set last_seen = now() where id = p_worker;" \
  "$MIG_F8"

# S14 — task_rework_feedback в перекрывающей миграции 014: снять identity.
# Целевой файл — именно 014: она применяется последней и перекрывает 012,
# поэтому мутация в 012 была бы no-op («мутант выжил» ложно).
# Ожидание: краснеет test_f3_task_rework_feedback_foreign_worker_rejected.
run_mutation S14 "task_rework_feedback (014): снять identity в перекрытии (FIND-NEW-02)" \
  "  perform assert_worker_identity(p_worker);
  perform assert_lease(p_task, p_worker, p_epoch);
  select t.rework into v_rework" \
  "  perform assert_lease(p_task, p_worker, p_epoch);
  select t.rework into v_rework" \
  "$MIG_014"

echo
echo "==================== РЕЗУЛЬТАТЫ SQL-МУТАЦИЙ S1–S14 ==================="
for r in "${RESULTS[@]}"; do echo "$r"; done
echo "----------------------------------------------------------------"
echo "поймано: $caught / $((caught + survived))   выжило: $survived"
echo

# Регресс-страж: restore() обязан вернуть ВСЕ мутируемые файлы. Если после
# прогона файл грязный, мутант утёк в рабочее дерево — это дефект скрипта, а не
# «шум». Падаем явно, чтобы мутанта не закоммитили (так и случилось 2026-10-16:
# S10/S11 мутировали 013, а restore() его не покрывал).
if command -v git >/dev/null 2>&1 && git rev-parse --git-dir >/dev/null 2>&1; then
  dirty=0
  for f in "$SCHEMA" "$MIG_F8" "$ROLES_2B" "$MIG_013" "$MIG_014"; do
    if ! git diff --quiet -- "$f"; then
      echo "FAIL: $f остался изменённым после прогона — restore() не покрывает файл" >&2
      dirty=1
    fi
  done
  [ "$dirty" -eq 0 ] || exit 2
fi

[ "$survived" -eq 0 ]
