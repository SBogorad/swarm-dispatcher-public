#!/usr/bin/env bash
# Выгрузка релиза/аудит-пакета swarm-dispatcher на Google Диск (rclone).
#
# Назначение: после закрытия контрольной точки (волны) или фиксации релиза
# складывать бандлы, diffs и evidence в папку на Диске — свежая копия под
# рукой с телефона/ноутбука. Код остаётся приватным (в публичный интернет не
# уходит; Диск владельца).
#
# Требуется: настроенный rclone-remote `gdrive` (аккаунт владельца).
# Проверка: `rclone lsd gdrive:` печатает список папок.
#
# Использование:
#   scripts/upload_release_gdrive.sh                # авто: тег или wip-<sha>
#   scripts/upload_release_gdrive.sh v0.5.0         # явное имя релиза
#   DRY_RUN=1 scripts/upload_release_gdrive.sh      # показать, что уйдёт
set -euo pipefail

# Корень репозитория (скрипт лежит в scripts/).
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

# --- параметры --------------------------------------------------------------
REMOTE="${GDRIVE_REMOTE:-gdrive}"
# Папка на Диске. Переопределяется переменной окружения GDRIVE_FOLDER.
TARGET_FOLDER="${GDRIVE_FOLDER:-Swarm_Dispatcher_Audit_and_Releases}"

# Определяем коммит/тег.
HEAD_COMMIT="$(git rev-parse --short HEAD)"
TAG_NAME="${1:-$(git describe --tags --exact-match 2>/dev/null || echo "wip-${HEAD_COMMIT}")}"
DEST="$(printf '%s:%s/release_%s' "$REMOTE" "$TARGET_FOLDER" "$TAG_NAME")"
RCLONE_OPTS=(--progress --transfers 4 --checkers 8)

echo "==> Релиз ${TAG_NAME} (${HEAD_COMMIT}) -> ${DEST}"

# Проверяем доступ к remote до начала работы.
if ! rclone lsd "${REMOTE}:" >/dev/null 2>&1; then
    echo "ОШИБКА: remote '${REMOTE}' недоступен. Настройте rclone (rclone config)." >&2
    echo "Проверка: rclone lsd ${REMOTE}:" >&2
    exit 1
fi

upload_dir() { # $1=локальный путь, $2=подкаталог назначения, $3...=--include маски
    local src="$1" sub="$2"; shift 2
    [ -d "$src" ] || return 0
    if [ "${DRY_RUN:-0}" = "1" ]; then
        echo "    [dry-run] rclone copy $src -> ${DEST}/${sub} $*"
    else
        rclone copy "$src" "${DEST}/${sub}" "${RCLONE_OPTS[@]}" "$@"
    fi
}

upload_file() { # $1=локальный файл, $2=подкаталог (по умолчанию — корень релиза)
    local src="$1" sub="${2:-}"
    [ -f "$src" ] || return 0
    local dst="${DEST}/"
    [ -n "$sub" ] && dst="${DEST}/${sub}/"
    if [ "${DRY_RUN:-0}" = "1" ]; then
        echo "    [dry-run] rclone copyto $src -> ${dst}$(basename "$src")"
    else
        rclone copyto "$src" "${dst}$(basename "$src")" "${RCLONE_OPTS[@]}"
    fi
}

# 1. Собранные архивы/пакеты (если есть dist/).
upload_dir "dist" "dist"

# 2. Evidence волн (все wave*.txt + triad/find-логи).
upload_dir "docs/audit/evidence" "evidence" --include "wave*.txt"

# 3. Патчи/diffs (если каталог ведётся).
upload_dir "docs/audit/diffs" "diffs"

# 4. Сводные документы аудита (протокол, маппинг, закрытие замечаний).
for f in docs/audit/AUDIT_PROTOCOL_*.md \
         docs/audit/FINDINGS_SD_MAPPING_*.md \
         docs/audit/REAUDIT_*_CLOSURE_*.md \
         docs/audit/AUDITOR_PROMPT_*.md \
         docs/audit/environment_versions.txt \
         docs/research/POSITIONING_2026.md; do
    upload_file "$f"
done

# 5. Готовые zip-пакеты аудита из outbox (если собран).
if compgen -G "outbox/*audit*.zip" >/dev/null 2>&1; then
    upload_dir "outbox" "bundles" --include "*audit*.zip"
fi

echo "==> Готово: ${DEST}"
echo "    Проверка: rclone lsf \"${DEST}\""
