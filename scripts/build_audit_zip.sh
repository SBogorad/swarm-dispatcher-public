#!/usr/bin/env bash
# Собирает аудит-пакет проекта в ZIP БЕЗ СЖАТИЯ (zip -0, метод Store).
#
# Почему без сжатия: внешний аудитор (LLM) может получать .zip как сырой
# байтовый поток БЕЗ распаковки Deflate. Тогда текст внутри сжатого архива
# выглядит бинарным блоком и не читается построчно. В режиме Store весь
# текст лежит в потоке открытым текстом.
#
# Что пакуется: всё дерево коммита HEAD (`git archive`), а не отдельные
# заранее собранные XML-тома. Прежняя версия копировала восемь файлов из
# docs/audit/ (PROJECT_*_BUNDLE_v0.4.xml, AUDITOR_BRIEF_v0.4.*,
# AUDIT_PACKAGE_INDEX_v0.4.md, CHECKSUMS_v0.4.txt) и падала, если их не
# было; томы при этом описывали срез v0.4 и отставали от HEAD. Снимок
# дерева всегда соответствует ровно тому коммиту, который в нём лежит.
#
# Секреты: любой `.env*`, кроме `.env.example` и `.env.artifacts.example`,
# останавливает сборку. Проверка идёт по распакованному дереву, то есть по
# фактическому содержимому архива, а не по индексу git.
#
# Использование:
#   scripts/build_audit_zip.sh [output.zip]
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

cleanup() { rm -rf "$STAGE"; }
trap cleanup EXIT

# Точное содержимое коммита, без рабочего дерева и без untracked-мусора.
git archive --format=tar HEAD | tar -x -C "$DEST"

# Fail-closed: секреты не должны уехать внешнему аудитору.
mapfile -t LEAKS < <(find "$DEST" -type f -name '.env*' \
  ! -name '.env.example' ! -name '.env.artifacts.example' -print)
if (( ${#LEAKS[@]} > 0 )); then
  printf 'ERROR: в снимке есть секретные файлы:\n' >&2
  printf '  %s\n' "${LEAKS[@]}" >&2
  exit 1
fi

printf '%s\n' "$HEAD_SHA" >"$DEST/HEAD.txt"

cat >"$DEST/README_AUDIT.md" <<EOF
# Аудит-пакет swarm-dispatcher (БЕЗ СЖАТИЯ)

Снимок репозитория на коммите $HEAD_SHA (см. HEAD.txt).
Архив собран \`zip -0\` (Store, без Deflate): текст внутри лежит открытым
текстом и читается в потоке без распаковщика.

## Состав
- Дерево репозитория целиком — ровно содержимое коммита $HEAD_SHA
  (\`git archive HEAD\`), включая \`crates/\`, \`schema/\`, \`scripts/\`,
  \`tests/\`, \`.github/\` и документацию.
- \`HEAD.txt\` — полный SHA снимка.
- \`CHECKSUMS.txt\` — sha256 по каждому файлу снимка.
- \`README_AUDIT.md\` — этот файл.

## Секреты
Файлы \`.env*\` в архив не попадают. Сборка падает, если в дереве окажется
что-либо кроме \`.env.example\` и \`.env.artifacts.example\`.

## Целостность
\`\`\`
sha256sum -c CHECKSUMS.txt
\`\`\`
EOF

# Контрольные суммы по фактическому содержимому снимка.
(
  cd "$DEST"
  find . -type f ! -name 'CHECKSUMS.txt' -print0 \
    | LC_ALL=C sort -z \
    | xargs -0 sha256sum >CHECKSUMS.txt
)

mkdir -p "$(dirname "$OUT")"
# ВАЖНО: архив пишем АБСОЛЮТНЫМ путём $OUT. Раньше было `cd "$STAGE" &&
# zip ... "$(basename "$OUT")"` — архив создавался ВНУТРИ $STAGE, и
# следующий `rm -rf "$STAGE"` его удалял. Скрипт печатал 'OK', а файла не
# было (молчаливый no-op).
( cd "$STAGE" && zip -0 -q -r "$OUT" "$NAME" )

# Проверка, что архив читается и что в нём нет секретов.
unzip -tq "$OUT" >/dev/null
if unzip -Z1 "$OUT" | grep -E '/\.env[^/]*$' \
     | grep -vE '/\.env\.example$|/\.env\.artifacts\.example$' | grep -q .; then
  printf 'ERROR: в архиве обнаружены .env-файлы\n' >&2
  exit 1
fi

FILES="$(unzip -Z1 "$OUT" | grep -cv '/$')"
printf 'OK %s\n' "$OUT"
printf 'файлов в архиве: %s\n' "$FILES"
printf 'sha256: %s\n' "$(sha256sum "$OUT" | cut -d' ' -f1)"
