#!/usr/bin/env bash
# apply_migrations.sh — идемпотентный миграционный раннер для ЖИВОГО тома.
#
# Зачем (ночная смена 2026-10-17, остаточное ограничение §5.3).
#   Миграции `schema/migrations/*.sql` применяются скриптом
#   `scripts/05-migrations.sh`, подключённым в `docker-entrypoint-initdb.d`.
#   Скрипты initdb отрабатывают ТОЛЬКО при первой инициализации ПУСТОГО тома.
#   На существующем томе новые миграции не догоняются: после мержа 014 фикс
#   identity в перекрывающих миграциях остался неприменённым на демо-томе
#   (проверено на живом PG: тело `task_rework_feedback` — по-прежнему без
#   `assert_worker_identity`). Штатного пути «накатить миграции на живой том»
#   не было — только ручной `psql -f` по порядку, без реестра и без защиты от
#   повторного/частичного применения.
#
# Что делает:
#   * ведёт реестр применённого В САМОЙ БД — `public.schema_migrations`
#     (filename, sha256, время, кто применил). Тот же формат пишет
#     `scripts/05-migrations.sh` при инициализации пустого тома, поэтому
#     «свежий том» и «живой том» говорят на одном языке (дрейф формы реестра
#     между двумя скриптами ловит `tests/test_migration_ledger_runner.py`);
#   * применяет ТОЛЬКО отсутствующие в реестре файлы, лексикографически по
#     имени — тот же порядок, что в initdb-цепочке;
#   * каждый файл применяется в ОТДЕЛЬНОЙ транзакции ВМЕСТЕ с записью в реестр:
#     упавшая миграция откатывается целиком и НЕ помечается применённой (иначе
#     «применено» в реестре и фактическая схема разошлись бы);
#   * сверяет sha256 уже применённых файлов: если файл изменили ПОСЛЕ
#     применения, раннер отказывается работать (exit 4) вместо тихой работы на
#     разъехавшейся схеме;
#   * держит advisory-lock на всё время прогона: два параллельных раннера не
#     накатят один файл дважды;
#   * отказывается работать на БД без базовой схемы (exit 3) — «применил
#     миграции» на неинициализированной БД было бы ложным успехом;
#   * отказывается угадывать на УНАСЛЕДОВАННОМ томе (exit 6): если реестра нет,
#     а схема есть, неизвестно, что уже применено. Оператор говорит это явно —
#     `--adopt-up-to <последняя применённая миграция>`. Без этого флага
#     `--record-only` пометил бы применёнными и те файлы, которых на томе нет
#     (ровно так 014 осталась бы неприменённой, но «записанной»).
#
# Использование:
#   # живой том, доступный по сети (прод/VPS/scratch-контейнер):
#   scripts/apply_migrations.sh --dsn "host=127.0.0.1 port=5432 user=swarm dbname=swarm password=..."
#
#   # том внутри docker-контейнера (демо-контур; локальный сокет — trust,
#   # пароль нужен только если в pg_hba включена аутентификация):
#   scripts/apply_migrations.sh --docker-exec swarm-demo-postgres-1 \
#       --docker-user swarm --docker-db swarm
#
#   # унаследованный том: initdb применил 001–013, 014 — нет.
#   #   --adopt-up-to 013_  -> отметить всё до 013 включительно применённым
#   #                          (НЕ исполняя эти файлы), затем накатить 014
#   scripts/apply_migrations.sh --docker-exec swarm-demo-postgres-1 \
#       --docker-user swarm --docker-db swarm --adopt-up-to 013_
#
#   # --adopt-up-to принимает ПРЕФИКС имени файла; 'none' = не применено ни одной
#   # (БД, собранная руками, а не initdb-цепочкой).
#
#   # посмотреть состояние / план, ничего не меняя:
#   scripts/apply_migrations.sh --status  --dsn "..."
#   scripts/apply_migrations.sh --dry-run --dsn "..."
#
# Опции:
#   --dsn DSN                DSN для psql (по умолчанию — $DATABASE_URL)
#   --docker-exec CONTAINER  вместо DSN: psql внутри контейнера (docker exec)
#   --docker-user USER       пользователь БД в режиме --docker-exec (postgres)
#   --docker-db DB           база в режиме --docker-exec (обязательна)
#   --docker-password PW     PGPASSWORD для режима --docker-exec (по умолчанию нет)
#   --sql-dir DIR            каталог миграций на ХОСТЕ (schema/migrations)
#   --container-sql-dir DIR  каталог миграций внутри контейнера (/sql/migrations)
#   --adopt-up-to PREFIX     отметить применёнными файлы до последнего с этим
#                            префиксом включительно, не исполняя их; 'none' —
#                            не применено ни одной
#   --status                 показать реестр и план, выйти
#   --dry-run                показать план, ничего не менять
#   -h, --help               эта справка
#
# Коды выхода: 0 — успех; 2 — ошибка аргументов; 3 — БД не инициализирована
# (нет public.tasks); 4 — дрейф контрольной суммы применённого файла;
# 5 — миграция не применилась; 6 — реестра нет на инициализированной БД
# (унаследованный том: нужен --adopt-up-to).
set -Eeuo pipefail
# Порядок файлов — лексикографический. Без LC_ALL=C glob зависит от локали, и
# «001, 002, …, 010» может превратиться в «001, 002, 010, 003».
export LC_ALL=C

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SQL_DIR="$REPO_ROOT/schema/migrations"
CONTAINER_SQL_DIR="/sql/migrations"
DSN="${DATABASE_URL:-}"
DOCKER_EXEC=""
DOCKER_USER="postgres"
DOCKER_DB=""
DOCKER_PASSWORD=""
ADOPT_UP_TO=""
STATUS_ONLY=0
DRY_RUN=0

usage() {
  # Печатаем шапку файла: строки комментария после шебанга до первого кода.
  # Справка не дублируется отдельным текстом — иначе она разъезжается с файлом.
  awk 'NR > 1 { if ($0 !~ /^#/) exit; sub(/^# ?/, ""); print }' "${BASH_SOURCE[0]}"
}

die() { printf 'ERROR: %s\n' "$*" >&2; exit "${EXIT_CODE:-2}"; }

while (($#)); do
  case "$1" in
    --dsn) DSN="${2:?--dsn requires a DSN}"; shift 2 ;;
    --docker-exec) DOCKER_EXEC="${2:?--docker-exec requires a container}"; shift 2 ;;
    --docker-user) DOCKER_USER="${2:?--docker-user requires a user}"; shift 2 ;;
    --docker-db) DOCKER_DB="${2:?--docker-db requires a database}"; shift 2 ;;
    --docker-password) DOCKER_PASSWORD="${2:?--docker-password requires a value}"; shift 2 ;;
    --sql-dir) SQL_DIR="${2:?--sql-dir requires a path}"; shift 2 ;;
    --container-sql-dir) CONTAINER_SQL_DIR="${2:?--container-sql-dir requires a path}"; shift 2 ;;
    --adopt-up-to) ADOPT_UP_TO="${2:?--adopt-up-to requires a filename prefix}"; shift 2 ;;
    --status) STATUS_ONLY=1; shift ;;
    --dry-run) DRY_RUN=1; shift ;;
    -h|--help) usage; exit 0 ;;
    *) die "unknown option: $1 (см. --help)" ;;
  esac
done

[[ -n "$DSN" && -n "$DOCKER_EXEC" ]] && die "--dsn и --docker-exec взаимоисключающие"
[[ -z "$DSN" && -z "$DOCKER_EXEC" ]] && die "нужен --dsn (или DATABASE_URL) либо --docker-exec"
[[ -n "$DOCKER_EXEC" && -z "$DOCKER_DB" ]] && die "--docker-exec требует --docker-db"
[[ -d "$SQL_DIR" ]] || die "каталог миграций не найден: $SQL_DIR"
command -v sha256sum >/dev/null || die "нужен sha256sum"

# Как ходить в БД. В docker-режиме пути внутри `\i` — контейнерные, поэтому
# миграции читаются из CONTAINER_SQL_DIR, а контрольные суммы считаются по
# файлам на хосте (bind mount → то же содержимое; если монтирование разошлось,
# реестр поймает дрейф на следующем прогоне).
PSQL=()
build_psql() {
  if [[ -n "$DSN" ]]; then
    PSQL=(psql -X -q -v ON_ERROR_STOP=1 "$DSN")
  else
    local args=(docker exec -i)
    [[ -n "$DOCKER_PASSWORD" ]] && args+=(-e "PGPASSWORD=$DOCKER_PASSWORD")
    PSQL=("${args[@]}" "$DOCKER_EXEC" psql -X -q -v ON_ERROR_STOP=1 -U "$DOCKER_USER" -d "$DOCKER_DB")
  fi
}
build_psql

# Один запрос, вывод «голый» (-At). Значение возвращается через stdout.
sql_scalar() { "${PSQL[@]}" -Atc "$1"; }

# --- 0. Базовая схема на месте? ---------------------------------------------
# Раннер накатывает ТОЛЬКО миграции. Если таблиц ещё нет, значит том не проходил
# initdb-цепочку (01_schema.sql + роли) — применять 001..014 к пустой БД нельзя:
# часть миграций правит то, что создаёт schema_v0.sql.
if [[ "$(sql_scalar "select to_regclass('public.tasks') is not null")" != "t" ]]; then
  EXIT_CODE=3
  die "БД не инициализирована: нет public.tasks. Раннер накатывает ТОЛЬКО миграции; сначала примените schema/schema_v0.sql и роли (scripts/02-roles.sh, 03-roles-2b.sh, 03-role-patches.sh)"
fi

# --- 1. Реестр --------------------------------------------------------------
# Унаследованный том (схема есть, реестра нет) — отдельный случай, и решается он
# ДО создания реестра: иначе «создали пустой реестр» выглядело бы как «всё
# неприменено», и раннер пошёл бы накатывать 001 поверх существующей llm_calls.
LEDGER_EXISTS="$(sql_scalar "select to_regclass('public.schema_migrations') is not null")"
if [[ "$LEDGER_EXISTS" != "t" ]]; then
  if (( STATUS_ONLY )) || (( DRY_RUN )); then
    LEDGER_PRESENT=0
  elif [[ -z "$ADOPT_UP_TO" ]]; then
    EXIT_CODE=6
    die "на инициализированной БД нет реестра миграций: неизвестно, что уже накатано. Скажите это явно: --adopt-up-to <последняя применённая миграция, напр. 013_>, либо --adopt-up-to none, если не применено ни одной"
  else
    "${PSQL[@]}" <<'SQL'
create table if not exists public.schema_migrations (
  filename   text primary key,
  checksum   text not null,
  applied_at timestamptz not null default now(),
  applied_by text not null default session_user
);
do $$ begin
  -- Инвариант каталога (step2/test_roles.py::test_catalog_invariants): таблицы
  -- в public принадлежат не суперпользователю. Раннер обычно ходит
  -- суперпользователем, поэтому владельца переводим на swarm_owner.
  if exists (select 1 from pg_roles where rolname = 'swarm_owner') then
    execute 'alter table public.schema_migrations owner to swarm_owner';
  end if;
end $$;
revoke all on table public.schema_migrations from public;
SQL
    LEDGER_PRESENT=1
  fi
else
  LEDGER_PRESENT=1
fi

# psql -A отдаёт поля через `|` (не таб) — читаем ровно этим разделителем.
# Имена файлов проверены регексом ниже (`[A-Za-z0-9_.-]+`), поэтому `|` в них
# не встретится и разделитель неоднозначности не создаёт.
declare -A LEDGER=()
if (( LEDGER_PRESENT )); then
  while IFS='|' read -r name sum; do
    [[ -n "$name" ]] && LEDGER["$name"]="$sum"
  done < <(sql_scalar "select filename, checksum from public.schema_migrations order by filename")
fi

# --- 2. Граница "уже применённого" на унаследованном томе --------------------
# --adopt-up-to принимает ПРЕФИКС имени (013_, 013_task_parent_artifacts.sql) и
# означает «на томе применено всё до последнего файла с этим префиксом
# включительно». Сравнивать строки напрямую нельзя: '013_task_....sql' > '013_'
# (префикс — начало строки), и 013 уехала бы в pending.
ADOPT_BOUNDARY=""
if [[ -n "$ADOPT_UP_TO" && "$ADOPT_UP_TO" != "none" ]]; then
  for f in "$SQL_DIR"/*.sql; do
    [[ -e "$f" ]] || continue
    base="$(basename "$f")"
    [[ "$base" == "$ADOPT_UP_TO"* ]] && ADOPT_BOUNDARY="$base"
  done
  if [[ -z "$ADOPT_BOUNDARY" ]]; then
    die "--adopt-up-to '$ADOPT_UP_TO' не совпал ни с одним файлом в $SQL_DIR (нужен префикс имени миграции, либо 'none')"
  fi
fi

# --- 3. План ----------------------------------------------------------------
PENDING=(); ADOPT=(); DRIFT=(); ALREADY=0
for f in "$SQL_DIR"/*.sql; do
  [[ -e "$f" ]] || continue
  name="$(basename "$f")"
  [[ "$name" =~ ^[A-Za-z0-9_.-]+$ ]] || die "подозрительное имя файла миграции: $name"
  sum="$(sha256sum "$f" | cut -d' ' -f1)"
  if [[ -n "${LEDGER[$name]:-}" ]]; then
    if [[ "${LEDGER[$name]}" != "$sum" ]]; then
      DRIFT+=("$name")
    else
      ALREADY=$((ALREADY + 1))
    fi
  elif [[ -n "$ADOPT_BOUNDARY" && ! "$name" > "$ADOPT_BOUNDARY" ]]; then
    ADOPT+=("$name")
  else
    PENDING+=("$name")
  fi
done

TOTAL=$(( ALREADY + ${#ADOPT[@]} + ${#PENDING[@]} + ${#DRIFT[@]} ))
(( TOTAL > 0 )) || die "в $SQL_DIR нет ни одного .sql"

if (( ${#DRIFT[@]} > 0 )); then
  printf 'DRIFT: контрольная сумма применённых миграций не совпадает с файлами:\n' >&2
  printf '  %s\n' "${DRIFT[@]}" >&2
  printf 'Применённая схема и репозиторий разошлись. Разберитесь вручную:\n' >&2
  printf '  * если файл правили ПОСЛЕ применения — нужна новая миграция, а не правка старой;\n' >&2
  printf '  * если правка безобидна (комментарий) — обновите checksum в public.schema_migrations осознанно.\n' >&2
  EXIT_CODE=4
  die "дрейф контрольных сумм: ${#DRIFT[@]} файл(ов)"
fi

if (( STATUS_ONLY )); then
  if (( LEDGER_PRESENT )); then
    printf 'ledger: present\n'
    for name in $(printf '%s\n' "${!LEDGER[@]}" | sort); do
      printf 'applied  %s\n' "$name"
    done
  else
    printf 'ledger: absent (унаследованный том или том без реестра)\n'
  fi
  if (( ${#ADOPT[@]} )); then for name in "${ADOPT[@]}"; do printf 'adopt    %s\n' "$name"; done; fi
  if (( ${#PENDING[@]} )); then for name in "${PENDING[@]}"; do printf 'pending  %s\n' "$name"; done; fi
  printf 'summary: total=%d already=%d adopt=%d pending=%d drift=0\n' \
    "$TOTAL" "$ALREADY" "${#ADOPT[@]}" "${#PENDING[@]}"
  exit 0
fi

if (( DRY_RUN )); then
  if (( ${#ADOPT[@]} )); then for name in "${ADOPT[@]}"; do printf '[dry-run] adopt   %s\n' "$name"; done; fi
  if (( ${#PENDING[@]} )); then for name in "${PENDING[@]}"; do printf '[dry-run] apply   %s\n' "$name"; done; fi
  printf 'summary: total=%d already=%d adopt=%d pending=%d drift=0 (dry-run)\n' \
    "$TOTAL" "$ALREADY" "${#ADOPT[@]}" "${#PENDING[@]}"
  exit 0
fi

# --- 4. Прогон --------------------------------------------------------------
# Один psql-сеанс на всё: advisory-lock держится до конца прогона, а каждая
# миграция идёт в своей транзакции вместе с записью в реестр.
SCRIPT="$(mktemp "${TMPDIR:-/tmp}/swarm-migrations.XXXXXX.sql")"
trap 'rm -f "$SCRIPT"' EXIT

{
  printf '\\set ON_ERROR_STOP on\n'
  printf "select pg_advisory_lock(hashtext('swarm-dispatcher:apply_migrations')::bigint);\n"
  for name in "${ADOPT[@]}"; do
    sum="$(sha256sum "$SQL_DIR/$name" | cut -d' ' -f1)"
    printf "insert into public.schema_migrations (filename, checksum, applied_by)\n"
    printf "  values ('%s', '%s', session_user) on conflict (filename) do nothing;\n" "$name" "$sum"
  done
  for name in "${PENDING[@]}"; do
    sum="$(sha256sum "$SQL_DIR/$name" | cut -d' ' -f1)"
    path="$SQL_DIR/$name"
    [[ -n "$DOCKER_EXEC" ]] && path="$CONTAINER_SQL_DIR/$name"
    printf 'begin;\n'
    printf '\\i %s\n' "$path"
    printf "insert into public.schema_migrations (filename, checksum, applied_by)\n"
    printf "  values ('%s', '%s', session_user);\n" "$name" "$sum"
    printf 'commit;\n'
  done
  printf "select pg_advisory_unlock(hashtext('swarm-dispatcher:apply_migrations')::bigint);\n"
} > "$SCRIPT"

if (( ${#ADOPT[@]} )); then
  for name in "${ADOPT[@]}"; do printf '[adopt] %s (отмечено применённым без исполнения)\n' "$name"; done
fi
if (( ${#PENDING[@]} )); then
  for name in "${PENDING[@]}"; do printf '[apply] %s\n' "$name"; done
fi

# Готовый скрипт отдаём в psql ЧЕРЕЗ STDIN, а не через `-f`: в режиме
# --docker-exec psql исполняется ВНУТРИ контейнера, а mktemp живёт на хосте —
# `-f /tmp/...` внутри контейнера не существует (поймано на живом демо-томе).
# Заодно убирается класс «путь к скрипту интерпретируется не тем psql».
if ! "${PSQL[@]}" < "$SCRIPT" >/dev/null; then
  printf 'миграции не применены: psql вернул ошибку. Откатилась только упавшая\n' >&2
  printf 'транзакция; предыдущие файлы уже применены и записаны в реестр —\n' >&2
  printf 'повторный запуск продолжит с упавшего файла.\n' >&2
  EXIT_CODE=5
  die "ошибка применения миграции"
fi

printf 'summary: total=%d already=%d adopt=%d pending=%d drift=0\n' \
  "$TOTAL" "$ALREADY" "${#ADOPT[@]}" "${#PENDING[@]}"
