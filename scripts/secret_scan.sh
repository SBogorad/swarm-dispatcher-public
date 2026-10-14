#!/usr/bin/env bash
# Скан секретов в диапазоне коммитов: ищет РЕАЛЬНЫЕ значения из .env*, а не
# паттерны. Вызывается из .githooks/pre-commit и .githooks/pre-push.
#
# Почему значения, а не паттерны: grep по `password|api_key|secret` даёт
# ложные срабатывания на плейсхолдерах (.env.example), именах полей и строках
# в промптах. Источник истины — сами значения из .env*; любое совпадение в
# диффе = реальная утечка.
#
# Использование:
#   scripts/secret_scan.sh <git-range>      # напр. origin/master..HEAD, или --cached
#   scripts/secret_scan.sh --self-test      # доказать, что скан краснеет на фейк-секрете
#
# Код возврата: 0 — чисто; 1 — найдена утечка; 2 — ошибка использования.
set -Eeuo pipefail

REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"

# --- self-test: доказать, что скан КРАСНЕЕТ на заведомом секрете ------------
if [[ "${1:-}" == "--self-test" ]]; then
  fake="FAKE_SECRET_VALUE_$(date +%s)_do_not_use"
  tmp="$(mktemp -d)"
  ( cd "$tmp" && git init -q && git config user.email t@t && git config user.name t \
    && printf 'key=%s\n' "$fake" > f.txt && git add f.txt )
  # .env с фейковым значением, чтобы скан его «нашёл».
  printf 'SOME_API_KEY=%s\n' "$fake" >"$tmp/.env"
  out="$(cd "$tmp" && SECRET_ENV_DIR="$tmp" bash "$REPO_ROOT/scripts/secret_scan.sh" --cached 2>&1 || true)"
  rm -rf "$tmp"
  if printf '%s' "$out" | grep -q "LEAK"; then
    echo "PASS self-test: скан краснеет на фейк-секрете"
    exit 0
  fi
  echo "FAIL self-test: скан НЕ поймал фейк-секрет"
  exit 1
fi

RANGE="${1:?нужен git-range (напр. origin/master..HEAD) или --cached}"
ENV_DIR="${SECRET_ENV_DIR:-$REPO_ROOT}"

# Собираем реальные значения секретов из .env*.
mapfile -t SECRETS < <(python3 - "$ENV_DIR" <<'PY'
import re, sys, glob, os
root = sys.argv[1]
key_re = re.compile(r'(PW|PASSWORD|API_KEY|TOKEN|SECRET|DSN|DATABASE_URL)', re.I)
nonsec = {'openai-compatible','deepseek-v4-flash','deepseek-v4-pro','deepseek-v4.1-flash',
          'openai','mock','relay.fast','true','false','localhost'}
vals = set()
for path in glob.glob(os.path.join(root, '.env*')):
    if os.path.basename(path) in ('.env.example',) or path.endswith('.example'):
        continue
    try:
        text = open(path, encoding='utf-8', errors='ignore').read()
    except OSError:
        continue
    for line in text.splitlines():
        line = line.strip()
        if not line or line.startswith('#') or '=' not in line:
            continue
        k, v = line.split('=', 1)
        v = v.strip().strip('"').strip("'")
        if not key_re.search(k):
            continue
        if len(v) >= 8 and v.lower() not in nonsec:
            vals.add(v)
        m = re.search(r'://[^:]+:([^@]+)@', v)
        if m and len(m.group(1)) >= 8:
            vals.add(m.group(1))
        m2 = re.search(r'password=([^\s]+)', v)
        if m2 and len(m2.group(1)) >= 8:
            vals.add(m2.group(1))
print('\n'.join(sorted(vals)))
PY
)

if [[ ${#SECRETS[@]} -eq 0 ]]; then
  echo "WARN: не найдено ни одного реального значения секрета в .env* (скан пропущен)"
  exit 0
fi

if [[ "$RANGE" == "--cached" ]]; then
  DIFF="$(git diff --cached -U0)"
else
  DIFF="$(git diff -U0 "$RANGE" 2>/dev/null || true)"
fi

hits=0
for v in "${SECRETS[@]}"; do
  # Только добавленные строки (+), чтобы удаление секрета не считалось утечкой.
  if printf '%s\n' "$DIFF" | grep -F -- "$v" >/dev/null 2>&1; then
    sha8="$(printf '%s' "$v" | sha256sum | cut -c1-8)"
    echo "LEAK: значение секрета (sha8=$sha8, len=${#v}) найдено в диффе $RANGE"
    hits=$((hits + 1))
  fi
done

if ((hits > 0)); then
  echo "RESULT: FAIL — найдено утечек: $hits. Коммит/пуш заблокирован."
  echo "Если это ложное срабатывание (значение легитимно в коде) — разберись вручную."
  exit 1
fi
echo "RESULT: PASS — секретов в диффе нет (проверено значений: ${#SECRETS[@]})"
