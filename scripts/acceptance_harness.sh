#!/usr/bin/env bash
# Disposable acceptance harness for the schema/RBAC acceptance branch.
# This file intentionally does not use docker compose: the database has no
# named volume and is removed by the EXIT trap on every path.
set -Eeuo pipefail

BASE=""
BRANCH="HEAD"
WORKTREE="$(pwd)"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
FULL_WORKSPACE_TEST=0
ALLOW_BLOCKED=0
LOG_DIR="${TMPDIR:-/tmp}/swarm-dispatcher-acceptance"
CONTAINER=""
DB_PASSWORD=""
LOG_FILE=""
SEALED_LOG_FILE=""
ATTESTATION_FILE=""
EVENT_FILE=""
TEST_NAMES_FILE=""
ALLOWLIST_FILE=""
FAILURES=0
BLOCKED_COUNT=0
DB_READY=0
TEST_DATABASE_URL=""
SANITIZED_TEST_DATABASE_URL=""
TEST_DB_HOST="127.0.0.1"
TEST_DB_PORT=""
TEST_DB_NAME="c2_swarm"
TEST_DB_USER="postgres"
CREDENTIALS_REDACTED=0

usage() {
  cat <<'USAGE'
Usage: scripts/acceptance_harness.sh --base COMMIT [options]

Options:
  --base COMMIT             Required diff comparison base.
  --branch REF              Ref to check (default: HEAD).
  --worktree PATH           Checkout to inspect (default: current directory).
  --allowlist-file PATH     Exact paths/prefixes (path or path/*, one per line).
  --log-dir PATH            Temporary log directory.
  --full-workspace-test     Require and run cargo test --workspace (release requirement).
  --allow-blocked            Diagnostic mode: allow BLOCKED checks and mark gate CONDITIONAL.

Default is a STRICT release gate: omitting --full-workspace-test records that check
as BLOCKED and exits nonzero. Use --allow-blocked only for diagnostics.
  -h, --help                Show this help.
USAGE
}

while (($#)); do
  case "$1" in
    --base) BASE="${2:?--base requires a commit}"; shift 2 ;;
    --branch) BRANCH="${2:?--branch requires a ref}"; shift 2 ;;
    --worktree) WORKTREE="${2:?--worktree requires a path}"; shift 2 ;;
    --allowlist-file) ALLOWLIST_FILE="${2:?--allowlist-file requires a path}"; shift 2 ;;
    --log-dir) LOG_DIR="${2:?--log-dir requires a path}"; shift 2 ;;
    --full-workspace-test) FULL_WORKSPACE_TEST=1; shift ;;
    --allow-blocked) ALLOW_BLOCKED=1; shift ;;
    -h|--help) usage; exit 0 ;;
    *) echo "FAIL unknown option: $1" >&2; usage >&2; exit 2 ;;
  esac
done

if [[ -z "$BASE" ]]; then
  echo "FAIL --base is required" >&2
  usage >&2
  exit 2
fi

# Нулевой SHA — маркер «нет базы». GitHub при первом пуше ветки передаёт
# `github.event.before = 0000000000000000000000000000000000000000` (`pull_request.base.sha`
# на push-событии пуст), и harness сравнивал ПОЛНУЮ историю репозитория вместо
# свежего диффа. Гейт краснел на pre-existing whitespace в файлах, которых ветка
# не касалась (push-прогон 38831042846: `git diff --check` на всей истории).
# Заменяем маркер на общий предок с целевой веткой — это и есть настоящая база.
if [[ "$BASE" =~ ^0+$ ]]; then
  # Базовая ветка: origin/HEAD -> дефолт репозитория (main в публичном репо,
  # master в старом). Жёсткий `origin/master` ломал гейт после переименования
  # ветки: merge-base возвращал пусто и harness падал с exit 2.
  BASE_REF="${BASE_REF:-$(git -C "$WORKTREE" symbolic-ref --quiet --short refs/remotes/origin/HEAD 2>/dev/null || true)}"
  if [[ -z "$BASE_REF" ]]; then
    for cand in origin/main origin/master; do
      if git -C "$WORKTREE" rev-parse --verify --quiet "$cand" >/dev/null 2>&1; then
        BASE_REF="$cand"; break
      fi
    done
  fi
  RESOLVED_BASE=""
  [[ -n "$BASE_REF" ]] && RESOLVED_BASE="$(git -C "$WORKTREE" merge-base "$BASE_REF" "$BRANCH" 2>/dev/null || true)"
  if [[ -z "$RESOLVED_BASE" ]]; then
    echo "FAIL --base is all-zeros and merge-base ${BASE_REF:-<no default branch>} $BRANCH is unresolved" >&2
    usage >&2
    exit 2
  fi
  echo "note: --base was all-zeros; resolved to merge-base $BASE_REF $BRANCH = $RESOLVED_BASE"
  BASE="$RESOLVED_BASE"
fi

WORKTREE="$(cd "$WORKTREE" && pwd)"
mkdir -p "$LOG_DIR"
LOG_FILE="$LOG_DIR/harness-$(date -u +%Y%m%dT%H%M%SZ)-$$.log"
SEALED_LOG_FILE="${LOG_FILE%.log}.sealed.log"
ATTESTATION_FILE="${LOG_FILE%.log}.attestation.json"
EVENT_FILE="${LOG_FILE%.log}.events.tsv"
# Волна 5.2: отдельный артефакт с КОНКРЕТНЫМИ именами тестов (cargo/pytest),
# иначе в аттестации были лишь названия шагов (замечание Claude).
TEST_NAMES_FILE="${LOG_FILE%.log}.tests.txt"
touch "$EVENT_FILE"
touch "$TEST_NAMES_FILE"
exec > >(tee -a "$LOG_FILE") 2>&1

status() {
  local state="$1" label="$2" exit_code="${3:-0}" command="${4:-}"
  printf '%s %s\n' "$state" "$label"
  printf '%s\t%s\t%s\t%s\n' "$state" "$exit_code" "$label" "$command" >>"$EVENT_FILE"
  if [[ "$state" == FAIL ]]; then
    FAILURES=$((FAILURES + 1))
  elif [[ "$state" == BLOCKED ]]; then
    BLOCKED_COUNT=$((BLOCKED_COUNT + 1))
    if ((ALLOW_BLOCKED == 0)); then
      FAILURES=$((FAILURES + 1))
    fi
  fi
  return 0
}

run_step() {
  local label="$1"; shift
  printf 'RUN %s\n' "$label"
  local command_text
  command_text="$(printf '%q ' "$@")"
  printf 'COMMAND %s\n' "$command_text"
  set +e
  "$@"
  local rc=$?
  set -e
  if ((rc == 0)); then
    status PASS "$label" "$rc" "$command_text"
  else
    status FAIL "$label (see log)" "$rc" "$command_text"
  fi
  return 0
}

run_worktree_step() {
  local label="$1"; shift
  printf 'RUN %s\n' "$label"
  local command_text
  command_text="(cd $(printf '%q' "$WORKTREE") && $(printf '%q ' "$@"))"
  printf 'COMMAND %s\n' "$command_text"
  local out_file rc
  out_file="$(mktemp)"
  set +e
  # tee: вывод остаётся виден в логе прогона И сохраняется для извлечения имён тестов.
  (cd "$WORKTREE" && "$@") 2>&1 | tee "$out_file"
  rc=${PIPESTATUS[0]}
  set -e
  # Волна 5.2: в артефакт аттестации добавляем КОНКРЕТНЫЕ имена тестов, а не
  # только название шага. Berём строки cargo (`test <name> ... ok/FAILED`)
  # и pytest (`<file>::<test>`, сводку). Пустые/служебные отфильтровываем.
  if [[ -n "$TEST_NAMES_FILE" ]]; then
    {
      printf '### %s\n' "$label"
      grep -E '^test .+ \.\.\. (ok|FAILED|ignored)$' "$out_file" \
        | sed -E 's/^test //; s/ \.\.\. .*$//' || true
      # pytest с -rA печатает строки вида `PASSED path::test` / `FAILED path::test`
      # в короткой сводке. Забираем и полные идентификаторы `path::test`.
      grep -E '^(PASSED|FAILED|ERROR) .+::' "$out_file" || true
      grep -E '^(PASSED|FAILED|ERROR) ' "$out_file" || true
    } >>"$TEST_NAMES_FILE"
  fi
  rm -f "$out_file"
  if ((rc == 0)); then
    status PASS "$label" "$rc" "$command_text"
  else
    status FAIL "$label (see log)" "$rc" "$command_text"
  fi
  return 0
}

write_attestation() {
  local base_sha head_sha branch_name timestamp rustc cargo pg_version image_ref image_digest log_sha release_gate
  base_sha="$(git -C "$WORKTREE" rev-parse "$BASE" 2>/dev/null || printf 'UNRESOLVED')"
  head_sha="$(git -C "$WORKTREE" rev-parse "$BRANCH" 2>/dev/null || printf 'UNRESOLVED')"
  branch_name="$(git -C "$WORKTREE" symbolic-ref --short -q HEAD 2>/dev/null || printf '%s' "$BRANCH")"
  timestamp="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  rustc="$(rustc -Vv 2>/dev/null || printf 'UNAVAILABLE')"
  cargo="$(cargo -V 2>/dev/null || printf 'UNAVAILABLE')"
  pg_version="${PG_VERSION:-UNAVAILABLE}"
  image_ref="postgres:16-alpine"
  image_digest="$(docker image inspect --format '{{index .RepoDigests 0}}' "$image_ref" 2>/dev/null || printf 'UNAVAILABLE')"
  log_sha="$(sha256sum "$SEALED_LOG_FILE" | awk '{print $1}')"
  release_gate="$( ((ALLOW_BLOCKED == 1)) && printf 'CONDITIONAL' || printf 'STRICT' )"
  export ATTESTATION_FILE base_sha head_sha branch_name timestamp rustc cargo pg_version image_ref image_digest log_sha release_gate BLOCKED_COUNT FULL_WORKSPACE_TEST TEST_DB_HOST TEST_DB_PORT TEST_DB_NAME TEST_DB_USER SANITIZED_TEST_DATABASE_URL CREDENTIALS_REDACTED EVENT_FILE SEALED_LOG_FILE TEST_NAMES_FILE
  python3 - <<'PY'
import json, os

def read_tests(path):
    """Список «шаг -> имена тестов» из TEST_NAMES_FILE (Волна 5.2)."""
    suites, cur = [], None
    try:
        with open(path, encoding="utf-8") as f:
            for line in f:
                line = line.rstrip("\n")
                if line.startswith("### "):
                    cur = {"label": line[4:], "tests": []}
                    suites.append(cur)
                elif cur is not None and line:
                    cur["tests"].append(line)
    except FileNotFoundError:
        pass
    return suites

def sha256_of(path):
    """Хеш файла или None, если его нет."""
    try:
        import hashlib
        h = hashlib.sha256()
        with open(path, "rb") as fh:
            for chunk in iter(lambda: fh.read(65536), b""):
                h.update(chunk)
        return h.hexdigest()
    except FileNotFoundError:
        return None

events = []
with open(os.environ["EVENT_FILE"], encoding="utf-8") as f:
    for line in f:
        state, code, label, command = line.rstrip("\n").split("\t", 3)
        events.append({"status": state, "exit_code": int(code), "label": label, "command": command})
test_suites = read_tests(os.environ.get("TEST_NAMES_FILE", ""))
# Волна 5.3: честная статистика имён. Группы пересекаются (lib-юниты входят
# в full-workspace-прогон), поэтому сумма строк > числа уникальных. Приводим оба,
# чтобы «N тестов» нельзя было прочитать как «N разных покрытых случаев».
_all = [t for s in test_suites for t in s["tests"]]
_unique = sorted(set(_all))
test_counts = {
    "rows_total": len(_all),
    "unique_names": len(_unique),
    "duplicates": len(_all) - len(_unique),
    "note": "rows_total суммирует группы (возможны пересечения); unique_names — честное число разных имён",
}
artifacts = [{"path": os.environ["SEALED_LOG_FILE"], "sha256": os.environ["log_sha"]}]
tests_path = os.environ.get("TEST_NAMES_FILE", "")
if tests_path and sha256_of(tests_path):
    artifacts.append({"path": tests_path, "sha256": sha256_of(tests_path)})
with open(os.environ["ATTESTATION_FILE"], "w", encoding="utf-8") as f:
    json.dump({
        "format": "swarm-dispatcher-acceptance-attestation/v1",
        "base_sha": os.environ["base_sha"], "branch": os.environ["branch_name"],
        "head_sha": os.environ["head_sha"], "timestamp_utc": os.environ["timestamp"],
        "release_gate": os.environ["release_gate"],
        "blocked_count": int(os.environ["BLOCKED_COUNT"]),
        "full_workspace_test_requested": bool(int(os.environ["FULL_WORKSPACE_TEST"])),
        "toolchain": {"rustc_Vv": os.environ["rustc"], "cargo": os.environ["cargo"]},
        "postgres": {"server_version": os.environ["pg_version"], "image": os.environ["image_ref"], "image_digest": os.environ["image_digest"]},
        "test_database": {"dsn": os.environ.get("SANITIZED_TEST_DATABASE_URL", "UNAVAILABLE"), "host": os.environ.get("TEST_DB_HOST", "UNAVAILABLE"), "port": os.environ.get("TEST_DB_PORT", "UNAVAILABLE"), "db": os.environ.get("TEST_DB_NAME", "UNAVAILABLE"), "user": os.environ.get("TEST_DB_USER", "UNAVAILABLE")},
        "credentials_redacted": bool(int(os.environ["CREDENTIALS_REDACTED"])),
        "statuses": events,
        "test_names": test_suites,
        "test_counts": test_counts,
        "log": {"path": os.environ["SEALED_LOG_FILE"], "sha256": os.environ["log_sha"]},
        "artifacts": artifacts,
    }, f, indent=2, sort_keys=True)
    f.write("\n")
    # Волна 5.3: печатаем честную статистику имён в стандартный вывод harness,
    # чтобы «N тестов» нельзя было прочитать по-разному.
    print("[test-names] rows_total=%d unique=%d duplicates=%d" % (
        test_counts["rows_total"], test_counts["unique_names"], test_counts["duplicates"]))
PY
}

cleanup() {
  local rc=$?
  if [[ -n "$CONTAINER" ]] && command -v docker >/dev/null 2>&1; then
    docker rm -f "$CONTAINER" >/dev/null 2>&1 || true
    printf 'PASS disposable container cleaned: %s\n' "$CONTAINER"
  fi
  PG_VERSION="${PG_VERSION:-UNAVAILABLE}"
  if ((rc != 0 || FAILURES != 0)); then
    printf 'FAIL acceptance harness completed with %d failure(s); log: %s; attestation: %s\n' "$FAILURES" "$LOG_FILE" "$ATTESTATION_FILE"
  else
    printf 'PASS acceptance harness complete; log: %s; attestation: %s\n' "$LOG_FILE" "$ATTESTATION_FILE"
  fi
  # Remove the ephemeral credential before sealing generated artifacts.
  if [[ -n "$DB_PASSWORD" && -f "$LOG_FILE" ]]; then
    sed -i "s|$DB_PASSWORD|[REDACTED]|g" "$LOG_FILE"
    if grep -Fq "$DB_PASSWORD" "$LOG_FILE"; then
      printf 'FAIL temporary database password remains in log\n'
      FAILURES=$((FAILURES + 1))
    else
      CREDENTIALS_REDACTED=1
      printf 'PASS temporary database password absent from log\n'
    fi
  fi
  # Seal the complete sanitized log before generating JSON; attestation generation emits no log output.
  cp -- "$LOG_FILE" "$SEALED_LOG_FILE"
  write_attestation
  exit "$((rc != 0 ? rc : (FAILURES > 0 ? 1 : 0)))"
}
trap cleanup EXIT

check_diff_scope() {
  local path rule prefix matched
  if [[ -n "$ALLOWLIST_FILE" ]]; then
    [[ -f "$ALLOWLIST_FILE" ]] || { printf 'allowlist file not found: %s\n' "$ALLOWLIST_FILE"; return 1; }
  fi
  while IFS= read -r path; do
    [[ -z "$path" ]] && continue
    matched=0
    if [[ -n "$ALLOWLIST_FILE" ]]; then
      while IFS= read -r rule || [[ -n "$rule" ]]; do
        rule="${rule%%#*}"
        rule="$(printf '%s' "$rule" | sed 's/^[[:space:]]*//; s/[[:space:]]*$//')"
        [[ -z "$rule" ]] && continue
        if [[ "$rule" == */\* ]]; then
          prefix="${rule%/*}/"
          [[ "$path" == "$prefix"* ]] && matched=1
        else
          [[ "$path" == "$rule" ]] && matched=1
        fi
        ((matched)) && break
      done < "$ALLOWLIST_FILE"
    else
      [[ "$path" == scripts/acceptance_harness.sh || "$path" == docs/audit/* ]] && matched=1
    fi
    ((matched)) || { printf 'out-of-scope path: %s\n' "$path"; return 1; }
  done < <(git -C "$WORKTREE" diff --name-only "$BASE..$BRANCH")
}

shell_checks() {
  local file
  bash -n "$SCRIPT_DIR/acceptance_harness.sh"
  while IFS= read -r file; do
    bash -n "$file"
  done < <(find "$WORKTREE/scripts" -maxdepth 1 -type f -name '*.sh' -print)
  if command -v shellcheck >/dev/null 2>&1; then
    shellcheck "$SCRIPT_DIR/acceptance_harness.sh"
  else
    status BLOCKED "shellcheck unavailable (bash -n passed)"
  fi
}

# Страж сканера секретов (регресс 2026-10-17).
#
# `scripts/secret_scan.sh` стоит в pre-commit/pre-push хуках и блокирует коммит
# при утечке. Дефект: при отсутствии `.env*` (свежий worktree/клон) `mapfile`
# давал массив из одного пустого элемента, а `grep -F -- ""` совпадал с любой
# строкой диффа — скан рапортовал ложную утечку (sha8=e3b0c442 — sha256 пустой
# строки) и блокировал ЛЮБОЙ коммит. Шаг гейта доказывает, что self-test
# сканера краснеет на фейк-секрете И молчит без `.env*`.
# Fail-closed: self-test не прошёл → FAIL.
secret_scan_self_test() {
  local out rc
  out="$(bash "$WORKTREE/scripts/secret_scan.sh" --self-test 2>&1)"
  rc=$?
  if ((rc != 0)); then
    printf '%s\n' "$out"
    status FAIL "secret scanner self-test failed (ложная утечка или пропуск фейка)" "$rc" \
      "scripts/secret_scan.sh --self-test"
    return 0
  fi
  status PASS "secret scanner self-test (фейк ловится, без .env* молчит)"
}

# rustfmt guard (техдолг, ночная смена 2026-10-15).
#
# `cargo fmt --check` был КРАСНЫМ на master, потому что его не запускал ни CI
# (компонент rustfmt ставится, но шага нет), ни harness. PR #38 привёл HEAD к
# канону, но последующие #45/#51 внесли новый неотформатированный код
# (`prompt.rs`, `worker.rs`), и это никто не заметил — ровно класс «проверки,
# которой нет». Теперь форматирование — шаг гейта.
#
# Fail-closed: rustfmt отсутствует → BLOCKED (не «зелено»). Отдельно важно, что
# `cargo fmt` без установленного rustfmt завершается 0, ничего не проверяя, —
# поэтому сначала проверяем `cargo fmt --version`, иначе шаг был бы вечно зелёным.
# Версии не флейки: rustfmt 1.94.1 (пин CI, dtolnay/rust-toolchain@1.94.1) и
# 1.99.0 (локальный) дают байт-идентичный вывод на этом дереве (проверено
# 2026-10-15: одинаковые 6 диффов до фикса, одинаковый результат после).
fmt_check() {
  local out rc ver
  if ! ver="$(cargo fmt --version 2>/dev/null)"; then
    status BLOCKED "rustfmt unavailable: cargo fmt --check not run"
    return 0
  fi
  set +e
  out="$(cd "$WORKTREE" && cargo fmt --check 2>&1)"
  rc=$?
  set -e
  if ((rc == 0)); then
    status PASS "rustfmt formatting ($ver)" 0 "cargo fmt --check"
  else
    printf '%s\n' "$out"
    status FAIL "rustfmt: код не отформатирован (cargo fmt --check, см. log)" "$rc" "cargo fmt --check"
  fi
}

# Техдолг (2026-10-17): clippy не входил в гейт, поэтому мёртвый код и
# предупреждения компилятора доезжали до main незамеченными (так проскочила
# `fn verify` в swarm-reviewer, PR #5). Fail-closed: нет subcommand clippy —
# BLOCKED, а не «зелено».
clippy_check() {
  local out rc ver
  if ! ver="$(cargo clippy --version 2>/dev/null)"; then
    status BLOCKED "clippy unavailable: cargo clippy not run"
    return 0
  fi
  set +e
  out="$(cd "$WORKTREE" && cargo clippy --workspace --all-targets --locked -- -D warnings 2>&1)"
  rc=$?
  set -e
  if ((rc == 0)); then
    status PASS "clippy ($ver)" 0 "cargo clippy --workspace --all-targets --locked -- -D warnings"
  else
    printf '%s\n' "$out"
    status FAIL "clippy: предупреждения компилятора (cargo clippy -D warnings, см. log)" "$rc" "cargo clippy --workspace --all-targets --locked -- -D warnings"
  fi
}

# FIND-01 (Волна 5.2, переработан): после снятия feature-гейта модули shell-tools
# компилируются ВСЕГДА, и символы модуля физически живут в бинаре (мангалинг:
# `swarm_worker7sandbox`, а не `swarm_worker::sandbox`). Поэтому старая
# негативная проверка по `::`-форме была ВАКУУМНОЙ (grep 0 совпадений всегда,
# даже при утечке) — Claude верно поймал это.
#
# Новая цель шага: доказать, что единственная защита вместо compile-гейта —
# runtime-гейт `WORKER_SHELL_TOOLS` — РЕАЛЬНО вкомпилирована в бинарь.
# Положительный контроль: строка отказа гейта обязана присутствовать.
# Плюс негативный контроль в корректной форме: символы `agent_loop`/`allowlist`
# НЕ должны попадать в бинарь (эти модули не используются воркером по умолчанию).
shell_tools_ban() {
  local bin="$WORKTREE/target/release/swarm-worker"
  if ! (cd "$WORKTREE" && cargo build --release -p swarm-worker --bin swarm-worker --locked >/dev/null 2>&1); then
    status FAIL "FIND-01: cargo build swarm-worker (no shell-tools) failed" 1 "cargo build --release -p swarm-worker --bin swarm-worker"
    return 0
  fi
  if [ ! -f "$bin" ]; then
    status FAIL "FIND-01: swarm-worker binary missing after build" 1 "ls $bin"
    return 0
  fi

  # 1) ПОЛОЖИТЕЛЬНЫЙ КОНТРОЛЬ (главный): runtime-гейт вкомпилирован.
  # Именно эта строка — актуальная защита вместо compile-гейта.
  #
  # ВАЖНО (Волна 5.2): НЕ используем `strings | grep -q` — при `set -o pipefail`
  # `grep -q` выходит на первом совпадении, `strings` получает SIGPIPE, и пайп
  # возвращает ненулевой код даже при успешном grep (ложный FAIL). Сначала
  # сбрасываем вывод strings в переменную, потом грепаем ПО ПЕРЕМЕННОЙ.
  local gate_string='shell-tools disabled (set WORKER_SHELL_TOOLS=1 to enable)'
  local bin_strings
  bin_strings="$(strings "$bin")"
  if ! grep -qF "$gate_string" <<<"$bin_strings"; then
    status FAIL "FIND-01: runtime-gate string absent in binary (gate NOT compiled in)" 1 \
      "strings swarm-worker | grep -F '$gate_string'"
    return 0
  fi

  # 2) НЕГАТИВНЫЙ КОНТРОЛЬ в КОРРЕКТНОЙ форме мангалинга.
  # `agent_loop`/`allowlist` не должны линковаться в бинарь воркера: воркер
  # не запускает agent loop по умолчанию. Проверяем legacy-форму, которую
  # реально порождает rustc (а не `::`, где grep всегда давал 0).
  local hit=0 sym
  for sym in swarm_worker9agent_loop swarm_worker9allowlist agent_system_prompt run_shell WORKER_ALLOWED_COMMANDS; do
    if grep -q "$sym" <<<"$bin_strings"; then
      hit=1
      printf 'FIND-01 BAN VIOLATION: %s found in swarm-worker binary\n' "$sym"
    fi
  done
  if ((hit != 0)); then
    status FAIL "FIND-01: shell-tools leaked into swarm-worker binary" 1 "strings swarm-worker | grep -E '<banned-legacy>|gate-string'"
    return 0
  fi

  status PASS "FIND-01: runtime-gate present + shell-tools modules absent" 0 "strings swarm-worker | grep -F '$gate_string'"
}

# Волна 5.2 (fail-closed): гарантия, что интеграция реально исполняется.
#
# Инцидент (найден ночным прогоном): harness-шаг --full-workspace-test
# собирал `cargo test --workspace` без `--ignored`, поэтому 21 интеграционный
# тест молча игнорировался, а гейт оставался зелёным. Чтобы это не повторилось,
# явно проверяем: в файле имён тестов есть имена интеграционных тестов из
# worker_sandbox (sandbox_*/agent_loop_*), и их не меньше ожидаемого минимума.
check_integration_not_ignored() {
  local min_expected=8 n
  n="$(grep -cE '^(sandbox|agent_loop)_' "$TEST_NAMES_FILE" 2>/dev/null || true)"
  n="${n:-0}"
  if ((n < min_expected)); then
    status FAIL "integration tests executed only ${n} (min ${min_expected}); --ignored may be silently skipping" 1 \
      "grep -cE '^(sandbox|agent_loop)_' ${TEST_NAMES_FILE}"
    return 0
  fi
  status PASS "integration tests executed: ${n} (>= ${min_expected})" 0 \
    "grep -cE '^(sandbox|agent_loop)_' ${TEST_NAMES_FILE}"
}

# Волна 5.2: backup/restore арбитр (KNOWN_LIMITATIONS 5.1).
# Дамп disposable-БД восстанавливается в новую БД и проходит verify_chain.
backup_restore_check() {
  [[ "$DB_READY" == 1 ]] || { status FAIL "backup/restore requires provisioned DB" 1 ""; return 0; }
  local tmp
  tmp="$(mktemp -d)"
  # DATABASE_URL для ops/db_backup.sh (postgres://-форма).
  local url="postgres://postgres:${DB_PASSWORD}@127.0.0.1:${TEST_DB_PORT}/${TEST_DB_NAME}"
  if ! (cd "$WORKTREE" && DATABASE_URL="$url" ./ops/db_backup.sh dump "$tmp") >/dev/null 2>&1; then
    status FAIL "backup/restore: dump failed" 1 "ops/db_backup.sh dump"
    rm -rf "$tmp"; return 0
  fi
  local dump="" f
  for f in "$tmp"/*.dump; do
    [[ -e "$f" ]] && dump="$f"
  done
  if [[ -z "$dump" ]]; then
    status FAIL "backup/restore: no dump produced" 1 "ops/db_backup.sh dump"
    rm -rf "$tmp"; return 0
  fi
  if (cd "$WORKTREE" && DATABASE_URL="$url" ./ops/db_backup.sh verify "$dump") >/dev/null 2>&1; then
    status PASS "backup/restore round-trip verified" 0 "ops/db_backup.sh verify"
  else
    status FAIL "backup/restore: verify failed" 1 "ops/db_backup.sh verify"
  fi
  rm -rf "$tmp"
}

compose_check() {  local env_file rendered
  env_file="$(mktemp)"
  cat >"$env_file" <<'ENV'
DB_PASSWORD=dummy_db_password
SWARM_PW_WORKER=dummy_worker_password
SWARM_PW_WATCHDOG=dummy_watchdog_password
SWARM_PW_PLANNER=dummy_planner_password
SWARM_PW_REVIEWER=dummy_reviewer_password
SWARM_PW_OPERATOR=dummy_operator_password
SWARM_PW_CLI=dummy_cli_password
SWARM_PW_W1=dummy_w1_password
SWARM_PW_W2=dummy_w2_password
SWARM_PW_W_1=dummy_w_1_password
SWARM_PW_GPU_1=dummy_gpu_1_password
WORKER_LLM_API_KEY=dummy_llm_api_key
ENV
  rendered="$(mktemp)"
  docker compose --env-file "$env_file" -f "$WORKTREE/docker-compose.yml" config >"$rendered" 2>/dev/null || true
  rm -f "$env_file"
  [[ -s "$rendered" ]] || { rm -f "$rendered"; return 1; }
  RENDERED_COMPOSE_FILE="$rendered" compose_security_acceptance
  local rc=$?
  rm -f "$rendered"
  return $rc
}

# P0-3: pin the worker container boundary in a verifiable check, not just docs.
compose_security_acceptance() {
  python3 - <<'PY'
import os, sys, yaml
with open(os.environ["RENDERED_COMPOSE_FILE"], encoding="utf-8") as f:
    cfg = yaml.safe_load(f) or {}
services = cfg.get("services", {})
workers = [n for n in services if n.startswith("worker-")]
def fail(msg):
    print(f"FAIL {msg}")
    sys.exit(1)
if not workers:
    fail("no worker-* service in compose")
for name in sorted(workers):
    svc = services[name]
    if svc.get("read_only") is not True:
        fail(f"{name}: read_only must be true")
    if "ALL" not in (svc.get("cap_drop") or []):
        fail(f"{name}: cap_drop must include ALL")
    sec = svc.get("security_opt") or []
    if not any("no-new-privileges" in s for s in sec):
        fail(f"{name}: no-new-privileges missing")
    for vol in svc.get("volumes") or []:
        src = vol if isinstance(vol, str) else (vol.get("source") or "")
        if "docker.sock" in str(src):
            fail(f"{name}: docker.sock must not be mounted by default")
    if svc.get("privileged") is True:
        fail(f"{name}: privileged must not be set")
    if svc.get("network_mode") == "host":
        fail(f"{name}: host network mode not allowed")
print("PASS worker container boundary pinned: " + ", ".join(sorted(workers)))

# Issue #24: DSN каждого сервиса обязан соответствовать его БД-роли.
# Иначе сервис стартует, но падает на правах (watchdog под swarm_worker_w1
# не имеет reclaim_expired/promote_ready -> цикл задач стоит).
def get_env(svc, key):
    env = svc.get("environment")
    if isinstance(env, dict):
        return env.get(key)
    for e in env or []:
        if str(e).startswith(key + "="):
            return str(e)[len(key) + 1:]
    return None

def service_db_role(name, svc):
    if name == "watchdog":
        return "swarm_watchdog"
    if name == "reviewer":
        return "swarm_reviewer"
    if name == "cli":
        return "swarm_cli"
    if name == "planner":
        return "swarm_planner"
    if name.startswith("worker-"):
        # Роль = swarm_worker_<normalize(WORKER_ID)>; см. roles_2b.sql.
        # normalize: lowercase + дефис -> подчёркивание ('w-1' -> 'w_1').
        wid = get_env(svc, "WORKER_ID")
        if not wid:
            fail(f"{name}: WORKER_ID missing (cannot derive DB role)")
        return "swarm_worker_" + str(wid).lower().replace("-", "_")
    return None

role_checked = []
for name in sorted(services):
    expected = service_db_role(name, services[name])
    if expected is None:
        continue
    dsn = get_env(services[name], "DATABASE_URL")
    if not dsn:
        fail(f"{name}: DATABASE_URL missing (expected role {expected})")
    # postgres://<user>:<pw>@host:port/db
    try:
        userinfo = dsn.split("://", 1)[1].split("@", 1)[0]
        user = userinfo.split(":", 1)[0]
    except IndexError:
        fail(f"{name}: malformed DATABASE_URL")
    if user != expected:
        fail(f"{name}: DATABASE_URL role is '{user}', expected '{expected}'")
    role_checked.append(f"{name}={user}")
if role_checked:
    print("PASS service DSN matches DB role: " + ", ".join(role_checked))
PY
}

pg() {
  local user="$1" password="$2"; shift 2
  docker exec -e PGPASSWORD="$password" "$CONTAINER" psql -X -v ON_ERROR_STOP=1 -U "$user" -d c2_swarm "$@"
}

expect_failure() {
  local label="$1"; shift
  if "$@" >/dev/null 2>&1; then
    status FAIL "$label unexpectedly succeeded"
    return 1
  fi
  status PASS "$label"
}

provision_database() {
  local failures_before=$FAILURES
  command -v docker >/dev/null 2>&1
  DB_PASSWORD="c2_$(openssl rand -hex 20)"
  CONTAINER="c2_acceptance_${USER:-user}_$$"
  CONTAINER="${CONTAINER//[^a-zA-Z0-9_.-]/_}"
  local common_args=(
    --name "$CONTAINER" -e POSTGRES_DB=c2_swarm -e POSTGRES_USER=postgres
    -e "POSTGRES_PASSWORD=$DB_PASSWORD"
    -p 127.0.0.1::5432
    -v "$WORKTREE/schema/schema_v0.sql:/c2/schema.sql:ro"
    -v "$WORKTREE/step2/roles.sql:/c2/roles.sql:ro"
    -v "$WORKTREE/step2b/roles_2b.sql:/c2/roles_2b.sql:ro"
    -v "$WORKTREE/step2/roles_patch_planner_status.sql:/c2/patch_planner.sql:ro"
    -v "$WORKTREE/step2/roles_patch_tool_call_audit.sql:/c2/patch_tool_call.sql:ro"
    -v "$WORKTREE/schema/migrations:/c2/migrations:ro"
  )
  docker run -d "${common_args[@]}" postgres:16-alpine >/dev/null
  PG_VERSION="$(docker exec "$CONTAINER" postgres --version)"
  TEST_DB_PORT="$(docker port "$CONTAINER" 5432/tcp | awk -F: 'NR == 1 {print $NF}')"
  [[ "$TEST_DB_PORT" =~ ^[0-9]+$ ]]
  TEST_DATABASE_URL="host=localhost port=${TEST_DB_PORT} user=postgres password=${DB_PASSWORD} dbname=${TEST_DB_NAME}"
  SANITIZED_TEST_DATABASE_URL="host=localhost port=${TEST_DB_PORT} user=postgres password=[REDACTED] dbname=${TEST_DB_NAME}"
  [[ "$TEST_DATABASE_URL" == *"password=${DB_PASSWORD}"* ]]
  [[ "$SANITIZED_TEST_DATABASE_URL" != *"${DB_PASSWORD}"* ]]
  local ready=0
  for _ in {1..60}; do
    if docker exec -e PGPASSWORD="$DB_PASSWORD" "$CONTAINER" \
      psql -X -U postgres -d c2_swarm -c 'select 1' >/dev/null 2>&1; then
      ready=1
      break
    fi
    sleep 1
  done
  ((ready == 1))

  # Apply exactly the repository's accepted order, with ON_ERROR_STOP on each file.
  pg postgres "$DB_PASSWORD" -f /c2/schema.sql
  pg postgres "$DB_PASSWORD" -v worker_pw=c2_worker -v watchdog_pw=c2_watchdog \
    -v planner_pw=c2_planner -v reviewer_pw=c2_reviewer -v operator_pw=c2_operator \
    -v cli_pw=c2_cli -f /c2/roles.sql
  pg postgres "$DB_PASSWORD" -v w1_pw=c2_w1 -v w2_pw=c2_w2 -v w_1_pw=c2_w_1 \
    -v gpu_1_pw=c2_gpu_1 -f /c2/roles_2b.sql
  pg postgres "$DB_PASSWORD" -f /c2/patch_planner.sql
  pg postgres "$DB_PASSWORD" -f /c2/patch_tool_call.sql
  # Миграции применяются тем же порядком, что и в продакшн-цепочке
  # (scripts/05-migrations.sh): лексикографически по имени файла. Без них в
  # тест-БД отсутствует llm_calls, и воркер с cost ledger (Issue #2/#3) не
  # проходит даже базовый сценарий — ложная регрессия в harness, а не в коде.
  local mig
  for mig in "$WORKTREE"/schema/migrations/*.sql; do
    [ -e "$mig" ] || continue
    pg postgres "$DB_PASSWORD" -f "/c2/migrations/$(basename "$mig")" || return 1
  done
  DB_READY=1
  status PASS "disposable PostgreSQL 16 provisioned and initialized"
}

database_acceptance() {
  local failures_before=$FAILURES
  [[ "$DB_READY" == 1 ]]

  local common_worker_login worker_login
  common_worker_login="$(pg postgres "$DB_PASSWORD" -Atc "select rolcanlogin from pg_roles where rolname='swarm_worker'")"
  worker_login="$(pg postgres "$DB_PASSWORD" -Atc "select rolcanlogin from pg_roles where rolname='swarm_worker_w1'")"
  if [[ "$common_worker_login" == f ]]; then
    status PASS "common swarm_worker is NOLOGIN"
  else
    status FAIL "common swarm_worker must be NOLOGIN (P0 RBAC fix is absent from this branch)"
  fi
  if [[ "$worker_login" == t ]]; then
    status PASS "per-worker role swarm_worker_w1 is LOGIN"
  else
    status FAIL "per-worker role swarm_worker_w1 must be LOGIN"
  fi

  pg swarm_worker_w1 c2_w1 -c "select register_worker('w1', '{\"ram_gb\":16}'::jsonb)"
  status PASS "per-worker register_worker works"
  expect_failure "worker identity spoofing is rejected" pg swarm_worker_w1 c2_w1 -c "select register_worker('w2', '{\"ram_gb\":16}'::jsonb)"
  expect_failure "cli claim_task is denied" pg swarm_cli c2_cli -c "select claim_task('w1', 10)"
  expect_failure "SET ROLE spoofing is denied" pg swarm_worker_w1 c2_w1 -c "set role swarm_worker_w2"
  ((FAILURES == failures_before))
}

review_gate_acceptance() {
  local failures_before=$FAILURES
  [[ "$DB_READY" == 1 ]]
  local suite
  local suites=(
    "step2/test_roles.py|RBAC role separation + catalog invariants (P0 #3/#4)"
    "step2b/test_roles_2b.py|worker identity, SET ROLE, id collisions (P0 #4)"
    "step2/test_review_gate.py|review-gate negative tests (P0 #5)"
    "tests/test_v0.py|state machine: lease/epoch, add_dep, reopen cascade (P0 #6/#7)"
    "tests/test_cost_reconciliation.py|cost ledger reconciliation vs relay.fast (Issue #2)"
    "tests/test_f8_orphan_llm_spend.py|F8 budget protects real spend, not only complete (Issue 5.10)"
    # FIND-NEW-04: три сьюта лежали мёртвым грузом мимо harness и CI.
    # test_f8 уже выше; ниже — остальные два (юнит-части, без сети/БД).
    "tests/test_model_quality.py|model quality view -> selector stats (Issue #7/#10)"
    "tests/test_model_catalog.py|relay.fast pricing -> USD/1M recalculation (Issue #7/#10)"
    # Ночная смена (2026-10-17, §5.3): реестр применённых миграций и раннер для
    # ЖИВОГО тома. Без этого шага раннер остался бы непроверенным кодом (класс
    # «проверки, которой нет»), а миграции на живой том накатывались бы вручную.
    "tests/test_migration_ledger_runner.py|миграционный реестр: initdb-путь и живой том (§5.3)"
    # RSI v2: классификация сбоя контура (HarnessFault vs AgentFault). Урок в
    # `agent_memory` пишется только при AGENT_FAULT — иначе сбой обвязки
    # становится «опытом» роя. Статический страж, БД не требует.
    "tests/test_harness_fault_classification.py|классификация сбоев контура: HARNESS_FAULT vs AGENT_FAULT (RSI v2)"
    # Полнота ИТОГОВОЙ схемы (2026-10-20): полная цепочка с нуля применяется
    # без ошибок, каталог совпадает с ожидаемым (37 функций / 10 таблиц), каждая
    # миграция отмечена в реестре, RPC handoff и памяти на месте. Ловит класс
    # «миграция есть в файлах, но до схемы не доехала» (так 015–018 не попали
    # на живой том). Поведенческий: нужна живая disposable-БД (PGDSN).
    "tests/test_schema_chain_completeness.py|полнота схемы: цепочка 001-019 -> 37 функций / 10 таблиц"
  )
  for suite in "${suites[@]}"; do
    [[ -f "$WORKTREE/${suite%%|*}" ]] || { status BLOCKED "missing test suite: ${suite%%|*}"; return 0; }
  done
  local py="$WORKTREE/.venv/bin/python"
  if [[ ! -x "$py" ]]; then
    if command -v uv >/dev/null 2>&1; then
      (cd "$WORKTREE" && uv venv --quiet .venv && uv pip install --quiet --python .venv/bin/python pytest psycopg2-binary) || true
    fi
  fi
  if [[ ! -x "$py" ]]; then
    status BLOCKED "python venv with pytest+psycopg2 unavailable (install uv or .venv)"
    return 0
  fi
  export ROLES_DB=c2_swarm ROLES_HOST=127.0.0.1 ROLES_PORT=$TEST_DB_PORT ADMIN_PW="$DB_PASSWORD"
  export SWARM_PW_W1=c2_w1 SWARM_PW_W2=c2_w2 SWARM_PW_W_1=c2_w_1 SWARM_PW_GPU_1=c2_gpu_1
  export SWARM_PW_WORKER=c2_worker SWARM_PW_WATCHDOG=c2_watchdog SWARM_PW_PLANNER=c2_planner
  export SWARM_PW_REVIEWER=c2_reviewer SWARM_PW_OPERATOR=c2_operator SWARM_PW_CLI=c2_cli
  # tests/test_v0.py speaks PGDSN (superuser): reuse the disposable DSN.
  export PGDSN="dbname=c2_swarm user=postgres host=127.0.0.1 port=$TEST_DB_PORT password=$DB_PASSWORD"
  # Волна 5.2: арбитры волны 4 (F15/F4/FIND-04/F2) требуют W4_*-окружения.
  # Без него conftest их блокировал (BLOCKED), и 38 тестов НИКОГДА не
  # исполнялись ни harness, ни ручным прогоном (найдено ночной сессией).
  export W4_ADMIN_DSN="host=127.0.0.1 port=$TEST_DB_PORT user=postgres password=$DB_PASSWORD dbname=c2_swarm"
  export W4_W1_PW=c2_w1 W4_W2_PW=c2_w2
  for suite in "${suites[@]}"; do
    run_worktree_step "${suite##*|} on patched c2_ database" \
      "$py" -m pytest -q -rA "${suite%%|*}"
  done
  # Волна 5.2: волна-4 арбитры — отдельным шагом с W4_*-окружением.
  if [[ -f "$WORKTREE/tests/test_contract_form_f15.py" ]]; then
    run_worktree_step "wave-4 PG arbiters (F15/F4/FIND-04/F2) on patched c2_ database" \
      "$py" -m pytest -q -rA \
        tests/test_contract_form_f15.py tests/test_fail_unblock_f4.py \
        tests/test_find04_record_tool_call_identity.py tests/test_worker_identity_f3.py \
        tests/test_review_gate_f2.py tests/test_review_gate_wave4.py \
        tests/test_dag_handoff_iteration3.py tests/test_agent_memory_015.py \
        tests/test_agent_memory_017.py tests/test_agent_memory_018.py
  fi
  unset ROLES_DB ROLES_HOST ROLES_PORT ADMIN_PW PGDSN W4_ADMIN_DSN W4_W1_PW W4_W2_PW
  unset SWARM_PW_W1 SWARM_PW_W2 SWARM_PW_W_1 SWARM_PW_GPU_1 SWARM_PW_WORKER SWARM_PW_WATCHDOG
  unset SWARM_PW_PLANNER SWARM_PW_REVIEWER SWARM_PW_OPERATOR SWARM_PW_CLI
  ((FAILURES == failures_before))
}

run_step "diff allowlist ($BASE..$BRANCH)" check_diff_scope
run_step "git diff --check" git -C "$WORKTREE" diff --check "$BASE..$BRANCH"
run_step "shell syntax checks" shell_checks
# Страж сканера секретов (2026-10-17): доказываем, что pre-commit-хук ловит
# фейк-секрет и НЕ даёт ложную блокировку без `.env*`. Дешёвый шаг без БД.
run_step "secret scanner self-test" secret_scan_self_test
# rustfmt guard (техдолг 2026-10-15): держим `cargo fmt --check` зелёным на HEAD.
# Дешёвый шаг без БД/Docker — сразу после синтаксиса, до дорогой сборки.
if command -v cargo >/dev/null 2>&1; then
  run_step "rustfmt formatting (cargo fmt --check)" fmt_check
  run_step "clippy linting (cargo clippy -D warnings)" clippy_check
else
  status BLOCKED "cargo unavailable: rustfmt check not run"
fi
# Техдолг (2026-10-14): drift guard против расхождения тел функций между
# schema/schema_v0.sql и step2b/roles_2b.sql. roles_2b.sql перекрывает пять
# функций воркера (добавляя identity-проверку); если кто-то поправит тело в
# одном файле и забудет в другом — тест краснеет. Класс дефекта уже дважды
# кусал: submit_review (F16г) и complete_task (Issue #20).
# Тест не требует БД. Окружение готовим ТЕМ ЖЕ venv, что и python-арбитры:
# pytest + psycopg2-binary обязаны стоять вместе в .venv, иначе предыдущий
# шаг «rbac/python suites» создал бы .venv с одним pytest и без psycopg2 →
# все DB-арбитры падают на ImportError (регрессия PR #39, доказана в CI
# run 38439933042). Единый провижн устраняет расхождение.
_dg_py="$WORKTREE/.venv/bin/python"
if [[ ! -x "$_dg_py" ]]; then
  if command -v uv >/dev/null 2>&1; then
    (cd "$WORKTREE" && uv venv --quiet .venv \
       && uv pip install --quiet --python .venv/bin/python pytest psycopg2-binary) || true
  fi
fi
if [[ -x "$_dg_py" ]]; then
  run_worktree_step "duplicate function-body drift guard (schema_v0 vs roles_2b)" \
    .venv/bin/python -m pytest -q tests/test_no_duplicate_function_bodies.py
  # Техдолг (2026-10-16, PR #3): drift guard против гниения ссылок `file:line`
  # в docs/KNOWN_LIMITATIONS.md. При аудите нашлись ссылки на удалённые файлы и
  # уехавшие строки. Тест статический, БД не требует — тот же venv.
  run_worktree_step "KNOWN_LIMITATIONS doc-ref drift guard" \
    .venv/bin/python -m pytest -q tests/test_known_limitations_refs.py
  # PR #4: демо-воркеры обязаны быть под своими ролями БД (не под суперпользователем
  # swarm), иначе assert_worker_identity в демо обходится. Статический разбор compose.
  run_worktree_step "demo worker role isolation guard" \
    .venv/bin/python -m pytest -q tests/test_demo_worker_roles.py
  # Ночная смена (2026-10-17, Q3): тот же класс гнили, что в PR #3, но во всём
  # живом корпусе документов — 59 битых ссылок, из них 34 реальных дефекта
  # (документы уехали в docs/optional/, integration.rs переименован в
  # integration_verified.rs, путь монтирования вместо репозиторного). Страж —
  # статический, БД не требует; замороженные срезы (docs/audit, docs/history)
  # вне скоупа, живой индекс docs/history/README.md — внутри.
  run_worktree_step "docs link drift guard (live docs)" \
    .venv/bin/python -m pytest -q tests/test_docs_link_drift.py
  # FIND-NEW-02 (ночная смена 2026-10-17): identity-проверка не должна теряться
  # при перекрытии функций миграциями. Миграции применяются ПОСЛЕ roles_2b.sql
  # и пересоздают тела (`create or replace`): 011 — claim_task/complete_task,
  # 012/014 — task_rework_feedback. Снятие identity-строки в перекрытии
  # ПЕРЕЖИВАЛО весь набор арбитров (зонд на disposable-БД: SURVIVED для
  # claim_task и complete_task), а 012 её вовсе не воспроизвела → чужой воркер
  # читал диагностику ревью чужой задачи. Страж статический, БД не требует.
  run_worktree_step "migration identity-override drift guard" \
    .venv/bin/python -m pytest -q tests/test_migration_identity_preserved.py
else
  status BLOCKED "drift guard blocked: .venv with pytest+psycopg2 unavailable"
fi
# Issue #1: архитектурные проверки ast-grep. `test` проверяет сами правила по
# фикстурам (падение → exit 4), `scan --error` — код репозитория (находка → exit 1).
# Fail-closed: без инструмента гейт не «зелёный», а BLOCKED (как cargo/docker).
if command -v ast-grep >/dev/null 2>&1; then
  run_worktree_step "ast-grep rule fixtures (Issue #1)" ast-grep test --skip-snapshot-tests
  run_worktree_step "ast-grep architecture scan (Issue #1)" ast-grep scan --error
else
  status BLOCKED "ast-grep unavailable: architecture checks not run"
fi
if command -v docker >/dev/null 2>&1; then
  run_step "compose config with dummy environment" compose_check
  # Арбитр ролей: self-test проверяет, что арбитр КРАСНЕЕТ на подмене роли
  # (класс #24: watchdog под воркерской ролью). Чистые функции, стека не
  # требует → безопасно в CI. Живой прогон (по контейнерам) — операционная
  # проверка: scripts/compose_roles_arbiter.sh (см. docs/AGENTS.md).
  run_step "compose roles arbiter self-test (#24)" "$WORKTREE/scripts/compose_roles_arbiter.sh" --self-test
  run_step "provision disposable PostgreSQL 16 before Rust tests" provision_database
else
  status BLOCKED "docker unavailable: compose config and database acceptance not run"
fi
if command -v cargo >/dev/null 2>&1; then
  run_worktree_step "Rust library/unit tests" cargo test --workspace --lib --locked -- --test-threads=1
  run_step "FIND-01 shell-tools symbol ban (worker binary)" shell_tools_ban
else
  status BLOCKED "cargo unavailable: Rust unit tests not run"
fi
if ((FULL_WORKSPACE_TEST)); then
  if ((DB_READY == 1)); then
    export TEST_DATABASE_URL
    export DATABASE_URL="$TEST_DATABASE_URL"
    # Волна 5.2: два шага вместо одного — релизный гейт должен реально ВИДЕТЬ
    # интеграцию. Раньше `cargo test --workspace` без `--ignored` молча
    # игнорировал 21 интеграционный тест (все Docker/PG-тесты), т.е.
    # --full-workspace-test завышал уверенность (найдено ночным прогоном).
    # FIND-01: shell-tools тесты требуют feature; бинарь без неё собирается в shell_tools_ban.
    run_worktree_step "required full workspace test with disposable c2_ database" \
      cargo test --workspace --features swarm-worker/shell-tools --locked -- --test-threads=1
    # Интеграция: `--ignored` при поднятой БД (требует TEST_DATABASE_URL).
    # Тесты сами падают (FIND-08), если DSN не задан — не молчаливый skip.
    run_worktree_step "required integration tests (--ignored) on disposable c2_ database" \
      cargo test --workspace --features swarm-worker/shell-tools --locked -- --test-threads=1 --ignored
    # Волна 5.2 (fail-closed): убедиться, что шаг --ignored ФАКТИЧЕСКИ выполнил
    # интеграционные тесты, а не молча их пропустил. Защита от возврата дефекта:
    # предыдущий инцидент — 21 тест игнорировался, а гейт был "зелёным".
    check_integration_not_ignored
    # Волна 5.2: backup/restore арбитр (охват KNOWN_LIMITATIONS 5.1). Дамп
    # disposable-БД восстанавливается в новую БД + verify_chain по задачам.
    run_step "backup/restore round-trip (ops/db_backup.sh verify)" backup_restore_check
  else
    status BLOCKED "full workspace test blocked: disposable c2_ database is not ready"
  fi
else
  status BLOCKED "full workspace test not requested (use --full-workspace-test for release gate)"
fi
if command -v docker >/dev/null 2>&1; then
  run_step "RBAC smoke on disposable c2_ database" database_acceptance
  run_step "RBAC/path/review Python suites on disposable c2_ database" review_gate_acceptance
fi
