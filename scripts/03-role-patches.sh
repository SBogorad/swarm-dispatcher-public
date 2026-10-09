#!/bin/sh
# 03-role-patches.sh — применяет патчи грантов после roles.sql и roles_2b.sql.
#
# Запускается официальным образом postgres из /docker-entrypoint-initdb.d/
# только при пустом каталоге данных (первая инициализация).
#
# Порядок инициализации:
#   01-schema.sql                 (schema/schema_v0.sql)
#   02-roles.sh                   (step2/roles.sql)
#   03-roles-2b.sh                (step2b/roles_2b.sql)
#   04-role-patches.sh            (этот файл; step2/roles_patch_*.sql)
#
# roles.sql и roles_2b.sql не меняются (приняты). Новые гранты добавляются
# отдельными патч-файлами вида step2/roles_patch_*.sql и подключаются здесь.
set -eu

: "${POSTGRES_USER:?POSTGRES_USER required}"
: "${POSTGRES_DB:?POSTGRES_DB required}"

for patch in /sql/roles_patch_*.sql; do
  [ -e "$patch" ] || continue
  echo "03-role-patches: applying $patch"
  psql -v ON_ERROR_STOP=1 \
       -U "$POSTGRES_USER" -d "$POSTGRES_DB" \
       -f "$patch"
done
