#!/bin/sh
# 03-roles-2b.sh — применяет step2b/roles_2b.sql (идентификация воркера)
# после roles.sql.
#
# Запускается официальным образом postgres из /docker-entrypoint-initdb.d/
# только при пустом каталоге данных (первая инициализация).
#
# Порядок инициализации:
#   01-schema.sql        (schema/schema_v0.sql)
#   02-roles.sh          (step2/roles.sql)
#   03-roles-2b.sh       (этот файл; step2b/roles_2b.sql)
#   04-role-patches.sh   (step2/roles_patch_*.sql)
set -eu

: "${POSTGRES_USER:?POSTGRES_USER required}"
: "${POSTGRES_DB:?POSTGRES_DB required}"

psql -v ON_ERROR_STOP=1 \
     -U "$POSTGRES_USER" -d "$POSTGRES_DB" \
     -v w1_pw="$SWARM_PW_W1" \
     -v w2_pw="$SWARM_PW_W2" \
     -v w_1_pw="$SWARM_PW_W_1" \
     -v gpu_1_pw="$SWARM_PW_GPU_1" \
     -f /sql/roles_2b.sql
