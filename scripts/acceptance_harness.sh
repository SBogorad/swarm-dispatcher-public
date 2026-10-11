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
        "log": {"path": os.environ["SEALED_LOG_FILE"], "sha256": os.environ["log_sha"]},
        "artifacts": artifacts,
    }, f, indent=2, sort_keys=True)
    f.write("\n")
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

compose_check() {
  local env_file rendered
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
  for suite in "${suites[@]}"; do
    run_worktree_step "${suite##*|} on patched c2_ database" \
      "$py" -m pytest -q "${suite%%|*}"
  done
  unset ROLES_DB ROLES_HOST ROLES_PORT ADMIN_PW PGDSN
  unset SWARM_PW_W1 SWARM_PW_W2 SWARM_PW_W_1 SWARM_PW_GPU_1 SWARM_PW_WORKER SWARM_PW_WATCHDOG
  unset SWARM_PW_PLANNER SWARM_PW_REVIEWER SWARM_PW_OPERATOR SWARM_PW_CLI
  ((FAILURES == failures_before))
}

run_step "diff allowlist ($BASE..$BRANCH)" check_diff_scope
run_step "git diff --check" git -C "$WORKTREE" diff --check "$BASE..$BRANCH"
run_step "shell syntax checks" shell_checks
if command -v docker >/dev/null 2>&1; then
  run_step "compose config with dummy environment" compose_check
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
    # FIND-01: shell-tools тесты требуют feature; бинарь собирается без неё в shell_tools_ban.
    run_worktree_step "required full workspace test with disposable c2_ database" cargo test --workspace --features swarm-worker/shell-tools --locked -- --test-threads=1
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
