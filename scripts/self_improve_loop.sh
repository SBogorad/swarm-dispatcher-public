#!/usr/bin/env bash
# self_improve_loop.sh — замкнутый контур самомодификации роя swarm-dispatcher.
#
# Зачем. До 2026-10-18 код репозитория правил внешний агент, а рой только
# копил текстовые уроки в `public.agent_memory`. Этот скрипт замыкает петлю:
#
#   A. Интроспекция — собирает вход из ЖИВЫХ данных, а не из фантазии:
#      уроки `agent_memory` (что уже ломалось), телеметрию реворков
#      (`tasks.rework`), зависшие задачи (`blocked`) и текущий каталог крейтов.
#   B. Исполнение роем — подаёт постановку в `swarm-planner`; граф
#      planner → worker → reviewer выполняется штатно, воркеры читают реальные
#      файлы из `SWARM_REPO_DIR` (read-only) и вспоминают уроки через
#      `task_recall_memory`.
#   C. Применение и верификация — достаёт unified diff ИЗ ОДОБРЕННОГО
#      АРТЕФАКТА роя (не из моего текста), применяет `git apply`, гоняет
#      жёсткий гейт. Красный гейт — не повод для человека: причина пишется
#      обратно в `agent_memory` как урок `blocked`, и виток повторяется с
#      этим уроком в постановке.
#   D. Рекурсия — принятый патч коммитится, уходит в PR, мержится, контейнеры
#      пересобираются, и следующий виток идёт уже на изменённом коде.
#
# Почему патч — НОВЫЙ ФАЙЛ. Воркер видит репозиторий только на чтение и не
# исполняет команды. Диff, создающий новый файл, не требует контекстных строк
# из целевого файла — значит `git apply` не зависит от того, насколько точно
# модель воспроизвела окружение. Для правки существующего файла ошибка в одной
# контекстной строке рушит весь патч, и «рой не смог» означало бы «не сошёлся
# контекст», а не «рой не справился с задачей». Поэтому виток добавляет
# ИМЕННО новый тест: это настоящий вклад в покрытие, и его проверяет тот же
# гейт, что и любой другой код.
#
# Использование:
#   scripts/self_improve_loop.sh --round 1                 # полный виток
#   scripts/self_improve_loop.sh --round 2 --dry-run       # только постановка
#   scripts/self_improve_loop.sh --round 3 --skip-merge    # без PR/мержа
#
# Коды возврата: 0 — виток замкнут (патч роя в main), 2 — постановка не
# выполнена за отведённые попытки (урок записан), 3 — гейт красный (урок
# записан), 4 — окружение (docker/psql/gh) недоступно.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ROUND=""
DRY_RUN=0
SKIP_MERGE=0
MAX_ATTEMPTS=3
GRAPH_TIMEOUT_SEC=900

while [[ $# -gt 0 ]]; do
  case "$1" in
    --round) ROUND="${2:?--round requires a number}"; shift 2 ;;
    --dry-run) DRY_RUN=1; shift ;;
    --skip-merge) SKIP_MERGE=1; shift ;;
    --max-attempts) MAX_ATTEMPTS="${2:?}"; shift 2 ;;
    --graph-timeout) GRAPH_TIMEOUT_SEC="${2:?}"; shift 2 ;;
    -h|--help) sed -n '2,40p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "ERROR: unknown option $1" >&2; exit 4 ;;
  esac
done

[[ -n "$ROUND" ]] || { echo "ERROR: --round is required" >&2; exit 4; }

log()  { printf '[self-improve %s] %s\n' "$ROUND" "$*"; }
fail() { printf '[self-improve %s] ERROR: %s\n' "$ROUND" "$*" >&2; exit "${2:-4}"; }

cd "$REPO_ROOT"

command -v docker >/dev/null || fail "docker недоступен"
docker compose ps --format '{{.Service}}' >/dev/null 2>&1 || fail "docker compose недоступен"

# ---------------------------------------------------------------------------
# Витки: что именно просим у роя. Тема берётся из данных, но «форма» работы
# фиксирована — новый интеграционный тест. Виток 2 обязан опираться на урок
# витка 1, иначе «память между витками» нечем доказать.
# ---------------------------------------------------------------------------
case "$ROUND" in
  1)
    CRATE="swarm-worker"
    TARGET="crates/swarm-worker/tests/self_improve_round1.rs"
    SUBJECT="порядок секций промпта воркера и неприкосновенность кода"
    BEHAVIOUR="1) \`user_prompt_with_repo\` ставит причину отказа (rework) ПОСЛЕДНЕЙ секцией: \
индекс подстроки 'Это повторная попытка' больше индексов секции уроков \
('Уроки прошлых графов того же вида работ') и секции исходников \
('Исходники репозитория, упомянутые в постановке'); \
2) \`sanitize_untrusted\` не портит код: вход 'fn f(x: Vec<T>) -> u8' возвращается \
БЕЗ ИЗМЕНЕНИЙ (дженерики и стрелки сохраняются); \
3) при пустом списке уроков секция памяти отсутствует (промпт первого графа не меняется)."
    READ_FILES="crates/swarm-worker/src/prompt.rs crates/swarm-worker/src/lib.rs"
    ;;
  2)
    CRATE="swarm-core"
    TARGET="crates/swarm-core/tests/self_improve_round2.rs"
    SUBJECT="валидация контракта задачи и разбор вердикта ревьюера"
    BEHAVIOUR="1) \`TaskContract::validate\` отвергает пустой \`checks\` и пустой \`kind\`, \
и принимает корректный контракт; \
2) \`TaskContract\` сериализуется в JSON с полем \`checks\` как массивом (round-trip через serde_json); \
3) \`Verdict::as_str\` даёт ровно 'approved' / 'changes_requested' / 'rejected' — эти строки \
пишутся в БД и читаются контуром памяти."
    READ_FILES="crates/swarm-core/src/types.rs crates/swarm-core/src/lib.rs"
    ;;
  *)
    fail "виток $ROUND не описан в скрипте: добавь тему, иначе рой получит бессмысленную задачу"
    ;;
esac

BRANCH="self-improve/round-${ROUND}"
PATCH_DIR="$REPO_ROOT/.self-improve"
PATCH_FILE="$PATCH_DIR/round-${ROUND}.patch"

# ---------------------------------------------------------------------------
# A. Интроспекция: живая память + телеметрия.
# ---------------------------------------------------------------------------
db() { docker compose exec -T postgres psql -U swarm -d swarm -X -tA -F ' | ' -c "$1"; }

log "A: читаю память роя и телеметрию"
LESSONS="$(db "select kind, outcome, rework_count, left(replace(lesson, E'\n', ' '), 300)
                 from public.agent_memory order by id desc limit 6;")"
REWORK_STATS="$(db "select coalesce(nullif(contract->>'kind',''),'?'), count(*) filter (where rework > 0), count(*)
                      from public.tasks group by 1 order by 3 desc limit 6;")"
BLOCKED="$(db "select coalesce(nullif(contract->>'kind',''),'?'), count(*), left(max(coalesce(block_reason,'')), 120)
                 from public.tasks where state = 'blocked' group by 1 order by 2 desc limit 5;")"

if [[ -z "${LESSONS//[[:space:]]/}" ]]; then
  LESSONS="(память пуста — это первый виток, опирайся на принципы и на код)"
fi

# Возможности парка: планировщик вправе запросить `needs.coder_class`,
# которого нет ни у одного воркера, и тогда задача навсегда останется в
# `ready` — `claim_task` фильтрует по `capabilities.models.coder_class`.
# Живой случай (виток 1, граф `15e21856`): LLM запросил `coder_class: 3.0`
# при потолке `2.0` у w1/w2, и граф встал без ошибки в логе. Потолок
# передаётся в постановку числами ИЗ БД, а не догадкой.
WORKER_CEILING="$(db "select coalesce(min((capabilities->'models'->>'coder_class')::numeric), 0)::text
                         || ' / ' ||
                         coalesce(min((capabilities->>'ram_gb')::numeric), 0)::text
                        from public.workers;" 2>/dev/null || true)"
CEIL_CLASS="${WORKER_CEILING%% / *}"
CEIL_RAM="${WORKER_CEILING##* / }"
[[ -n "$CEIL_CLASS" && "$CEIL_CLASS" != "0" ]] || CEIL_CLASS=2
[[ -n "$CEIL_RAM" && "$CEIL_RAM" != "0" ]] || CEIL_RAM=16

mkdir -p "$PATCH_DIR"

TASK_TEXT="$(cat <<EOF
Самосовершенствование роя swarm-dispatcher, виток ${ROUND}.

Ты — часть роя, который улучшает собственный исходный код. Задача — добавить
РОВНО ОДИН новый файл интеграционного теста, который закрепляет поведение,
уже реализованное в коде. Ничего не переписывай и не рефактори.

НОВЫЙ ФАЙЛ (путь ровно такой): ${TARGET}
КРЕЙТ: ${CRATE}
ПРЕДМЕТ: ${SUBJECT}

ЧТО ИМЕННО ДОЛЖЕН ПРОВЕРЯТЬ ТЕСТ:
${BEHAVIOUR}

ФАЙЛЫ, КОТОРЫЕ НУЖНО ПРОЧИТАТЬ ПЕРЕД ОТВЕТОМ (доступны только на чтение):
${READ_FILES}

ПАМЯТЬ РОЯ — уроки прошлых витков (kind | outcome | rework | текст):
${LESSONS}

ТЕЛЕМЕТРИЯ РЕВОРКОВ (kind | задач с реворком | всего задач):
${REWORK_STATS}

ЗАБЛОКИРОВАННЫЕ ЗАДАЧИ (kind | штук | последняя причина):
${BLOCKED}

ВОЗМОЖНОСТИ ПАРКА (жёсткий потолок, выше — задача никогда не будет взята):
- \`needs.coder_class\` НЕ БОЛЬШЕ ${CEIL_CLASS}
- \`needs.min_ram_gb\` НЕ БОЛЬШЕ ${CEIL_RAM}
Ставь эти значения или ниже. Задача с завышенным \`coder_class\` остаётся в
\`ready\` навсегда: \`claim_task\` фильтрует по возможностям воркера, и никто
её не возьмёт.

ТРЕБОВАНИЯ К ОТВЕТУ (нарушение любого = отказ ревьюера):
- Ответ — ОДИН unified diff в блоке \`\`\`diff ... \`\`\`.
- Diff создаёт РОВНО ОДИН новый файл: ${TARGET}. Других файлов в диффе нет.
- Заголовки ровно: \`--- /dev/null\` и \`+++ b/${TARGET}\`.
- Файл КОМПАКТНЫЙ: не более 90 строк. Без вспомогательных функций, без таблиц
  в комментариях, без длинных doc-блоков. Три теста — три функции.
- Заголовок hunk'а (\`@@ -0,0 +1,N @@\`) обязан совпадать с числом строк в блоке:
  пересчитай N перед отправкой. Обрезанный блок не применится и будет отклонён.
- Никаких заглушек и маркеров незавершённой работы в тексте.
- Тест обязан компилироваться и проходить под \`cargo test -p ${CRATE} --locked\`.
- Используй ТОЛЬКО публичный API крейта; не тяни новые зависимости.
- Не создавай отдельную подзадачу-верификатор: прогон \`cargo test\` выполняет
  внешний контур, у тебя нет shell. Одна задача — один файл теста.
- После блока diff добавь секцию \`## Decision Trail\` — markdown-таблицу
  со столбцами \`phase | decision | why | evidence | result\`. В КАЖДОЙ строке
  \`evidence\` — проверяемый указатель: путь с файлом (${READ_FILES%% *}),
  \`cargo test -p ${CRATE}\` или \`rc=0\`. Описательный текст ревьюер отклонит.
- Последняя секция — \`## Handoff\`: одной строкой, что именно закрепляет тест.
EOF
)"

if ((DRY_RUN)); then
  printf '%s\n' "$TASK_TEXT"
  exit 0
fi

# ---------------------------------------------------------------------------
# B. Исполнение роем.
# ---------------------------------------------------------------------------
planner_run() {
  local text="$1"
  docker compose run --rm --no-deps planner swarm-planner "$text" 2>&1 \
    | tee /tmp/self-improve-planner.log \
    | sed -n 's/^root_id: //p' | tail -1
}

wait_graph() {
  local root="$1" waited=0 state unclaimable
  while (( waited < GRAPH_TIMEOUT_SEC )); do
    state="$(db "select count(*) from public.tasks
                  where root_id = '$root'
                    and state in ('pending','ready','leased','review');")"
    if [[ "$state" == "0" ]]; then
      return 0
    fi
    # Задача в `ready`, которую не может взять НИ ОДИН воркер, не сдвинется
    # никогда: ждать её — значит сжечь таймаут и записать урок не о том.
    unclaimable="$(db "select count(*) from public.tasks t
                        where t.root_id = '$root' and t.state = 'ready'
                          and t.attempts < 3
                          and not exists (
                            select 1 from public.workers w
                             where (w.capabilities->'models'->>'coder_class')::numeric
                                     >= (t.contract #>> '{needs,coder_class}')::numeric
                               and (w.capabilities->>'ram_gb')::numeric
                                     >= (t.contract #>> '{needs,min_ram_gb}')::numeric);")"
    if [[ -n "$unclaimable" && "$unclaimable" != "0" ]]; then
      log "граф встал: $unclaimable ready-задач(и) требуют возможностей выше парка (coder_class<=$CEIL_CLASS)"
      return 1
    fi
    sleep 10
    waited=$((waited + 10))
  done
  return 1
}

# Одобренные артефакты графа, от свежих к старым.
#
# Берём не «артефакт корня»: планировщик вправе разбить постановку на
# подзадачи, и diff может оказаться у любой из них. Порядок по `r.id desc`
# отражает финальное состояние после всех реворков, поэтому перебор идёт от
# самого свежего одобрения и останавливается на первом, из которого реально
# извлекается diff.
artifact_refs_for_root() {
  local root="$1"
  # Свежие сверху: `max(r.id) desc` отражает финальное одобрение после всех
  # реворков. Раньше здесь был `order by 1` + `tac` (обратный лексикографический
  # порядок хэшей) — это выбирало артефакт по случайному признаку, а не по
  # времени. Живой прогон витка 1 показал дефект: при трёх одобренных задачах
  # графа первым мог приехать артефакт `code.verify`, в котором diff'а нет.
  db "select a.ref from public.tasks t
        join public.artifacts a on a.task_id = t.id
        join public.reviews r on r.task_id = t.id and r.artifact_ref = a.ref
       where t.root_id = '$root' and r.verdict = 'approved'
       group by a.ref
       order by max(r.id) desc;"
}

read_artifact() {
  local ref="$1" hex vol
  hex="${ref#sha256:}"
  if docker compose exec -T worker-1 sh -c "test -f /var/swarm/artifacts/${hex}.txt" 2>/dev/null; then
    docker compose exec -T worker-1 sh -c "cat /var/swarm/artifacts/${hex}.txt"
    return 0
  fi
  vol="$(docker compose config --format json 2>/dev/null \
        | python3 -c 'import json,sys; d=json.load(sys.stdin); print(d["name"]+"_swarm-artifacts")' 2>/dev/null || true)"
  [[ -n "$vol" ]] || return 1
  docker run --rm -v "${vol}:/a:ro" alpine:3 sh -c "cat /a/${hex}.txt"
}

extract_patch() {
  # stdin: артефакт. stdout: unified diff. Ненулевой код — diff не найден.
  python3 - "$TARGET" <<'PY'
import re, sys
target = sys.argv[1]
body = sys.stdin.read()
blocks = re.findall(r"```(?:diff|patch)?\n(.*?)```", body, re.S)
cands = [b for b in blocks if "+++ b/" in b or b.lstrip().startswith("diff --git")]
if not cands:
    sys.exit(1)
# Предпочитаем блок, который создаёт ровно целевой файл.
for b in cands:
    if f"+++ b/{target}" in b and b.count("+++ b/") == 1:
        print(b if b.endswith("\n") else b + "\n", end="")
        sys.exit(0)
sys.exit(2)
PY
}

write_lesson() {
  local task="$1" kind="$2" outcome="$3" text="$4"
  # Прямая вставка: консолидатор пишет уроки сам, но здесь источник — гейт,
  # а не переход состояния задачи. `on conflict` держит инвариант «один урок
  # на (задача, вид)» из 016.
  db "insert into public.agent_memory (kind, source_task_id, rework_count, lesson, outcome)
      select '$kind', '$task', coalesce(t.rework, 0), left(\$lesson\$${text}\$lesson\$, 2000), '$outcome'
        from public.tasks t where t.id = '$task'
      on conflict (source_task_id, outcome) do update set lesson = excluded.lesson;" >/dev/null \
    || log "WARN: урок не записан (задача $task)"
  log "урок записан: $outcome $kind"
}

attempt=1
while (( attempt <= MAX_ATTEMPTS )); do
  log "B: виток $ROUND, попытка $attempt — подаю постановку в planner"
  ROOT_ID="$(planner_run "$TASK_TEXT")"
  [[ -n "$ROOT_ID" ]] || fail "planner не вернул root_id (см. /tmp/self-improve-planner.log)"
  log "граф: $ROOT_ID"

  if ! wait_graph "$ROOT_ID"; then
    REASON="$(db "select coalesce(string_agg(coalesce(block_reason,'нет'), '; '), 'нет')
                    from public.tasks where root_id = '$ROOT_ID' and state = 'blocked';")"
    UNCLAIMABLE="$(db "select coalesce(string_agg(t.contract->>'kind' || ' (coder_class='
                                             || coalesce(t.contract #>> '{needs,coder_class}', '?') || ')', ', '), '')
                        from public.tasks t
                       where t.root_id = '$ROOT_ID' and t.state = 'ready'")"
    if [[ -n "${UNCLAIMABLE//[[:space:]]/}" ]]; then
      REASON="задача не берётся парком: ${UNCLAIMABLE}; потолок coder_class=$CEIL_CLASS"
    fi
    write_lesson "$ROOT_ID" "$CRATE" blocked \
      "Виток $ROUND: граф не завершился за ${GRAPH_TIMEOUT_SEC}s. Причина: ${REASON:-не указана}."
    fail "граф не завершился: ${REASON:-не указана}" 2
  fi

  REFS="$(artifact_refs_for_root "$ROOT_ID")"
  if [[ -z "${REFS//[[:space:]]/}" ]]; then
    REASON="$(db "select coalesce(string_agg(coalesce(block_reason,'нет'), '; '), 'нет')
                    from public.tasks where root_id = '$ROOT_ID' and state = 'blocked';")"
    write_lesson "$ROOT_ID" "$CRATE" blocked \
      "Виток $ROUND: ни одна задача графа не одобрена. Причина: ${REASON:-не указана}."
    attempt=$((attempt + 1))
    continue
  fi

  REF=""
  while IFS= read -r cand; do
    [[ -n "$cand" ]] || continue
    if ARTIFACT="$(read_artifact "$cand")" \
       && printf '%s' "$ARTIFACT" | extract_patch > "$PATCH_FILE"; then
      REF="$cand"
      break
    fi
  done <<< "$REFS"

  if [[ -z "$REF" ]]; then
    write_lesson "$ROOT_ID" "$CRATE" blocked \
      "Виток $ROUND: ни один одобренный артефакт графа не содержит применимого unified diff для $TARGET."
    attempt=$((attempt + 1))
    continue
  fi
  log "патч извлечён из артефакта $REF ($(wc -l < "$PATCH_FILE") строк)"

  # -------------------------------------------------------------------------
  # C. Применение и верификация.
  # -------------------------------------------------------------------------
  git fetch -q public main
  BASE_SHA="$(git rev-parse public/main)"
  git checkout -q -B "$BRANCH" "$BASE_SHA"
  if ! git apply --check "$PATCH_FILE" 2>/tmp/self-improve-apply.err; then
    # Точная причина важна: «патч не применился» не говорит следующему витку
    # ничего. Обрезанный hunk — самый частый случай (модель теряет строки на
    # длинном файле), и лечится он требованием компактного ответа.
    if grep -q 'corrupt patch' /tmp/self-improve-apply.err; then
      DECLARED="$(grep -m1 '^@@' "$PATCH_FILE" | sed -n 's/.*+\([0-9]*\).*/\1/p')"
      ACTUAL="$(grep -c '^+' "$PATCH_FILE")"
      write_lesson "$ROOT_ID" "$CRATE" blocked \
        "Виток $ROUND: патч роя обрезан — hunk объявляет $DECLARED строк, в блоке $ACTUAL. " \
        "Следующий виток: файл теста компактнее (<=90 строк), без вспомогательных функций."
    else
      write_lesson "$ROOT_ID" "$CRATE" blocked \
        "Виток $ROUND: патч роя не применяется к $BASE_SHA: $(head -3 /tmp/self-improve-apply.err | tr '\n' ' ')"
    fi
    git checkout -q main
    attempt=$((attempt + 1))
    continue
  fi
  git apply "$PATCH_FILE"
  log "патч применён к $BRANCH (base $BASE_SHA)"

  if ! cargo test -p "$CRATE" --locked > /tmp/self-improve-test.log 2>&1; then
    TAIL="$(grep -E '^(error|test result: FAILED)' /tmp/self-improve-test.log | head -3 | tr '\n' ' ')"
    write_lesson "$ROOT_ID" "$CRATE" blocked \
      "Виток $ROUND: тест роя не проходит после применения: ${TAIL:-см. лог}."
    git checkout -q -- . && git clean -qfd
    git checkout -q main
    attempt=$((attempt + 1))
    continue
  fi

  if ! ./scripts/acceptance_harness.sh --base "$BASE_SHA" --branch HEAD \
        --full-workspace-test --allowlist-file scripts/acceptance_allowlist.txt \
        > /tmp/self-improve-gate.log 2>&1; then
    FAILED="$(grep -E '^FAIL ' /tmp/self-improve-gate.log | head -3 | tr '\n' ' ')"
    write_lesson "$ROOT_ID" "$CRATE" blocked \
      "Виток $ROUND: STRICT-гейт красный после патча роя: ${FAILED:-см. лог}."
    git checkout -q -- . && git clean -qfd
    git checkout -q main
    attempt=$((attempt + 1))
    continue
  fi

  log "STRICT-гейт зелёный"
  git add -A
  git commit -q -F - <<MSG
self-improve(round $ROUND): ${SUBJECT}

Патч произведён РОЕМ, не человеком: постановка подана в swarm-planner,
граф planner → worker → reviewer выполнился штатно, воркер прочитал файлы
${READ_FILES} из SWARM_REPO_DIR и вспомнил уроки из public.agent_memory.

Источник патча — одобренный артефакт задачи графа $ROOT_ID (ref $REF).
Файл: ${TARGET}

Проверка: cargo test -p ${CRATE} --locked; acceptance_harness.sh STRICT.
MSG
  log "коммит: $(git rev-parse --short HEAD)"

  if ((SKIP_MERGE)); then
    log "готово (--skip-merge): ветка $BRANCH"
    exit 0
  fi

  git push -q -u public "$BRANCH"
  PR_URL="$(gh pr create --repo xodapi/swarm-dispatcher-public --base main \
            --head "$BRANCH" --title "self-improve(round $ROUND): ${SUBJECT}" \
            --body "Патч сгенерирован роем (артефакт \`$REF\`), применён и проверен контуром \`scripts/self_improve_loop.sh\`. Виток $ROUND." 2>&1 | tail -1)"
  log "PR: $PR_URL"

  for _ in $(seq 1 40); do
    sleep 20
    if ! gh pr checks "$PR_URL" 2>/dev/null | grep -q pending; then break; fi
  done
  if gh pr checks "$PR_URL" 2>/dev/null | grep -qE 'fail'; then
    write_lesson "$ROOT_ID" "$CRATE" blocked "Виток $ROUND: CI красный на PR $PR_URL."
    fail "CI красный: $PR_URL" 3
  fi
  gh pr merge "$PR_URL" --squash --delete-branch >/dev/null
  log "PR смержен"

  # -------------------------------------------------------------------------
  # D. Рекурсия: пересборка роя на новом коде.
  # -------------------------------------------------------------------------
  git checkout -q main && git pull -q public main
  docker compose up -d --build worker-1 worker-2 reviewer planner > /tmp/self-improve-rebuild.log 2>&1 \
    || fail "пересборка контейнеров упала (см. /tmp/self-improve-rebuild.log)"
  log "рой пересобран на $(git rev-parse --short HEAD)"
  exit 0
done

fail "виток $ROUND не замкнут за $MAX_ATTEMPTS попыток (уроки записаны)" 2
