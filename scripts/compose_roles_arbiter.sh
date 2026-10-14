#!/usr/bin/env bash
# Живой арбитр ролей: прод-стек под своей ролью БД, а не «гейт зелёный».
#
# ЗАЧЕМ. `scripts/acceptance_harness.sh` (compose_security_acceptance) сверяет
# ОТРЕНДЕРЕННЫЙ docker-compose.yml: для каждого сервиса ожидаемая роль
# сравнивается с DATABASE_URL. Это ловит регрессию вида #24 (watchdog под
# ролью воркера) НА УРОВНЕ ФАЙЛА.
#
# Но файл может быть верным, а ЖИВОЙ контейнер — нет (старый образ, ручной
# `docker exec`, переопределённый env в рантайме). Класс #24/#26 — «гейт
# зелёный ≠ прод жив». Этот арбитр смотрит на РЕАЛЬНЫЕ контейнеры.
#
# ИНСТРУМЕНТ (почему не только pg_stat_activity).
#   Первая версия проверяла роль через `pg_stat_activity.usename` — и ложно
#   краснела на watchdog: он открывает НОВОЕ соединение на каждую итерацию и
#   сразу закрывает его (см. `run_iteration`: `SwarmDispatcher::connect` внутри
#   цикла). Коннект живёт миллисекунды раз в `WATCHDOG_INTERVAL` с — сэмплинг
#   pg_stat_activity его не ловит. Это «флаки-зелёный/красный», а не арбитр.
#   Поэтому здесь два ДЕТЕРМИНИРОВАННЫХ инструмента:
#     1) DSN-роль из env живого контейнера (docker exec printenv);
#     2) права роли в КАТАЛОГЕ (has_function_privilege) — не зависит от того,
#        подключён процесс прямо сейчас.
#   pg_stat_activity используется только как мягкий сигнал для долгоживущих
#   сервисов (worker/reviewer) и НЕ влияет на вердикт для watchdog/cli/planner.
#
# БЕЗОПАСНОСТЬ. Скрипт только читает: `docker exec printenv` и `select` под
# read-only ролью. Ничего не пишет, не перезапускает, не пересоздаёт.
#
# Использование:
#   scripts/compose_roles_arbiter.sh [--project NAME] [--pg-container NAME]
#   scripts/compose_roles_arbiter.sh --self-test   # доказать, что арбитр не «всегда зелёный»
#
# Код возврата: 0 — все роли совпали; 1 — расхождение; 2 — нечего проверять
# (стек не поднят) — это BLOCKED, а не «зелено».

set -Eeuo pipefail

PROJECT="${COMPOSE_PROJECT_NAME:-swarm-dispatcher}"
PG_CONTAINER=""
SELF_TEST=0

while (($#)); do
  case "$1" in
    --project) PROJECT="${2:?--project requires a name}"; shift 2 ;;
    --pg-container) PG_CONTAINER="${2:?--pg-container requires a name}"; shift 2 ;;
    --self-test) SELF_TEST=1; shift ;;
    -h|--help) sed -n '1,30p' "$0"; exit 0 ;;
    *) echo "FAIL unknown option: $1" >&2; exit 2 ;;
  esac
done

# Ожидаемая роль для сервиса. Единственный источник истины о соответствии
# «сервис → роль БД»; совпадает с compose_security_acceptance в harness.
#
# ВАЖНО: роль воркера выводится из WORKER_ID контейнера, а НЕ из имени
# сервиса. Сервис `worker-1` работает с `WORKER_ID=w1` → роль
# `swarm_worker_w1` (normalize: lowercase, дефис → подчёркивание).
# Первая версия арбитра выводила роль из имени (`swarm_worker_1`) и падала
# на собственном баге — это и есть доказательство, что проверка не «всегда
# зелёная». Аргумент $2 — WORKER_ID (пусто для не-воркерских сервисов).
expected_role() {
  case "$1" in
    watchdog) echo "swarm_watchdog" ;;
    reviewer) echo "swarm_reviewer" ;;
    cli)      echo "swarm_cli" ;;
    planner) echo "swarm_planner" ;;
    worker-*)
      local wid="${2:?worker role needs WORKER_ID}"
      printf 'swarm_worker_%s' "$(printf '%s' "$wid" | tr 'A-Z' 'a-z' | tr '-' '_')" ;;
    *)        echo "" ;;
  esac
}

# Роль из DSN: postgres://<user>:...@host/db
role_from_dsn() {
  printf '%s' "$1" | sed -E 's#^[a-z]+://([^:/@]+)(:[^@]*)?@.*#\1#'
}

# --- self-test: доказать, что арбитр краснеет на классе #24 -------------------
# Не трогает прод: проверяет чистые функции на синтетических данных.
if ((SELF_TEST)); then
  rc=0
  # (a) корректная карта → ожидаем PASS.
  [[ "$(expected_role watchdog)" == "swarm_watchdog" ]] || { echo "FAIL self-test: watchdog→role"; rc=1; }
  [[ "$(expected_role worker-1 w1)" == "swarm_worker_w1" ]] || { echo "FAIL self-test: worker-1/w1→role"; rc=1; }
  [[ "$(expected_role worker-2 w2)" == "swarm_worker_w2" ]] || { echo "FAIL self-test: worker-2/w2→role"; rc=1; }
  # (b) роль из DSN разбирается верно.
  [[ "$(role_from_dsn 'postgres://swarm_watchdog:pw@postgres:5432/swarm')" == "swarm_watchdog" ]] \
    || { echo "FAIL self-test: role_from_dsn"; rc=1; }
  # (c) КЛЮЧЕВОЕ: карта с watchdog под воркерской ролью обязана дать mismatch.
  dsn="postgres://swarm_worker_w1:pw@postgres:5432/swarm"
  got="$(role_from_dsn "$dsn")"
  exp="$(expected_role watchdog)"
  if [[ "$got" == "$exp" ]]; then
    echo "FAIL self-test: арбитр НЕ поймал watchdog под воркерской ролью (#24)"; rc=1
  else
    echo "PASS self-test: #24 ловится (watchdog DSN-роль '$got' != '$exp')"
  fi
  if ((rc)); then echo "RESULT: self-test FAIL"; exit 1; fi
  echo "RESULT: self-test PASS (арбитр краснеет на подмене роли)"
  exit 0
fi

fail=0
checked=0
seen_roles=""
worker_roles=""
watchdog_role=""
reviewer_role=""
planner_role=""

for svc in watchdog reviewer cli planner worker-1 worker-2; do
  c="${PROJECT}-${svc}-1"
  if ! docker inspect -f '{{.State.Running}}' "$c" >/dev/null 2>&1; then
    continue
  fi
  wid=""
  case "$svc" in worker-*) wid="$(docker exec "$c" printenv WORKER_ID 2>/dev/null || true)";; esac
  exp="$(expected_role "$svc" "$wid")"
  [[ -n "$exp" ]] || continue
  dsn="$(docker exec "$c" printenv DATABASE_URL 2>/dev/null || true)"
  if [[ -z "$dsn" ]]; then
    echo "FAIL $svc: DATABASE_URL отсутствует в контейнере"
    fail=1; continue
  fi
  got="$(role_from_dsn "$dsn")"
  if [[ "$got" != "$exp" ]]; then
    echo "FAIL $svc: DSN-роль '$got', ожидалась '$exp'"
    fail=1
  else
    echo "PASS $svc: DSN-роль = $exp"
  fi
  checked=$((checked + 1))
  seen_roles="$seen_roles $exp"
  case "$svc" in
    watchdog) watchdog_role="$exp" ;;
    reviewer) reviewer_role="$exp" ;;
    planner)  planner_role="$exp" ;;
    worker-*) worker_roles="$worker_roles $exp" ;;
  esac
done

if ((checked == 0)); then
  echo "BLOCKED: ни один сервис не запущен (project=$PROJECT)"
  exit 2
fi

# --- детерминированный инструмент: права роли в каталоге ---------------------
if [[ -z "$PG_CONTAINER" ]]; then
  PG_CONTAINER="$(docker ps --format '{{.Names}}' | grep -E "(^|-)postgres(-1)?$" | head -1 || true)"
fi
if [[ -z "$PG_CONTAINER" ]]; then
  echo "BLOCKED: postgres-контейнер не найден (стек не поднят?)"
  exit 2
fi
pw="$(docker exec "$PG_CONTAINER" printenv POSTGRES_PASSWORD 2>/dev/null || true)"
if [[ -z "$pw" ]]; then
  echo "BLOCKED: POSTGRES_PASSWORD недоступен в $PG_CONTAINER"
  exit 2
fi
q() { docker exec -e PGPASSWORD="$pw" "$PG_CONTAINER" psql -X -U swarm -d swarm -Atc "$1" 2>/dev/null; }

# Признак роли: набор функций, которые она ОБЯЗАНА (t) и НЕ ВПРАВЕ (f) иметь.
# Проверка не зависит от текущих подключений — только от каталога.
if [[ -n "$watchdog_role" ]]; then
  wd_priv="$(q "select has_function_privilege('$watchdog_role','reclaim_expired()','EXECUTE') and has_function_privilege('$watchdog_role','promote_ready()','EXECUTE')")"
  if [[ "$wd_priv" == "t" ]]; then
    echo "PASS catalog: $watchdog_role имеет reclaim_expired/promote_ready"
  else
    echo "FAIL catalog: $watchdog_role НЕ имеет reclaim_expired/promote_ready (сторож не сможет работать)"; fail=1
  fi
fi

# FIND-NEW-06: позитивные проверки и для reviewer/planner (раньше — только
# watchdog/worker). Reviewer обязан уметь review_candidates/submit_review и НЕ
# вправе брать задачи в работу (claim_task) — иначе валидатор может сам себе
# выдать задачу и нарушить разделение ролей.
if [[ -n "$reviewer_role" ]]; then
  rv_ok="$(q "select has_function_privilege('$reviewer_role','review_candidates(integer)','EXECUTE') and has_function_privilege('$reviewer_role','submit_review(uuid,text,text,text,jsonb)','EXECUTE')")"
  rv_claim="$(q "select has_function_privilege('$reviewer_role','claim_task(text,integer)','EXECUTE')")"
  if [[ "$rv_ok" == "t" ]]; then
    echo "PASS catalog: $reviewer_role может review_candidates/submit_review"
  else
    echo "FAIL catalog: $reviewer_role НЕ имеет review_candidates/submit_review (ревьюер не сможет работать)"; fail=1
  fi
  if [[ "$rv_claim" == "f" ]]; then
    echo "PASS catalog: $reviewer_role не может claim_task (разделение ролей)"
  else
    echo "FAIL catalog: $reviewer_role имеет claim_task — валидатор не должен брать задачи"; fail=1
  fi
fi

# Planner обязан уметь add_task/add_dep и НЕ вправе завершать/ревьюить задачи.
if [[ -n "$planner_role" ]]; then
  pl_ok="$(q "select has_function_privilege('$planner_role','add_task(uuid,uuid,jsonb,bigint,integer)','EXECUTE') and has_function_privilege('$planner_role','add_dep(uuid,uuid)','EXECUTE')")"
  pl_bad="$(q "select has_function_privilege('$planner_role','complete_task(uuid,text,bigint,text,bigint)','EXECUTE') or has_function_privilege('$planner_role','submit_review(uuid,text,text,text,jsonb)','EXECUTE')")"
  if [[ "$pl_ok" == "t" ]]; then
    echo "PASS catalog: $planner_role может add_task/add_dep"
  else
    echo "FAIL catalog: $planner_role НЕ имеет add_task/add_dep (планировщик не сможет работать)"; fail=1
  fi
  if [[ "$pl_bad" == "f" ]]; then
    echo "PASS catalog: $planner_role не может complete_task/submit_review"
  else
    echo "FAIL catalog: $planner_role имеет complete_task/submit_review — планировщик не должен закрывать задачи"; fail=1
  fi
fi

for wr in $worker_roles; do
  # Воркер обязан уметь брать/завершать задачу и НЕ обязан уметь работу сторожа.
  can_work="$(q "select has_function_privilege('$wr','claim_task(text,integer)','EXECUTE') and has_function_privilege('$wr','complete_task(uuid,text,bigint,text,bigint)','EXECUTE')")"
  can_watch="$(q "select has_function_privilege('$wr','reclaim_expired()','EXECUTE')")"
  if [[ "$can_work" == "t" ]]; then
    echo "PASS catalog: $wr может claim_task/complete_task"
  else
    echo "FAIL catalog: $wr НЕ может claim_task/complete_task"; fail=1
  fi
  if [[ "$can_watch" == "f" ]]; then
    echo "PASS catalog: $wr не имеет прав сторожа (deny-by-default)"
  else
    echo "FAIL catalog: $wr имеет reclaim_expired — роль воркера слишком широка"; fail=1
  fi
done

# --- негативные инварианты безопасности (директива аудитора) -----------------
# Позитивные проверки (есть ли право) доказывают работоспособность, но НЕ
# изоляцию. Арбитр обязан проверять и запреты:
#   (a) у сервисной роли НЕТ rolsuper/rolbypassrls;
#   (b) нет ПРЯМЫХ изменяющих прав на защищённые таблицы — доступ только
#       через SECURITY DEFINER RPC (иначе воркер может править tasks/events
#       в обход инвариантов и identity-проверок).
#       ИСКЛЮЧЕНИЕ — llm_calls: воркер ОБЯЗАН писать туда cost ledger
#       (грант INSERT/SELECT из migration 002). Для неё запрещаем только
#       UPDATE/DELETE (ledger неизменяем), INSERT — легитимен.
IMMUTABLE_LEDGER="llm_calls"
RPC_ONLY_TABLES="tasks events artifacts reviews task_deps workers"
for role in $seen_roles; do
  privs="$(q "select rolsuper::text || '|' || rolbypassrls::text from pg_roles where rolname='$role'")"
  case "$privs" in
    "false|false") echo "PASS catalog: $role не super/bypassrls" ;;
    "") echo "WARN catalog: роль $role не найдена в pg_roles" ;;
    *) echo "FAIL catalog: $role имеет rolsuper/rolbypassrls ($privs)"; fail=1 ;;
  esac
  # Таблицы только-через-RPC: любой INSERT/UPDATE/DELETE — нарушение.
  for tbl in $RPC_ONLY_TABLES; do
    direct="$(q "select has_table_privilege('$role','$tbl','INSERT') or has_table_privilege('$role','$tbl','UPDATE') or has_table_privilege('$role','$tbl','DELETE')")"
    if [[ "$direct" == "t" ]]; then
      echo "FAIL catalog: $role имеет прямой DML на $tbl (в обход RPC)"; fail=1
    fi
  done
  # Неизменяемый ledger: запрещены UPDATE/DELETE (INSERT — легитимен).
  for tbl in $IMMUTABLE_LEDGER; do
    mut="$(q "select has_table_privilege('$role','$tbl','UPDATE') or has_table_privilege('$role','$tbl','DELETE')")"
    if [[ "$mut" == "t" ]]; then
      echo "FAIL catalog: $role может менять/удалять $tbl (ledger должен быть неизменяем)"; fail=1
    fi
  done
done
echo "PASS catalog: негативные инварианты проверены (super/bypassrls, прямой DML)"

# Прямой запрет класса #24: watchdog не под ролью воркера.
if [[ -n "$watchdog_role" ]]; then
  case "$watchdog_role" in
    swarm_worker_*) echo "FAIL #24: watchdog подключён ролью воркера ($watchdog_role)"; fail=1 ;;
    swarm_watchdog) echo "PASS #24: watchdog не под ролью воркера" ;;
  esac
fi

# --- мягкий сигнал: долгоживущие сервисы держат соединение -------------------
# worker/reviewer подключаются постоянно; их присутствие в pg_stat_activity —
# дополнительное подтверждение, что процесс ЖИВ (не только env корректен).
# Для watchdog/cli/planner это НЕ проверяем (короткие/периодические коннекты).
live="$(q "select distinct usename from pg_stat_activity where usename like 'swarm%'")"
for wr in $worker_roles; do
  if printf '%s\n' "$live" | grep -qx "$wr"; then
    echo "PASS live: $wr держит соединение"
  else
    echo "WARN live: $wr не виден в снимке pg_stat_activity (не влияет на вердикт)"
  fi
done

if ((fail)); then
  echo "RESULT: FAIL (арбитр нашёл расхождение ролей)"
  exit 1
fi
echo "RESULT: PASS (все $checked сервисов под ожидаемыми ролями)"
