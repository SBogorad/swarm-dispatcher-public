# Развёртывание без Docker и без root

Инструкция для сред, где Docker недоступен или требует root. Проверено на
Omarchy (Arch Linux) с Postgres 18.6 в user-space.

**Что это:** альтернатива `docker compose up`. Даёт тот же функциональный
результат: Postgres + watchdog + воркеры + CLI. Без изоляции (нет
`cap_drop`), но с полной функциональностью.

**Когда использовать:**
- Docker требует root, а у вас его нет.
- Исследовательская среда, где Docker запрещён политикой.
- Быстрый dev-цикл без пересборки контейнеров.

**Когда НЕ использовать:**
- Production (там нужна изоляция воркеров).
- Мульти-пользовательская среда (нет ролей БД).

---

## 1. Требования

- Linux / macOS (проверено на Arch).
- Postgres 16+ (16.4 или 18.6).
- Rust 1.85+.
- Для acceptance-проверок: Docker, `shellcheck`, `python3` и `sha256sum`.
- `~/.local/bin` в `$PATH`.
- ~500 МБ свободного места.

---

## 2. Установка Postgres в user-space (Arch)

```bash
# Скачать пакет без установки через pacman
cd /tmp
pacman -Sw postgresql --noconfirm
# Или скачать вручную с https://archive.archlinux.org/packages/p/postgresql/

# Распаковать в ~/.local/opt
mkdir -p ~/.local/opt/postgres
cd ~/.local/opt/postgres
tar -xJf /tmp/postgresql-*.pkg.tar.zst

# Проверить
~/.local/opt/postgres/usr/bin/postgres --version
```

Для Debian/Ubuntu:

```bash
# Установить через apt-get download + dpkg -x
cd /tmp
apt-get download postgresql-16 postgresql-client-16
mkdir -p ~/.local/opt/postgres
dpkg -x postgresql-16*.deb ~/.local/opt/postgres
dpkg -x postgresql-client-16*.deb ~/.local/opt/postgres
```

---

3. Инициализация кластера

```bash
PGDATA=~/.local/share/swarm-test/pgdata
PGBIN=~/.local/opt/postgres/usr/bin

# Создать директорию
mkdir -p "$PGDATA"

# Инициализировать кластер
$PGBIN/initdb -D "$PGDATA" --encoding=UTF8 --locale=C

# Настроить порт (не 5432, чтобы не конфликтовать с системным)
cat >> "$PGDATA/postgresql.conf" <<EOF
port = 55440
listen_addresses = '127.0.0.1'
unix_socket_directories = '$PGDATA'
EOF

# Запустить
$PGBIN/pg_ctl -D "$PGDATA" -l "$PGDATA/log" start
```

---

4. Создать БД и применить схему

```bash
PGBIN=~/.local/opt/postgres/usr/bin

# Создать БД
$PGBIN/createdb -p 55440 -h 127.0.0.1 swarm

# Применить схему
$PGBIN/psql -p 55440 -h 127.0.0.1 -d swarm -f schema/schema_v0.sql

# Применить роли и патчи — строго в этом порядке:
#   schema_v0.sql → roles.sql → roles_2b.sql → roles_patch_planner_status.sql
#                                    → roles_patch_tool_call_audit.sql
$PGBIN/psql -p 55440 -h 127.0.0.1 -d swarm \
  -v worker_pw=pw_worker -v watchdog_pw=pw_watchdog -v planner_pw=pw_planner \
  -v reviewer_pw=pw_reviewer -v operator_pw=pw_operator -v cli_pw=pw_cli \
  -f step2/roles.sql
$PGBIN/psql -p 55440 -h 127.0.0.1 -d swarm \
  -v w1_pw=pw_w1 -v w2_pw=pw_w2 -v w_1_pw=pw_w_1 -v gpu_1_pw=pw_gpu_1 \
  -f step2b/roles_2b.sql
$PGBIN/psql -p 55440 -h 127.0.0.1 -d swarm \
  -f step2/roles_patch_planner_status.sql
$PGBIN/psql -p 55440 -h 127.0.0.1 -d swarm \
  -f step2/roles_patch_tool_call_audit.sql   # v0.4: аудит tool calls

# Проверить
$PGBIN/psql -p 55440 -h 127.0.0.1 -d swarm -c "\dt"
# Ожидается 6 таблиц: tasks, artifacts, task_deps, workers, events, reviews
$PGBIN/psql -p 55440 -h 127.0.0.1 -d swarm -Atc \
  "select count(*) from pg_proc p join pg_namespace n on n.oid=p.pronamespace where n.nspname='public';"
# Ожидается 23 функции (19 в схеме + task_status + функции step 2b + 2 патча)
```

**Важно (процедурная заметка v0.3.1).** `task_status` определён в
`step2/roles.sql`, а не в `schema_v0.sql` (19 функций против 20). Если
собрать БД только из схемы, вызовы `task_status` падают с
`function task_status(unknown) does not exist` — как это было с тестовой
БД `v0worker`. Всегда применять полный порядок четырёх файлов.

---

5. Собрать бинарники

```bash
cd swarm-dispatcher
cargo build --release

# Бинарники в target/release/
ls target/release/swarm-*
# swarm-cli  swarm-watchdog  swarm-worker
```

---

6. Запустить сторож и воркеров

В отдельных терминалах (или через nohup, tmux, systemd --user):

```bash
export DATABASE_URL="postgres://$USER@127.0.0.1:55440/swarm"

# Терминал 1: watchdog
WATCHDOG_INTERVAL=10 RUST_LOG=swarm_watchdog=info \
  ./target/release/swarm-watchdog

# Терминал 2: worker-1
WORKER_ID=worker-1 WORK_SECONDS=5 LEASE_SEC=10 \
  RUST_LOG=swarm_worker=info \
  ./target/release/swarm-worker

# Терминал 3: worker-2 (с крашем)
WORKER_ID=worker-2 WORK_SECONDS=5 LEASE_SEC=10 CRASH_AFTER_CLAIM=1 \
  RUST_LOG=swarm_worker=info \
  ./target/release/swarm-worker
```

---

7. Использовать CLI

```bash
export DATABASE_URL="postgres://$USER@127.0.0.1:55440/swarm"

# Создать задачи
./target/release/swarm-cli seed

# Посмотреть статус
./target/release/swarm-cli status

# Одобрить ревью
./target/release/swarm-cli review <task_id> approve <artifact_ref>

# Переоткрыть задачу
./target/release/swarm-cli reopen <task_id> operator
```

---

8. Остановка и очистка

```bash
# Остановить Postgres
$PGBIN/pg_ctl -D "$PGDATA" stop

# Полная очистка (удаляет все данные)
rm -rf ~/.local/share/swarm-test
rm -rf ~/.local/opt/postgres
```

---

9. Управляющий скрипт (опционально)

Если развёртывание повторяется — сделать ~/.local/bin/swarm-test:

```bash
#!/bin/bash
# swarm-test: управление native-средой

PGDATA=~/.local/share/swarm-test/pgdata
PGBIN=~/.local/opt/postgres/usr/bin
PGPORT=55440
PGBIN_PATH=~/.local/opt/postgres/usr/bin
BIN_DIR=~/Projects/rust/swarm-dispatcher/target/release

export DATABASE_URL="postgres://$USER@127.0.0.1:$PGPORT/swarm"

case "$1" in
  start)
    $PGBIN/pg_ctl -D "$PGDATA" -l "$PGDATA/log" start
    ;;
  stop)
    $PGBIN/pg_ctl -D "$PGDATA" stop
    ;;
  status)
    $PGBIN/pg_ctl -D "$PGDATA" status
    $BIN_DIR/swarm-cli status 2>/dev/null || echo "(cli не собран)"
    ;;
  reset)
    $PGBIN/pg_ctl -D "$PGDATA" stop
    rm -rf "$PGDATA"
    $PGBIN/initdb -D "$PGDATA" --encoding=UTF8 --locale=C
    echo "port = $PGPORT" >> "$PGDATA/postgresql.conf"
    echo "listen_addresses = '127.0.0.1'" >> "$PGDATA/postgresql.conf"
    $PGBIN/pg_ctl -D "$PGDATA" -l "$PGDATA/log" start
    sleep 2
    $PGBIN/createdb -p $PGPORT -h 127.0.0.1 swarm
    $PGBIN/psql -p $PGPORT -h 127.0.0.1 -d swarm -f schema/schema_v0.sql
    ;;
  psql)
    shift
    $PGBIN/psql -p $PGPORT -h 127.0.0.1 -d swarm "$@"
    ;;
  seed)
    $BIN_DIR/swarm-cli seed
    ;;
  cli)
    shift
    $BIN_DIR/swarm-cli "$@"
    ;;
  watchdog)
    $BIN_DIR/swarm-watchdog
    ;;
  worker)
    shift
    $BIN_DIR/swarm-worker "$@"
    ;;
  *)
    echo "Usage: swarm-test {start|stop|status|reset|psql|seed|cli|watchdog|worker}"
    exit 1
    ;;
esac
```

```bash
chmod +x ~/.local/bin/swarm-test
swarm-test start
swarm-test seed
swarm-test cli status
```

---

10. Отличия от Docker Compose

Аспект Compose Native
Изоляция воркеров cap_drop: ALL, read_only: true Нет
Роли БД Возможны (roles.sql) Один пользователь
Воспроизводимость Полная (образы пиннуются) Зависит от версии Postgres
Root Требуется для Docker Не требуется
Диск Volumes в Docker Прямой доступ к ~/.local/
Порт Внутри сети Docker 127.0.0.1:55440

Native — для dev / research. Compose — для production / демонстраций.

---

11. Известные подводные камни

1. Postgres 18 vs 16. Схема работает на обеих. Но если параллельно
используете compose (16.4) и native (18.6) — не подключайте воркеров
одного Postgres к другому.

2. Unix socket. В postgresql.conf указан unix_socket_directories =
'$PGDATA'. Если хотите psql без -h 127.0.0.1 — используйте
-h $PGDATA.

3. Порты. 55440 выбран, чтобы не конфликтовать с системным Postgres.
Если у вас уже занят — поменяйте в postgresql.conf.

4. Порты воркеров. Воркеры не слушают порты — только исходящие
соединения к Postgres. Межмашинная работа требует, чтобы Postgres был
доступен по сети (порт 55440 открыт в firewall для нужных IP).

```

---

## Файл 8: `docs/BRIEF_FOR_AGENTS.md` (обновлённый)

Полная версия — в предыдущем сообщении, здесь только правки:

```diff
-| `schema/schema_v0.sql` | Готов, 26 тестов проходят | 6 таблиц, 14 функций, codes LS001…WK002 |
+| `schema/schema_v0.sql` | Готов, 26 тестов проходят | 6 таблиц, 19 функций, codes LS001…WK002 |
```

Уточнение в разделе 4:

```markdown
## 4. Что уже есть

| Артефакт | Статус | Что внутри |
|----------|--------|-----------|
| `schema/schema_v0.sql` | Готов, 26 тестов проходят | 6 таблиц, **19 функций** (14 публичных API + 5 вспомогательных), codes LS001…WK002 |
| ... | ... | ... |

**Публичные API (14):** `add_task`, `add_dep`, `promote_ready`,
`register_worker`, `claim_task`, `heartbeat`, `save_artifact`,
`complete_task`, `submit_review`, `reclaim_expired`, `reopen_task`,
`unblock_task`, `append_event`, `verify_chain`.

**Вспомогательные (5):** `c_num`, `lock_root`, `event_hash`,
`assert_lease`, `validate_contract`.
