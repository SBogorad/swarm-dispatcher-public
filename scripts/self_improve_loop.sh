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

# ---------------------------------------------------------------------------
# Классификация сбоя: HarnessFault vs AgentFault (Reflexion / ExpeL).
#
# Урок в `agent_memory` — это знание роя о СВОИХ ошибках. Если контур пишет
# туда сбой обвязки (не поднялась БД, съеденный stdin, 503 провайдера), рой
# получает суеверный урок о том, чего не делал. Живой случай: баг `exec -T`
# записал «ни один артефакт не содержит diff», хотя патч был, и следующий
# виток учился на галлюцинации контура.
#
# Правило: урок пишется ТОЛЬКО при AGENT_FAULT — патч роя не собрался, уронил
# тест, не затронул все `TARGET_FILES` или отвергнут ревьюером. Всё, что
# сломалось до/вне кода роя, — HARNESS_FAULT: виток останавливается, память
# не трогается.
# ---------------------------------------------------------------------------
FAULT_CLASS=""
agent_fault()   { FAULT_CLASS="AGENT_FAULT"; }
harness_fault() { FAULT_CLASS="HARNESS_FAULT"; }

# Последние строки лога при ошибке: полный лог остаётся в файле, в консоль
# уходит только хвост — иначе длинный прогон взрывает контекст координатора
# (живой `503 system memory overloaded` на 153k токенов).
tail_log() {
  local file="$1" n="${2:-25}"
  [[ -s "$file" ]] || return 0
  printf '[self-improve %s] --- последние %s строк %s ---\n' "$ROUND" "$n" "$file"
  tail -n "$n" "$file" | sed 's/^/  /'
}

# Проводка (Darwin Gödel Machine, Wiring Check): патч обязан затронуть КАЖДЫЙ
# заявленный файл. Живой случай — «мёртвый остров»: рой добавил
# `choose_with_rework` в `model_selector.rs` и тест к нему, все гейты зелёные,
# а боевой `worker.rs` продолжал звать `choose(contract)` с `rework = 0`.
# Механизм существовал и не был подключён. Три артефакта из трёх не трогали
# второй файл, и ничто это не ловило: `git apply --check` проходил.
missing_targets() {
  local patch="$1" f touched
  touched="$(sed -n 's|^+++ b/||p' "$patch" | sort -u)"
  for f in $TARGET_FILES; do
    printf '%s\n' "$touched" | grep -qxF "$f" || printf '%s\n' "$f"
  done
}

# Счётчики тестов из лога `cargo test` — компактная сводка вместо простыни.
test_counts() {
  local file="$1"
  [[ -s "$file" ]] || { printf 'нет лога'; return 0; }
  local passed failed
  passed="$(grep -oE '[0-9]+ passed' "$file" | awk '{s+=$1} END {print s+0}')"
  failed="$(grep -oE '[0-9]+ failed' "$file" | awk '{s+=$1} END {print s+0}')"
  printf '%s passed, %s failed' "$passed" "$failed"
}

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
    MODE="new"
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
    MODE="new"
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
  3)
    MODE="edit"
    CRATE="swarm-reviewer"
    TARGET="crates/swarm-reviewer/src/main.rs"
    SUBJECT="валидация unified diff в артефакте до вынесения вердикта"
    BEHAVIOUR="В \`verify_with_trail\` добавить реальную проверку структуры unified diff. \
Сейчас проверяются только длина текста и Decision Trail: битый патч (заголовок \
\`@@ set -euo pipefail @@\`, неверный счёт строк в ханке) получает \`Approved\`, а \
\`git apply\` падает позже. Добавить функцию \
\`fn validate_unified_diff(body: &str) -> Result<usize, String>\`, которая: \
1) находит блоки, начинающиеся с \`\`\`diff; 2) для каждого требует строку \
\`+++ b/<path>\` и \`--- a/<path>\` либо \`--- /dev/null\`; 3) требует заголовок ханка \
строго вида \`@@ -l,s +l,s @@\` с числами; 4) считает фактические строки тела ханка: \
old = строки, начинающиеся с пробела или '-', new = пробела или '+', и сравнивает \
с объявленными в заголовке; 5) при расхождении возвращает Err с точным описанием: \
номер строки заголовка, объявлено/фактически для old и new; 6) Ok(n) — число \
проверенных блоков. Вызвать её в \`verify_with_trail\` после проверки маркеров \
незавершённой работы и до проверки длины: при Err вернуть \`ChangesRequested\` с \
текстом \`[RUBRIC:DIFF] ...\`. Если diff-блоков нет — проверка не применяется."
    READ_FILES="crates/swarm-reviewer/src/main.rs"
    ;;
  4)
    MODE="edit"
    CRATE="swarm-worker"
    TARGET="crates/swarm-worker/src/model_selector.rs"
    TARGET_FILES="crates/swarm-worker/src/model_selector.rs crates/swarm-worker/src/worker.rs"
    SUBJECT="повышение класса модели при реворке"
    BEHAVIOUR="Сейчас при повторной попытке (\`rework > 0\`) воркер берёт ту же модель, \
на которой уже ошибся. 1) В \`model_selector.rs\` добавить \
\`pub fn choose_with_rework(&self, contract: &serde_json::Value, rework: i32) -> Option<Selection>\`: \
эффективный уровень = \`(task_tier + rework).min(3)\` при \`rework > 0\`, иначе \`task_tier\`; \
выбор идёт по этому уровню. Существующий \`choose\` оставить делегатом \
\`choose_with_rework(contract, 0)\` — публичный API не ломать. 2) В \`worker.rs\` \
прокинуть \`rework\` в \`fn select_model\` и вызывать \`choose_with_rework\`; \
\`call_llm_with_retries\` уже получает \`input.rework\`. В \`reason\` показать подъём: \
\`rework=N tier=T->E\`."
    READ_FILES="crates/swarm-worker/src/model_selector.rs crates/swarm-worker/src/worker.rs"
    ;;
  5)
    MODE="edit"
    CRATE="swarm-worker"
    TARGET="crates/swarm-worker/src/prompt.rs"
    SUBJECT="дедупликация вспоминаемых уроков памяти"
    BEHAVIOUR="Сейчас \`memory_section\` печатает все переданные уроки подряд. Если пять \
задач упали с одной причиной, воркер получает пять копий одного урока и теряет \
остальные. В \`prompt.rs\` дедуплицировать уроки по СМЫСЛУ перед печатью: нормализовать \
текст (нижний регистр, схлопнуть пробелы, отрезать хвост после 'Причина:' и оставить \
первые 80 символов нормализованной причины) и оставить первый (самый свежий) урок \
из каждой группы; порядок остальных сохранить. Реализовать приватной функцией \
\`fn dedup_lessons(lessons: &[swarm_core::MemoryLesson]) -> Vec<&swarm_core::MemoryLesson>\` \
и вызвать в начале \`memory_section\`. При пустом результате секция не печатается."
    READ_FILES="crates/swarm-worker/src/prompt.rs"
    ;;
  6)
    MODE="sql"
    # SQL-виток: `cargo test -p` здесь нечего проверять, миграцию ловит
    # acceptance_harness (SQL-арбитры на одноразовой БД). Пустой CRATE
    # отключает шаг cargo test — иначе он гонял бы посторонний крейт.
    CRATE=""
    TARGET="schema/migrations/018_recall_memory_dedup.sql"
    SUBJECT="дедупликация уроков памяти до LIMIT 5"
    BEHAVIOUR="Функция \`public.task_recall_memory\` (см. schema/migrations/015_agent_memory.sql) \
делает \`order by m.created_at desc, m.id desc limit 5\` и отдаёт воркеру пять САМЫХ СВЕЖИХ \
строк. Если в таблице подряд лежат пять копий одного урока, воркер получает пять копий \
и теряет все остальные уроки: Rust-дедупликация в \`prompt.rs\` схлопывает их уже ПОСЛЕ \
LIMIT. Нужна новая миграция, которая пересоздаёт функцию так, чтобы дедупликация \
происходила ДО ограничения: 1) вычислить смысловой отпечаток урока — \
\`lower(regexp_replace(lesson, '\\s+', ' ', 'g'))\` и обрезать хвост после 'причина:' до 80 \
символов; 2) из каждой группы одинаковых отпечатков оставить САМУЮ СВЕЖУЮ строку \
(\`distinct on (отпечаток) ... order by отпечаток, created_at desc, id desc\`); \
3) отсортировать выжившие по \`created_at desc, id desc\` и вернуть до 5; 4) сохранить \
сигнатуру \`(p_task uuid, p_worker text, p_epoch bigint)\`, \`security definer\`, \
\`set search_path = public, pg_temp\`, проверки \`assert_worker_identity\` и \`assert_lease\`, \
исключение своей задачи (\`m.source_task_id <> p_task\`) и фильтр \`m.kind in (v_kind, '*')\`; \
5) вернуть \`alter function ... owner to swarm_owner\`, \`revoke all ... from public\` и \
\`grant execute ... to swarm_worker\` — без них функция перестанет быть вызываемой \
воркером. Функция должна возвращать 5 РАЗНЫХ по смыслу уроков, даже если в таблице \
десять копий одного."
    READ_FILES="schema/migrations/015_agent_memory.sql schema/migrations/016_agent_memory_feedback.sql schema/migrations/017_block_reason_limit.sql"
    ;;
  *)
    fail "виток $ROUND не описан в скрипте: добавь тему, иначе рой получит бессмысленную задачу"
    ;;
esac

MODE="${MODE:-new}"
TARGET_FILES="${TARGET_FILES:-$TARGET}"

BRANCH="self-improve/round-${ROUND}"
PATCH_DIR="$REPO_ROOT/.self-improve"
PATCH_FILE="$PATCH_DIR/round-${ROUND}.patch"
ARTIFACT_FILE="$PATCH_DIR/round-${ROUND}.artifact.txt"

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

# ---------------------------------------------------------------------------
# Pre-flight (Darwin Gödel Machine): виток не стартует на грязном дереве и не
# просит рой править файлы, которых нет. Оба условия — HARNESS_FAULT: это
# состояние контура, а не ошибка роя, и урок в память не пишется.
# ---------------------------------------------------------------------------
preflight() {
  local dirty missing=()
  # `--dry-run` только печатает постановку и ничего не меняет: требовать от
  # него чистого дерева значит запретить отладку самого контура.
  (( DRY_RUN )) && { log "pre-flight: пропущен (--dry-run)"; return 0; }
  dirty="$(git status --porcelain)"
  if [[ -n "$dirty" ]]; then
    harness_fault
    tail_log <(printf '%s\n' "$dirty") 8
    fail "pre-flight: рабочее дерево грязное — виток не стартует (см. git status)" 4
  fi
  if [[ "$MODE" == "edit" ]]; then
    # Правка: файл обязан быть, иначе патч с контекстом некуда прикладывать.
    for f in $TARGET_FILES; do
      [[ -f "$f" ]] || missing+=("$f")
    done
    if (( ${#missing[@]} > 0 )); then
      harness_fault
      fail "pre-flight: целевых файлов для правки нет в дереве: ${missing[*]}" 4
    fi
    log "pre-flight: дерево чистое, целевые файлы на месте ($(echo $TARGET_FILES | wc -w))"
  else
    # Создание: файла ещё нет — иначе diff `--- /dev/null` не применится.
    if [[ -e "$TARGET" ]]; then
      harness_fault
      fail "pre-flight: $TARGET уже существует — режим $MODE создаёт новый файл" 4
    fi
    log "pre-flight: дерево чистое, $TARGET свободен для создания"
  fi
}

preflight

attempt=1
# Уникальный идентификатор запуска: планировщик идемпотентен по хэшу постановки,
# и без него повтор после падения (например, transport-ошибка LLM) возвращал бы
# уже мёртвый граф. Смена id гарантирует новый граф на каждый запуск.
RUN_ID="$(date +%Y%m%dT%H%M%S)-$$"
mkdir -p "$PATCH_DIR"

if [[ "$MODE" == "sql" ]]; then
  WORK_HEADER="Задача — добавить РОВНО ОДИН новый файл SQL-миграции, который меняет
поведение существующей функции БД. Это не тест и не документация: миграция
пересоздаёт функцию, и её поведение меняется для всех вызовов.
Новых таблиц и колонок не добавляй: миграция обязана применяться на живой схеме."
  FILE_FORM_RULES="- Diff создаёт РОВНО ОДИН новый файл: ${TARGET}. Других файлов в диффе нет.
- Заголовки ровно: \`--- /dev/null\` и \`+++ b/${TARGET}\`.
- Файл КОМПАКТНЫЙ: не более 120 строк, только \`create or replace function\` и
  сопутствующие \`alter\`/\`revoke\`/\`grant\`/\`comment\`.
- Функция должна остаться вызываемой воркером: без \`grant execute\` она
  перестанет работать, и это не поймает ни один локальный тест.
- Заголовок hunk'а (\`@@ -0,0 +1,N @@\`) обязан совпадать с числом строк в блоке:
  пересчитай N перед отправкой. Обрезанный блок не применится и будет отклонён."
elif [[ "$MODE" == "edit" ]]; then
  WORK_HEADER="Задача — ИЗМЕНИТЬ СУЩЕСТВУЮЩИЙ БОЕВОЙ файл: добавить в него реальную
функциональность. Это не тест и не документация: меняется поведение движка.
Правку вноси ТОЛЬКО в перечисленные файлы, остальное не трогай."
  FILE_FORM_RULES="- Diff правит ТОЛЬКО эти существующие файлы (каждый, если их несколько):
$(for f in $TARGET_FILES; do printf '    * %s (строки \`--- a/%s\` и \`+++ b/%s\`)\n' "$f" "$f" "$f"; done)
- Правка каждого файла — своим ханком или своим diff-блоком; пропустить файл
  из списка нельзя: задача считается невыполненной.
- Никаких новых файлов, никаких тестов, никакой документации.
- Контекст — МАКСИМУМ 3 строки до места вставки и 3 после. Тело функции,
  в которую вставляешь, повторять НЕЛЬЗЯ: дублированный контекст отвергается.
- Контекстные строки копируй БАЙТ В БАЙТ из прочитанного файла: \`git apply\`
  сверяет их точно. Если не уверен в строке — не включай её в контекст.
- Якорь выбирай по УНИКАЛЬНОЙ строке рядом с местом вставки, а не по номеру
  строки: номера в заголовке ханка контур пересчитывает сам.
- Заголовок ханка: \`@@ -<старая_стр>,<old_n> +<новая_стр>,<new_n> @@\`, где
  old_n = контекст + удаления, new_n = контекст + добавления. Пересчитай перед отправкой.
- Правка КОМПАКТНАЯ: не более 60 добавленных строк. Одна функция или один блок."
else
  WORK_HEADER="Задача — добавить РОВНО ОДИН новый файл интеграционного теста, который
закрепляет поведение, уже реализованное в коде. Ничего не переписывай и не рефактори."
  FILE_FORM_RULES="- Diff создаёт РОВНО ОДИН новый файл: ${TARGET}. Других файлов в диффе нет.
- Заголовки ровно: \`--- /dev/null\` и \`+++ b/${TARGET}\`.
- Файл КОМПАКТНЫЙ: не более 90 строк. Без вспомогательных функций, без таблиц
  в комментариях, без длинных doc-блоков. Три теста — три функции.
- Заголовок hunk'а (\`@@ -0,0 +1,N @@\`) обязан совпадать с числом строк в блоке:
  пересчитай N перед отправкой. Обрезанный блок не применится и будет отклонён."
fi

TASK_TEXT="$(cat <<EOF
Самосовершенствование роя swarm-dispatcher, виток ${ROUND}.

Ты — часть роя, который улучшает собственный БОЕВОЙ исходный код.
${WORK_HEADER}

ЦЕЛЕВЫЕ ФАЙЛЫ (все обязательны):
$(for f in $TARGET_FILES; do printf '  - %s\n' "$f"; done)
$( [[ -n "$CRATE" ]] && printf 'КРЕЙТ: %s\n' "$CRATE" )ПРЕДМЕТ: ${SUBJECT}

ЧТО ИМЕННО ДОЛЖНО ИЗМЕНИТЬСЯ В КОДЕ:
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
${FILE_FORM_RULES}
- Никаких заглушек и маркеров незавершённой работы в тексте.
$( if [[ -n "$CRATE" ]]; then printf -- '- Код обязан компилироваться под `cargo build -p %s --locked`.\n' "$CRATE"; else printf -- '- Миграция обязана применяться на существующей схеме без ошибок.\n'; fi )
- Используй ТОЛЬКО существующие зависимости; новых не добавляй.
- Не создавай подзадачу-верификатор: сборку выполняет внешний контур.
- После блока diff добавь секцию \`## Decision Trail\` — markdown-таблицу
  со столбцами \`phase | decision | why | evidence | result\`. В КАЖДОЙ строке
  \`evidence\` — проверяемый указатель: путь с файлом (${READ_FILES%% *}),
  $( if [[ -n "$CRATE" ]]; then printf '`cargo build -p %s`' "$CRATE"; else printf '`psql -f <миграция>`'; fi ) или \`rc=0\`. Описательный текст ревьюер отклонит.
- Последняя секция — \`## Handoff\`: одной строкой, что именно изменилось.
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
  local text="$1" rc=0
  # `set -o pipefail` ронял весь скрипт, если planner возвращал ненулевой код
  # (например, валидация отвергла тип проверки). Юнит умирал МОЛЧА: в логе
  # оставались только строки «подаю постановку», а причина — в отдельном
  # planner-логе. Живой случай: `task t1 has disallowed check type "output"`.
  docker compose run --rm --no-deps planner swarm-planner "$text" > /tmp/self-improve-planner.log 2>&1 || rc=$?
  # `graph already exists` приходит с ненулевым кодом, но это НЕ отказ: граф
  # создан прошлой попыткой и готов к работе. Проверяем лог ДО кода возврата,
  # иначе повторная подача навсегда считается провалом планировщика.
  if grep -q 'graph already exists (root_id=' /tmp/self-improve-planner.log; then
    return 0
  fi
  if (( rc != 0 )); then
    log "planner вернул rc=$rc: $(grep -m1 -E 'error:|Error:' /tmp/self-improve-planner.log || echo 'см. /tmp/self-improve-planner.log')"
    return 1
  fi
  # Планировщик идемпотентен по хэшу постановки: повторная подача той же
  # постановки не создаёт второй граф, а печатает `graph already exists
  # (root_id=...)` и НЕ печатает `root_id:`. Живой виток 2 упал именно так:
  # первая попытка создала граф, вторая — получила существующий и умерла с
  # «planner не вернул root_id», хотя граф был готов к работе.
  local root
  root="$(sed -n 's/^root_id: //p' /tmp/self-improve-planner.log | tail -1)"
  if [[ -z "$root" ]]; then
    root="$(sed -n 's/.*graph already exists (root_id=\([0-9a-f-]\{36\}\)).*/\1/p' \
              /tmp/self-improve-planner.log | tail -1)"
    [[ -n "$root" ]] && log "граф уже существует, переиспользую: $root"
  fi
  printf '%s\n' "$root"
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
                                     >= coalesce((t.contract #>> '{needs,coder_class}')::numeric, 0)
                               and (w.capabilities->>'ram_gb')::numeric
                                     >= coalesce((t.contract #>> '{needs,min_ram_gb}')::numeric, 0));")"
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
  # `docker compose exec -T` читает stdin. Вызывающая сторона перебирает
  # кандидатов через `while ... done <<< "$REFS"`, поэтому exec съедал
  # here-string: после первого кандидата цикл завершался. Первым по свежести
  # идёт `code.verify` (в нём diff'а нет), и контур писал ложный урок
  # «ни один артефакт не содержит применимого diff», хотя валидный патч лежал
  # в третьем кандидате. stdin закрываем явно — цикл не должен зависеть от того,
  # читает ли вызываемая команда stdin.
  if docker compose exec -T worker-1 sh -c "test -f /var/swarm/artifacts/${hex}.txt" </dev/null 2>/dev/null; then
    docker compose exec -T worker-1 sh -c "cat /var/swarm/artifacts/${hex}.txt" </dev/null
    return 0
  fi
  vol="$(docker compose config --format json 2>/dev/null \
        | python3 -c 'import json,sys; d=json.load(sys.stdin); print(d["name"]+"_swarm-artifacts")' 2>/dev/null || true)"
  [[ -n "$vol" ]] || return 1
  docker run --rm -i -v "${vol}:/a:ro" alpine:3 sh -c "cat /a/${hex}.txt" </dev/null
}

extract_patch() {
  # $1: файл с артефактом. stdout: unified diff. Ненулевой код — diff не найден.
  # Артефакт читается ИЗ ФАЙЛА, а не из stdin: heredoc `python3 - <<'PY'`
  # занимает stdin под саму программу, поэтому pipe с артефактом молча давал
  # пустое тело. Живой виток 2 записал из-за этого ложный урок «ни один
  # артефакт не содержит применимого diff», хотя diff в артефакте был.
  python3 - "$TARGET" "$1" "${TARGET_FILES:-$TARGET}" <<'PY'
import re
import sys


def normalize_new_file(block):
    """Пересобрать hunk-заголовок для diff'а «новый файл» по фактическим строкам.

    Модель регулярно ошибается в счёте строк: объявляет `@@ -0,0 +1,63 @@`,
    а в блоке 61 строка содержимого. `git apply` отвечает `corrupt patch` и
    отвергает ВЕСЬ патч — хотя текст теста целый. Для diff'а, создающего файл,
    счёт однозначно выводится из тела: это не догадка, а арифметика. Правка
    заголовка не меняет содержимое ни на байт, поэтому не может «подогнать»
    патч под желаемый результат.
    """
    lines = block.rstrip("\n").split("\n")
    if len(lines) < 3 or lines[0].strip() != "--- /dev/null":
        return block if block.endswith("\n") else block + "\n"
    payload = lines[3:]
    if any(l.startswith(("-", "@@", "diff --git")) for l in payload):
        return block if block.endswith("\n") else block + "\n"
    n = sum(1 for l in payload if l.startswith("+"))
    head = [lines[0], lines[1], "@@ -0,0 +1,%d @@" % n]
    return "\n".join(head + payload) + "\n"


def normalize_hunk_counts(block):
    """Пересчитать счётчики единственного ханка по его телу.

    Для ПРАВКИ существующего файла модель тоже ошибается в счёте строк, и
    `git apply` отвергает весь патч (`corrupt patch`). Числа в заголовке
    `@@ -a,b +c,d @@` однозначно выводятся из тела: `b` — строки контекста и
    удаления, `d` — контекст и добавления. Меняем ТОЛЬКО числа, содержимое
    не трогаем ни на байт. Работаем лишь при одном ханке: при нескольких
    непонятно, к какому заголовку относится тело.
    """
    lines = block.rstrip("\n").split("\n")
    heads = [i for i, l in enumerate(lines) if l.startswith("@@")]
    if len(heads) != 1:
        return block if block.endswith("\n") else block + "\n"
    hi = heads[0]
    m = re.match(r"@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@", lines[hi])
    if not m:
        return block if block.endswith("\n") else block + "\n"
    old_start, new_start = m.group(1), m.group(3)
    payload = lines[hi + 1:]
    old_n = sum(1 for l in payload if l[:1] in (" ", "-") and not l.startswith("\\"))
    new_n = sum(1 for l in payload if l[:1] in (" ", "+") and not l.startswith("\\"))
    if (old_n, new_n) == (0, 0):
        return block if block.endswith("\n") else block + "\n"
    lines[hi] = "@@ -%s,%d +%s,%d @@" % (old_start, old_n, new_start, new_n)
    return "\n".join(lines) + "\n"


def repair_context_markers(block):
    """Восстановить потерянный ведущий пробел у контекстных строк ханка.

    Модель иногда копирует контекст без первого пробела: строка `/// Есть ли
    в строке hex-токен` приходит как есть вместо ` /// Есть ли...`. Для
    `git apply` это «unexpected line», и патч отвергается целиком, хотя
    содержимое верное. Внутри тела ханка любая непустая строка обязана
    начинаться с ' ', '+', '-' или '\\'; всё прочее — потерянный маркер
    контекста. Восстановление не меняет содержимое: `git apply` всё равно
    сверит строку с файлом байт в байт, поэтому подогнать результат нельзя.
    """
    lines = block.rstrip("\n").split("\n")
    out, in_hunk = [], False
    for l in lines:
        if l.startswith("@@"):
            in_hunk = True
            out.append(l)
            continue
        if l.startswith(("diff --git", "--- ", "+++ ")):
            in_hunk = False
            out.append(l)
            continue
        if in_hunk and l and l[0] not in " +-\\":
            out.append(" " + l)
        else:
            out.append(l)
    return "\n".join(out) + "\n"


target, path, allowed = sys.argv[1], sys.argv[2], sys.argv[3]
allowed_set = {p for p in allowed.split() if p}
body = open(path, encoding="utf-8", errors="replace").read()
# Закрывающий забор — ТОЛЬКО отдельная строка целиком. Внутри добавленного
# кода законно встречается ``` в тексте (например, doc-комментарий
# «съесть закрывающий ```»), и нежадный поиск обрывал блок на нём: патч
# витка 3 приехал обрезанным на 52 строках из 157 и не применялся.
blocks = re.findall(r"^```(?:diff|patch)?[ \t]*\n(.*?)^```[ \t]*$", body, re.S | re.M)
cands = [b for b in blocks if "+++ b/" in b or b.lstrip().startswith("diff --git")]
if not cands:
    sys.exit(1)
# Блок годится, если он трогает целевой файл и НЕ трогает ничего вне
# разрешённого набора: патч, задевший чужой файл, не должен применяться молча.
for b in cands:
    touched = re.findall(r"^\+\+\+ b/(.+)$", b, re.M)
    if target not in touched:
        continue
    if not set(touched) <= allowed_set:
        continue
    fixed = normalize_new_file(b)
    if fixed == b or not b.lstrip().startswith("--- /dev/null"):
        fixed = normalize_hunk_counts(repair_context_markers(fixed))
    print(fixed, end="")
    sys.exit(0)
sys.exit(2)
PY
}

write_lesson() {
  local task="$1" kind="$2" outcome="$3" text="$4"
  # Reflexion/ExpeL: память роя — знание о СВОИХ ошибках. Сбой обвязки,
  # записанный как урок, — суеверие: следующий виток учится на галлюцинации
  # контура (живой случай — ложный урок «нет diff» из-за съеденного stdin).
  if [[ "${FAULT_CLASS:-}" != "AGENT_FAULT" ]]; then
    log "урок НЕ записан ($outcome $kind): сбой классифицирован как ${FAULT_CLASS:-НЕ КЛАССИФИЦИРОВАН}, память роя не отравляем"
    return 0
  fi
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

# Одноразовая Postgres для шага тестов контура.
#
# `cargo test -p swarm-worker` включает интеграционные тесты
# (`worker_heartbeat_f5`, `worker_integration`): они требуют ЖИВУЮ базу и
# падают с `Connection refused`, если её нет. Виток 4 трижды применил патч
# роя и трижды покраснел на этом шаге — при том что те же тесты падают и на
# чистом `main`. Это дефект контура, а не патча: гейт
# (`acceptance_harness.sh`) поднимает одноразовую БД, а шаг тестов — нет.
#
# Порядок инициализации ровно тот же, что в гейте: schema_v0.sql → roles.sql →
# roles_2b.sql → два патча → миграции по лексикографическому имени.
TEST_DB_CONTAINER=""
TEST_DB_PASSWORD=""

cleanup_test_db() {
  if [[ -n "$TEST_DB_CONTAINER" ]]; then
    docker rm -f "$TEST_DB_CONTAINER" >/dev/null 2>&1 || true
    TEST_DB_CONTAINER=""
  fi
}
trap cleanup_test_db EXIT

provision_test_db() {
  [[ -n "$TEST_DB_CONTAINER" ]] && return 0
  command -v docker >/dev/null 2>&1 || return 1
  TEST_DB_PASSWORD="si_$(openssl rand -hex 20)"
  TEST_DB_CONTAINER="si_test_${USER:-user}_$$"
  TEST_DB_CONTAINER="${TEST_DB_CONTAINER//[^a-zA-Z0-9_.-]/_}"
  docker run -d --name "$TEST_DB_CONTAINER" \
    -e POSTGRES_DB=c2_swarm -e POSTGRES_USER=postgres \
    -e "POSTGRES_PASSWORD=$TEST_DB_PASSWORD" \
    -p 127.0.0.1::5432 \
    -v "$REPO_ROOT/schema/schema_v0.sql:/si/schema.sql:ro" \
    -v "$REPO_ROOT/step2/roles.sql:/si/roles.sql:ro" \
    -v "$REPO_ROOT/step2b/roles_2b.sql:/si/roles_2b.sql:ro" \
    -v "$REPO_ROOT/step2/roles_patch_planner_status.sql:/si/patch_planner.sql:ro" \
    -v "$REPO_ROOT/step2/roles_patch_tool_call_audit.sql:/si/patch_tool_call.sql:ro" \
    -v "$REPO_ROOT/schema/migrations:/si/migrations:ro" \
    postgres:16-alpine >/dev/null || return 1

  local port ready=0
  port="$(docker port "$TEST_DB_CONTAINER" 5432/tcp | awk -F: 'NR == 1 {print $NF}')"
  [[ "$port" =~ ^[0-9]+$ ]] || return 1
  for _ in {1..60}; do
    if docker exec -e PGPASSWORD="$TEST_DB_PASSWORD" "$TEST_DB_CONTAINER" \
      psql -X -U postgres -d c2_swarm -c 'select 1' >/dev/null 2>&1; then
      ready=1
      break
    fi
    sleep 1
  done
  ((ready == 1)) || return 1

  local pg
  pg() { docker exec -e PGPASSWORD="$TEST_DB_PASSWORD" "$TEST_DB_CONTAINER" \
           psql -X -v ON_ERROR_STOP=1 -U postgres -d c2_swarm "$@"; }
  pg -f /si/schema.sql || return 1
  pg -v worker_pw=si_worker -v watchdog_pw=si_watchdog \
     -v planner_pw=si_planner -v reviewer_pw=si_reviewer \
     -v operator_pw=si_operator -v cli_pw=si_cli -f /si/roles.sql || return 1
  pg -v w1_pw=si_w1 -v w2_pw=si_w2 -v w_1_pw=si_w_1 \
     -v gpu_1_pw=si_gpu_1 -f /si/roles_2b.sql || return 1
  pg -f /si/patch_planner.sql || return 1
  pg -f /si/patch_tool_call.sql || return 1
  local mig
  for mig in "$REPO_ROOT"/schema/migrations/*.sql; do
    [ -e "$mig" ] || continue
    pg -f "/si/migrations/$(basename "$mig")" || return 1
  done

  export TEST_DATABASE_URL="host=127.0.0.1 port=${port} user=postgres password=${TEST_DB_PASSWORD} dbname=c2_swarm"
  export DATABASE_URL="$TEST_DATABASE_URL"
  log "тестовая БД поднята (порт $port, контейнер $TEST_DB_CONTAINER)"
}

while (( attempt <= MAX_ATTEMPTS )); do
  log "B: виток $ROUND, попытка $attempt — подаю постановку в planner"
  # Планировщик идемпотентен по хэшу постановки. `TASK_TEXT` собирается до
  # цикла, поэтому без явного суффикса попытка 2 получала тот же граф и тот же
  # отвергнутый артефакт — «повтор с уроком» был фикцией.
  ROOT_ID="$(planner_run "${TASK_TEXT}

Номер этой попытки: ${attempt}. Учти уроки выше: прошлая попытка отвергнута.
Идентификатор запуска: ${RUN_ID}-${attempt}. Он нужен только для того, чтобы
повтор после сбоя не переиспользовал граф, умерший на transport-ошибке.")"
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
    # Таймаут сам по себе — состояние обвязки. Но если задачи графа УСПЕЛИ
    # заблокироваться с причиной (`max_rework`, отказ ревьюера), виноват рой:
    # это AGENT_FAULT, и урок уместен. Различаем по наличию blocked-причин.
    BLOCKED_N="$(db "select count(*) from public.tasks
                      where root_id = '$ROOT_ID' and state = 'blocked'
                        and coalesce(block_reason, '') <> '';" 2>/dev/null || echo 0)"
    if [[ "${BLOCKED_N:-0}" != "0" ]]; then
      agent_fault
    else
      harness_fault
    fi
    write_lesson "$ROOT_ID" "$CRATE" blocked \
      "Виток $ROUND: граф не завершился за ${GRAPH_TIMEOUT_SEC}s. Причина: ${REASON:-не указана}."
    fail "граф не завершился: ${REASON:-не указана}" 2
  fi

  REFS="$(artifact_refs_for_root "$ROOT_ID")"
  if [[ -z "${REFS//[[:space:]]/}" ]]; then
    REASON="$(db "select coalesce(string_agg(coalesce(block_reason,'нет'), '; '), 'нет')
                    from public.tasks where root_id = '$ROOT_ID' and state = 'blocked';")"
    # Ни одна задача не одобрена: ревьюер отверг то, что произвёл рой.
    agent_fault
    write_lesson "$ROOT_ID" "$CRATE" blocked \
      "Виток $ROUND: ни одна задача графа не одобрена. Причина: ${REASON:-не указана}."
    attempt=$((attempt + 1))
    continue
  fi

  REF=""
  while IFS= read -r cand; do
    [[ -n "$cand" ]] || continue
    if ARTIFACT="$(read_artifact "$cand")" \
       && printf '%s' "$ARTIFACT" > "$ARTIFACT_FILE" \
       && extract_patch "$ARTIFACT_FILE" > "$PATCH_FILE"; then
      REF="$cand"
      break
    fi
  done <<< "$REFS"

  if [[ -z "$REF" ]]; then
    # Артефакт одобрен, но diff из него не извлекается — это форма ответа роя.
    # (Исторический ложный случай — съеденный stdin — уже устранён `</dev/null`;
    # если урок снова появится, причина будет именно в артефакте.)
    agent_fault
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
  # `--recount` пересчитывает счётчики ханка, но НЕ отменяет сверку
  # содержимого: подогнать патч под файл он не может.
  if ! git apply --recount --check "$PATCH_FILE" 2>/tmp/self-improve-apply.err; then
    # Точная причина важна: «патч не применился» не говорит следующему витку
    # ничего. Обрезанный hunk — самый частый случай (модель теряет строки на
    # длинном файле), и лечится он требованием компактного ответа.
    agent_fault
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
  # Проводка проверяется ДО применения: патч, не затронувший заявленный файл,
  # оставил бы механизм неподключённым — ровно тот дефект, что дал виток 4.
  MISSING="$(missing_targets "$PATCH_FILE")"
  if [[ -n "${MISSING//[[:space:]]/}" ]]; then
    agent_fault
    write_lesson "$ROOT_ID" "$CRATE" blocked \
      "Виток $ROUND: патч роя не затронул заявленные файлы: $(printf '%s ' $MISSING)." \
      "Следующий виток: правка каждого файла из TARGET_FILES обязательна — без неё механизм остаётся неподключённым."
    log "проводка неполная: не затронуты $(printf '%s ' $MISSING)"
    git checkout -q main
    attempt=$((attempt + 1))
    continue
  fi

  git apply --recount "$PATCH_FILE"
  log "патч применён к $BRANCH (base $BASE_SHA)"

  # `cargo fmt --all` перед гейтом. Рой пишет рабочий код, но не расставляет
  # переносы: виток 3 дважды прошёл `git apply` и компиляцию и дважды покраснел
  # ровно на шаге rustfmt. Форматирование детерминировано и семантику не
  # меняет, поэтому это работа контура, а не повод для ещё одного витка.
  cargo fmt --all

  # Интеграционным тестам крейта нужна живая БД; без неё шаг краснеет
  # независимо от патча роя (проверено на чистом main).
  # Без живой БД шаг тестов краснеет независимо от патча: интеграционные тесты
  # требуют Postgres и падают с `Connection refused`. Это HARNESS_FAULT —
  # виток останавливается, урок в память роя не пишется (иначе рой «выучит»
  # урок про отсутствующий контейнер).
  if ! provision_test_db > /tmp/self-improve-testdb.log 2>&1; then
    harness_fault
    tail_log /tmp/self-improve-testdb.log 15
    git checkout -q -- . && git clean -qfd
    git checkout -q main
    fail "тестовая БД не поднялась — виток остановлен, память роя не тронута" 4
  fi

  # `--test-threads=1`: интеграционные тесты крейта работают с ОДНОЙ базой и
  # в `clear()` транкают таблицы. Параллельный прогон валит их друг другу:
  # `f5_lease_revoked_mid_flight_fails_fast` падал с `left: "review"`,
  # потому что соседний тест успевал завершить его задачу. Гейт гоняет те же
  # тесты с `--test-threads=1` — здесь тот же режим.
  if [[ -z "$CRATE" ]]; then
    log "шаг cargo test пропущен: виток правит SQL, проверку делает STRICT-гейт"
  elif ! cargo test -p "$CRATE" --locked -- --test-threads=1 > /tmp/self-improve-test.log 2>&1; then
    # Патч роя уронил тест — это ошибка агента, урок уместен.
    agent_fault
    TAIL="$(grep -E '^(error|test result: FAILED)' /tmp/self-improve-test.log | head -3 | tr '\n' ' ')"
    log "тесты красные: $(test_counts /tmp/self-improve-test.log)"
    tail_log /tmp/self-improve-test.log 25
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
    agent_fault
    FAILED="$(grep -E '^FAIL ' /tmp/self-improve-gate.log | head -3 | tr '\n' ' ')"
    log "STRICT-гейт красный: ${FAILED:-см. лог}"
    tail_log /tmp/self-improve-gate.log 25
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
    # Локальный гейт зелёный, CI красный — расхождение среды, но источник всё
    # равно патч роя: урок уместен.
    agent_fault
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
