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
LOG_DIR="${TMPDIR:-/tmp}/swarm-dispatcher-acceptance"
CONTAINER=""
DB_PASSWORD=""
LOG_FILE=""
ATTESTATION_FILE=""
EVENT_FILE=""
FAILURES=0

usage() {
  cat <<'USAGE'
Usage: scripts/acceptance_harness.sh --base COMMIT [options]

Options:
  --base COMMIT             Required diff comparison base.
  --branch REF              Ref to check (default: HEAD).
  --worktree PATH           Checkout to inspect (default: current directory).
  --log-dir PATH            Temporary log directory.
  --full-workspace-test     Also run cargo test --workspace (optional, explicit).
  -h, --help                Show this help.
USAGE
}

while (($#)); do
  case "$1" in
    --base) BASE="${2:?--base requires a commit}"; shift 2 ;;
    --branch) BRANCH="${2:?--branch requires a ref}"; shift 2 ;;
    --worktree) WORKTREE="${2:?--worktree requires a path}"; shift 2 ;;
    --log-dir) LOG_DIR="${2:?--log-dir requires a path}"; shift 2 ;;
    --full-workspace-test) FULL_WORKSPACE_TEST=1; shift ;;
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
ATTESTATION_FILE="${LOG_FILE%.log}.attestation.json"
EVENT_FILE="${LOG_FILE%.log}.events.tsv"
touch "$EVENT_FILE"
exec > >(tee -a "$LOG_FILE") 2>&1

status() {
  local state="$1" label="$2" exit_code="${3:-0}" command="${4:-}"
  printf '%s %s\n' "$state" "$label"
  printf '%s\t%s\t%s\t%s\n' "$state" "$exit_code" "$label" "$command" >>"$EVENT_FILE"
  if [[ "$state" == FAIL ]]; then
    FAILURES=$((FAILURES + 1))
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

write_attestation() {
  local base_sha head_sha branch_name timestamp rustc cargo pg_version image_ref image_digest log_sha
  base_sha="$(git -C "$WORKTREE" rev-parse "$BASE" 2>/dev/null || printf 'UNRESOLVED')"
  head_sha="$(git -C "$WORKTREE" rev-parse "$BRANCH" 2>/dev/null || printf 'UNRESOLVED')"
  branch_name="$(git -C "$WORKTREE" symbolic-ref --short -q HEAD 2>/dev/null || printf '%s' "$BRANCH")"
  timestamp="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  rustc="$(rustc -Vv 2>/dev/null || printf 'UNAVAILABLE')"
  cargo="$(cargo -V 2>/dev/null || printf 'UNAVAILABLE')"
  pg_version="${PG_VERSION:-UNAVAILABLE}"
  image_ref="postgres:16-alpine"
  image_digest="$(docker image inspect --format '{{index .RepoDigests 0}}' "$image_ref" 2>/dev/null || printf 'UNAVAILABLE')"
  log_sha="$(sha256sum "$LOG_FILE" | awk '{print $1}')"
  export ATTESTATION_FILE base_sha head_sha branch_name timestamp rustc cargo pg_version image_ref image_digest log_sha EVENT_FILE LOG_FILE
  python3 - <<'PY'
import json, os
events = []
with open(os.environ["EVENT_FILE"], encoding="utf-8") as f:
    for line in f:
        state, code, label, command = line.rstrip("\n").split("\t", 3)
        events.append({"status": state, "exit_code": int(code), "label": label, "command": command})
with open(os.environ["ATTESTATION_FILE"], "w", encoding="utf-8") as f:
    json.dump({
        "format": "swarm-dispatcher-acceptance-attestation/v1",
        "base_sha": os.environ["base_sha"], "branch": os.environ["branch_name"],
        "head_sha": os.environ["head_sha"], "timestamp_utc": os.environ["timestamp"],
        "toolchain": {"rustc_Vv": os.environ["rustc"], "cargo": os.environ["cargo"]},
        "postgres": {"server_version": os.environ["pg_version"], "image": os.environ["image_ref"], "image_digest": os.environ["image_digest"]},
        "statuses": events,
        "log": {"path": os.environ["LOG_FILE"], "sha256": os.environ["log_sha"]},
        "artifacts": [{"path": os.environ["LOG_FILE"], "sha256": os.environ["log_sha"]}],
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
  # No output follows this call, so the recorded log hash covers the complete log.
  write_attestation
  exit "$((rc != 0 ? rc : (FAILURES > 0 ? 1 : 0)))"
}
trap cleanup EXIT

check_diff_scope() {
  local path
  while IFS= read -r path; do
    [[ -z "$path" ]] && continue
    case "$path" in
      scripts/acceptance_harness.sh|docs/audit/*) ;;
      *) printf 'out-of-scope path: %s\n' "$path"; return 1 ;;
    esac
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

compose_check() {
  local env_file
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
  docker compose --env-file "$env_file" -f "$WORKTREE/docker-compose.yml" config >/dev/null
  rm -f "$env_file"
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

database_acceptance() {
  local failures_before=$FAILURES
  command -v docker >/dev/null 2>&1
  DB_PASSWORD="c2_$(openssl rand -hex 20)"
  CONTAINER="c2_acceptance_${USER:-user}_$$"
  CONTAINER="${CONTAINER//[^a-zA-Z0-9_.-]/_}"
  local common_args=(
    --name "$CONTAINER" -e POSTGRES_DB=c2_swarm -e POSTGRES_USER=postgres
    -e "POSTGRES_PASSWORD=$DB_PASSWORD"
    -v "$WORKTREE/schema/schema_v0.sql:/c2/schema.sql:ro"
    -v "$WORKTREE/step2/roles.sql:/c2/roles.sql:ro"
    -v "$WORKTREE/step2b/roles_2b.sql:/c2/roles_2b.sql:ro"
    -v "$WORKTREE/step2/roles_patch_planner_status.sql:/c2/patch_planner.sql:ro"
    -v "$WORKTREE/step2/roles_patch_tool_call_audit.sql:/c2/patch_tool_call.sql:ro"
  )
  docker run -d "${common_args[@]}" postgres:16-alpine >/dev/null
  PG_VERSION="$(docker exec "$CONTAINER" postgres --version | sed 's/^postgres (PostgreSQL /postgresql-/' | sed 's/).*$//')"
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

run_step "diff allowlist ($BASE..$BRANCH)" check_diff_scope
run_step "git diff --check" git -C "$WORKTREE" diff --check "$BASE..$BRANCH"
run_step "shell syntax checks" shell_checks
if command -v docker >/dev/null 2>&1; then
  run_step "compose config with dummy environment" compose_check
else
  status BLOCKED "docker unavailable: compose config and database acceptance not run"
fi
if command -v cargo >/dev/null 2>&1; then
  run_step "Rust library/unit tests" cargo test --workspace --lib --locked
else
  status BLOCKED "cargo unavailable: Rust unit tests not run"
fi
if ((FULL_WORKSPACE_TEST)); then
  run_step "optional full workspace test" cargo test --workspace --locked
else
  status BLOCKED "full workspace test skipped (use --full-workspace-test to opt in)"
fi
if command -v docker >/dev/null 2>&1; then
  run_step "disposable PostgreSQL 16 schema/RBAC acceptance" database_acceptance
fi
