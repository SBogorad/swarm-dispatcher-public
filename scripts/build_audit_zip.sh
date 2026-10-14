#!/usr/bin/env bash
# Собирает аудит-пакет проекта в ZIP БЕЗ СЖАТИЯ (zip -0, метод Store).
#
# Почему без сжатия: внешний аудитор (LLM) может получать .zip как сырой
# байтовый поток БЕЗ распаковки Deflate. Тогда XML-тома и Markdown-брифы
# внутри сжатого архива выглядят бинарными блоками и не читаются построчно.
# В режиме Store весь текст лежит в потоке открытым текстом.
#
# Использование:
#   scripts/build_audit_zip.sh [output.zip]
#
# Предусловие: тома уже собраны (scripts/build_project_bundle.py +
# build_audit_index.py) и docs/audit/*.xml актуальны для текущего HEAD.
set -Eeuo pipefail

REPO_ROOT="$(git rev-parse --show-toplevel)"
cd "$REPO_ROOT"

HEAD_SHA="$(git rev-parse HEAD)"
SHORT="$(git rev-parse --short HEAD)"
OUT="${1:-$REPO_ROOT/outbox/swarm-dispatcher-audit-${SHORT}-uncompressed.zip}"
STAGE="$(mktemp -d)"
NAME="swarm-dispatcher-audit-${SHORT}-uncompressed"
DEST="$STAGE/$NAME"
mkdir -p "$DEST"

AUD="docs/audit"
for f in PROJECT_CODE_BUNDLE_v0.4.xml PROJECT_INFRA_BUNDLE_v0.4.xml \
         PROJECT_DOCS_CORE_BUNDLE_v0.4.xml PROJECT_DOCS_BUNDLE_v0.4.xml \
         AUDITOR_BRIEF_v0.4.md AUDITOR_BRIEF_v0.4.en.md \
         AUDIT_PACKAGE_INDEX_v0.4.md CHECKSUMS_v0.4.txt; do
  [[ -f "$AUD/$f" ]] || { echo "ERROR: нет $AUD/$f — собери тома сначала" >&2; exit 1; }
  cp "$AUD/$f" "$DEST/"
done
printf '%s\n' "$HEAD_SHA" >"$DEST/HEAD.txt"

cat >"$DEST/README_AUDIT.md" <<EOF
# Аудит-пакет swarm-dispatcher (БЕЗ СЖАТИЯ)

Снимок репозитория на коммите $HEAD_SHA (см. HEAD.txt).
Архив собран \`zip -0\` (Store, без Deflate): XML и Markdown внутри лежат
открытым текстом и читаются в потоке без распаковщика.

## Файлы
- PROJECT_CODE_BUNDLE_v0.4.xml      — Rust: крейты (src+tests+фикстуры+Cargo)
- PROJECT_INFRA_BUNDLE_v0.4.xml     — SQL схема/роли/патчи, init-скрипты, Dockerfile, compose, .env.example
- PROJECT_DOCS_CORE_BUNDLE_v0.4.xml — ТЗ, архитектура, ограничения, отчёты
- PROJECT_DOCS_BUNDLE_v0.4.xml      — то же + методология, история решений
- AUDITOR_BRIEF_v0.4.md / .en.md    — бриф аудитора
- AUDIT_PACKAGE_INDEX_v0.4.md       — сводный индекс
- CHECKSUMS_v0.4.txt                — sha256 по томам
- HEAD.txt                          — $HEAD_SHA

## Целостность
\`sha256sum -c CHECKSUMS_v0.4.txt\`
EOF

mkdir -p "$(dirname "$OUT")"
# ВАЖНО: архив пишем АБСОЛЮТНЫМ путём $OUT. Раньше было `cd "$STAGE" &&
# zip ... "$(basename "$OUT")"` — архив создавался ВНУТРИ $STAGE, и
# следующая строка `rm -rf "$STAGE"` его удаляла. Скрипт печатал 'OK',
# а файла не было (молчаливый no-op).
( cd "$STAGE" && zip -0 -q -r "$OUT" "$NAME" )
rm -rf "$STAGE"
echo "OK $OUT"
unzip -v "$OUT" | sed -n '1,4p'
