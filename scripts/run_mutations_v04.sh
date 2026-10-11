#!/usr/bin/env bash
# run_mutations_v04.sh — мутационное тестирование v0.4 (ТЗ §9, M1–M6).
#
# Каждая мутация ломает ровно одну защиту; ожидается, что соответствующий тест
# ПАДАЕТ. Мутация, которую тесты не заметили, = выживший мутант.
#
# Использование:
#   TEST_DATABASE_URL="host=127.0.0.1 port=55440 user=swarm password=pw dbname=v0sandbox" \
#   SANDBOX_IMAGE=agent-sandbox:1.85 ./scripts/run_mutations_v04.sh
#
# Скрипт сам восстанавливает файлы (backup + trap).

set -uo pipefail
cd "$(dirname "$0")/.."

: "${TEST_DATABASE_URL:=host=127.0.0.1 port=55440 user=swarm password=pw dbname=v0sandbox}"
: "${SANDBOX_IMAGE:=agent-sandbox:1.85}"
export TEST_DATABASE_URL SANDBOX_IMAGE

# ---------------------------------------------------------------------------
# Environment pre-conditions (аудит: тестовая честность).
#
# Мутационный прогон, который не может подключиться к БД или собрать тесты,
# помечает ЛЮБУЮ мутацию как CAUGHT (тест «упал» = компиляция/connect упали).
# Это ложноположительный результат. Поэтому: pre-condition FAIL => BLOCKED
# (не PASS/FAIL), и скрипт выходит, ничего не инвертируя.
# ---------------------------------------------------------------------------
db_name() {
  local dsn="$1" name
  if [[ "$dsn" == postgres://* || "$dsn" == postgresql://* ]]; then
    name="${dsn#*://}"; name="${name#*/}"; name="${name%%/*}"; name="${name%%\?*}"
  else
    local tok
    for tok in $dsn; do [[ "$tok" == dbname=* ]] && name="${tok#dbname=}"; done
  fi
  printf '%s' "$name"
}

blocked() { echo "BLOCKED $1"; exit 3; }

db="$(db_name "$TEST_DATABASE_URL")"
case "$db" in
  test_*|v0*|c2_*) : ;;
  *) blocked "non-disposable database '$db' (allowed prefixes: test_, v0, c2_) — refusing to run mutations" ;;
esac
export PGPASSWORD="$(printf '%s' "$TEST_DATABASE_URL" | sed -n 's/.*password=\([^ ]*\).*/\1/p')"
pg_host="$(printf '%s' "$TEST_DATABASE_URL" | sed -n 's/.*host=\([^ ]*\).*/\1/p')"
pg_port="$(printf '%s' "$TEST_DATABASE_URL" | sed -n 's/.*port=\([^ ]*\).*/\1/p')"
pg_user="$(printf '%s' "$TEST_DATABASE_URL" | sed -n 's/.*user=\([^ ]*\).*/\1/p')"
if ! pg_isready -h "${pg_host:-127.0.0.1}" -p "${pg_port:-5432}" -U "${pg_user:-postgres}" \
     -d "$db" >/dev/null 2>&1; then
  blocked "Postgres not reachable at ${pg_host:-127.0.0.1}:${pg_port:-5432} db=$db"
fi
if ! docker image inspect "$SANDBOX_IMAGE" >/dev/null 2>&1; then
  blocked "sandbox image '$SANDBOX_IMAGE' not present locally"
fi
# Один прогон без мутаций: baseline должен быть ЗЕЛЁНЫМ (иначе любой CAUGHT — артефакт).
if ! cargo test -p swarm-worker --lib --locked >/tmp/mut_baseline.log 2>&1; then
  blocked "baseline 'cargo test -p swarm-worker --lib' is red before any mutation (see /tmp/mut_baseline.log)"
fi
echo "pre-conditions OK: db=$db image=$SANDBOX_IMAGE baseline=green"

W=crates/swarm-worker/src
BACKUP_DIR=$(mktemp -d)
declare -a TOUCHED=()

restore() {
  for f in "${TOUCHED[@]:-}"; do
    [ -n "$f" ] && cp -f "$BACKUP_DIR/$(basename "$f")" "$f" 2>/dev/null
  done
  rm -rf "$BACKUP_DIR"
}
trap restore EXIT

snapshot() {
  local f="$1"
  cp -f "$f" "$BACKUP_DIR/$(basename "$f")"
  TOUCHED+=("$f")
}

pass=0; caught=0; survived=0
declare -a RESULTS=()

run_mutation() {
  local id="$1" desc="$2" file="$3" sed_expr="$4" test_cmd="$5" expect_test="$6"
  snapshot "$file"
  sed -i "$sed_expr" "$file"
  # убедимся, что мутация вообще применилась
  if cmp -s "$file" "$BACKUP_DIR/$(basename "$file")"; then
    RESULTS+=("$id  SKIP  мутация не применилась ($desc)")
    restore_one "$file"
    return
  fi
  if eval "$test_cmd" >/tmp/mut_$id.log 2>&1; then
    RESULTS+=("$id  SURVIVED  $desc  (тест $expect_test не упал)")
    survived=$((survived+1))
  else
    RESULTS+=("$id  CAUGHT    $desc  (тест $expect_test упал, как ожидалось)")
    caught=$((caught+1))
  fi
  restore_one "$file"
}

restore_one() {
  cp -f "$BACKUP_DIR/$(basename "$1")" "$1"
}

LIB_TEST='cargo test -p swarm-worker --lib --locked'
# Волна 5.2: INT_TEST обязан включать --ignored. Docker/PG-тесты в
# worker_sandbox.rs помечены #[ignore] (N2, Волна 5.1: некомпилируемость без
# live-стенда), поэтому без --ignored они ИГНОРИРУЮТСЯ — и мутации M2/M4/M5/M6
# ложно «выживают» (тест просто не запускается). Мутационный прогон требует
# живого Docker+PG (это проверяет pre-conditions выше), значит --ignored уместен.
INT_TEST='cargo test -p swarm-worker --test worker_sandbox --locked -- --test-threads=1 --ignored'

# M1 — убрать проверку allowlist целиком (check всегда Ok).
run_mutation M1 "убрать allowlist check" "$W/allowlist.rs" \
  's|self.check_depth(command, 0)|Ok(())|' \
  "$LIB_TEST allowlist_rejects_rm" "allowlist_rejects_rm"

# M2 — убрать обработку таймаута.
run_mutation M2 "убрать timeout" "$W/sandbox.rs" \
  's/if out.exit_code == 124 {/if false \&\& out.exit_code == 124 {/' \
  "$INT_TEST worker_timeout_handled" "worker_timeout_handled"

# M3 — убрать --cap-drop ALL.
run_mutation M3 "убрать --cap-drop ALL" "$W/sandbox.rs" \
  's/"--cap-drop".into(),/"--cap-drop-X".into(),/' \
  "$LIB_TEST run_args_have_security_flags" "run_args_have_security_flags"

# M4 — убрать --network none.
run_mutation M4 "убрать --network none" "$W/sandbox.rs" \
  's/network: "none".into(),/network: "bridge".into(),/' \
  "$INT_TEST worker_network_isolated" "worker_network_isolated"

# M5 — убрать --read-only.
run_mutation M5 "убрать --read-only" "$W/sandbox.rs" \
  's/"--read-only".into(),/"--read-only-X".into(),/' \
  "$INT_TEST sandbox_starts_and_stops" "sandbox_starts_and_stops"

# M6 — убрать запись audit-события.
run_mutation M6 "убрать audit event" "$W/agent_loop.rs" \
  's/sink.record(\&audit)?;/let _ = \&audit;/' \
  "$INT_TEST tool_call_events_logged" "tool_call_events_logged"

echo
echo "==================== РЕЗУЛЬТАТЫ МУТАЦИЙ M1–M6 ===================="
for r in "${RESULTS[@]}"; do echo "$r"; done
echo "----------------------------------------------------------------"
echo "поймано: $caught / $((caught+survived))   выжило: $survived"
echo
echo "--- git diff --stat (должно быть пусто) ---"
if command -v git >/dev/null && git rev-parse --git-dir >/dev/null 2>&1; then
  git diff --stat
else
  echo "(не git-репозиторий — проверяем контрольные суммы ниже)"
fi
md5sum "$W/allowlist.rs" "$W/sandbox.rs" "$W/agent_loop.rs"

[ "$survived" -eq 0 ]
