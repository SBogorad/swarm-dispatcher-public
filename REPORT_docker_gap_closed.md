# REPORT — Docker-гэп закрыт (§5.5 / критерии 7–9 step 1)

**Дата:** 2026-10-08
**Верификатор:** OpenClaw (Арго)
**Статус:** ✅ **Docker-гэп закрыт.** Полный стек в контейнерах healthy, раздельные DSN по ролям, e2e под реальными ролями (не суперпользователем).
**Разблокировка:** оператор добавил себя в группу `docker` (`sudo usermod -aG docker $USER`). Агент работает через `newgrp docker` — процесс гейтвея стартовал до `usermod`, поэтому группа берётся из `/etc/group` обёрткой.

---

## 1. Что проверено (критерии §10 ТЗ v0.4 / step 1, критерии 7–9)

| # | Критерий | Результат |
|---|---|---|
| 1 | `docker compose up -d --build` — полный стек | ✅ postgres healthy, watchdog + worker-1 + worker-2 запущены |
| 2 | `scripts/02-roles.sh` применён, роли работают | ✅ init-порядок 01→04, 22 функции, 12 ролей |
| 3 | Раздельные DSN по сервисам | ✅ `swarm_watchdog` / `swarm_worker_w1` / `swarm_worker_w2` / `swarm_cli` / `swarm_planner` |
| 4 | e2e под ролями в контейнерах (не native) | ✅ Planner → Worker → артефакт → review → done |

**init-порядок в логах postgres:**
```
running /docker-entrypoint-initdb.d/01-schema.sql
running /docker-entrypoint-initdb.d/02-roles.sh
sourcing /docker-entrypoint-initdb.d/03-roles-2b.sh
running /docker-entrypoint-initdb.d/04-role-patches.sh
03-role-patches: applying /sql/roles_patch_planner_status.sql
```
**Итог БД:** 22 функции (19 схемы + `task_status` + step 2b + патч), 12 ролей (`swarm`, `swarm_owner`, `swarm_worker`, `swarm_watchdog`, `swarm_planner`, `swarm_reviewer`, `swarm_operator`, `swarm_cli`, `swarm_worker_w1`, `swarm_worker_w2`, `swarm_worker_w_1`, `swarm_worker_gpu_1`).

---

## 2. Найденные и исправленные дефекты конфигурации (7)

Все — конфигурационные, не логические. Ни один не был виден на native-прогоне.

1. **`.env` без паролей ролей** → `docker compose config` падал на `${SWARM_PW_*:?set}`.
   Фикс: `.env` дополнен `SWARM_PW_WORKER…SWARM_PW_GPU_1` (10 значений, сгенерированы `secrets.token_urlsafe`).
2. **`DATABASE_URL` всех сервисов — суперпользователь `swarm`.** Критерий «раздельные DSN по сервисам» не выполнялся: watchdog/worker/cli ходили под `swarm`.
   Фикс: DSN по ролям (`swarm_watchdog`, `swarm_worker_w1`, `swarm_worker_w2`, `swarm_cli`, `swarm_planner`).
   *Дополнительно поймано:* при первой правке в URL остался `${DB_PASSWORD}` — все сервисы получали пароль суперпользователя, аутентификация падала `E28P01`.
3. **`worker_id` = `worker-1`/`worker-2`, а step 2b требует `w1`/`w2`.** `normalize_worker_id` даёт `swarm_worker_worker_1`, роль `swarm_worker_w1` не совпадает → `WK003 worker_identity_mismatch`.
   Фикс: `WORKER_ID: w1` / `w2`.
4. **Воркер v0.3 падал на старте: `LLM_PROVIDER required`.** Compose от step 1/2 не передавал LLM-настройки.
   Фикс: `LLM_PROVIDER/LLM_BASE_URL/LLM_API_KEY/LLM_MODEL` + `ARTIFACTS_DIR` в env воркеров (значения из `.env`, по умолчанию `mock`).
5. **tmpfs `/var/swarm/artifacts` принадлежал root** → запись артефакта падала `Permission denied (os error 13)` (воркер работает под uid 1001).
   Фикс: `uid=1001,gid=1001,mode=0755` в tmpfs-опции.
6. **Сервис `cli` под `profiles: ["tools"]` не собирался при `docker compose up -d --build`.** `run --rm --no-deps cli` брал **8-часовой устаревший образ**, где `swarm-cli status` ещё делал прямой `SELECT ... FROM tasks` (до `task_status`) → `permission denied for table tasks` под ролью.
   Фикс: `docker compose build cli` явно; проверено — `status` под `swarm_cli` работает через `task_status`.
7. **Planner (step 3) отсутствовал в `Dockerfile` и `compose`.** e2e в контейнерах был невозможен.
   Фикс: `swarm-planner` в образ + сервис `planner` (одноразовый, как `cli`; запуск `docker compose run --rm --no-deps planner swarm-planner "<текст>"`).

---

## 3. e2e в контейнерах под реальными ролями

```
1) Planner (роль swarm_planner, реальный relay.fast):
   $ docker compose run --rm --no-deps planner swarm-planner \
       "Напиши на Rust функцию add(a: i32, b: i32) -> i32, которая складывает два числа."
   root_id: c2a1fb03-6382-5b0b-bdad-cabd9ed1a734
   tasks created: 2
   ready to execute: 1

2) Worker (роль swarm_worker_w1, WORKER_ID=w1):
   worker registered worker_id=w1
   start work task_id=c2a1fb03… epoch=1
   completed new_state=review tokens=325
   task -> review artifact_ref=sha256:732355035dbe…a7f273

3) Артефакт на диске (в контейнере):
   /var/swarm/artifacts/732355035dbe…a7f273.txt  (55 байт, владелец swarm:swarm)
   sha256sum → 732355035dbe…a7f273  ==  ref в БД ✅
   содержимое:
   ```rust
   fn add(a: i32, b: i32) -> i32 {
       a + b
   }
   ```

4) Review (роль swarm_cli → swarm_reviewer):
   new state: done
   verify_chain(c2a1fb03…) → t
```

**Идемпотентность Planner** подтверждена и в контейнерах: повторный запуск с тем же текстом дал **тот же** `root_id`.

---

## 4. Регрессии (не сломано)

| Проверка | Результат |
|---|---|
| `cargo build --workspace --locked` | 0 warn / 0 err |
| `cargo test --workspace --lib` | 8 core + 2 llm + 16 planner + 12 worker |
| `planner_integration` (native) | 9 passed |
| `worker_integration` (native) | 10 passed |
| `tests/test_v0.py` | 26 passed |
| `step2/test_roles.py` | 30 passed |
| `step2b/test_roles_2b.py` | 19 passed |

---

## 5. Что не сработало / не проверено

1. **Рестарт гейтвея повалил стек.** Native Postgres (жил в cgroup гейтвея) умер; docker-контейнеры получили `Exited (255)`. Поднято заново: `swarm-test start` + `docker compose up -d`. Процедурная заметка: после рестарта гейтвея проверять и native PG, и docker-стек.
2. **`worker-2` (демо-краш) мешает e2e.** `CRASH_AFTER_CLAIM=1` сжигает попытки при каждом claim → задача уходит в `blocked: max_attempts`. Для e2e его останавливали. Ожидаемое поведение демо, не дефект.
3. **`docker compose --profile tools up` запускает `cli`/`planner` как сервисы** (без `command` они исполняют `CMD` образа = watchdog). Правильно: обычный `up` (без tools) + `run --rm` для `cli`/`planner`. В compose добавлен комментарий.
4. **Truncate во время работы воркера снимает регистрацию** (`worker is not registered`) — воркер перерегистрируется сам, но нужно дать ему пару секунд.
5. **v0.4 не начинался** — это закрытие Docker-гэпа, не реализация shell tools/sandbox.

---

## 6. Файлы + md5

| Файл | Строк | md5 |
|---|---|---|
| `docker-compose.yml` | 133 | `229661245c945eb66e16a99fb115d487` |
| `Dockerfile` | 35 | `6f42122a37fa891b6b5d18bca645c3b8` |
| `scripts/02-roles.sh` | 25 | `222f2961a86f5beb870842a67892823f` |
| `scripts/03-roles-2b.sh` | 24 | `7641d32f6d09c4e07ae237a90945a7a5` |
| `scripts/03-role-patches.sh` | 26 | `fb95baf99f0113e70b2be398967fad25` |
| `.env.example` | — | `881eda95ee6ba793c4433a38767a7641` |
| `docs/KNOWN_LIMITATIONS.md` | — | `9f9616e95b2294447e221450148c91fe` |
| `RESUME.md` | — | `752cbbf3b39f16db88bc4c8fe74e962a` |
| `docs/METHODOLOGY.md` | — | `a0144c4311bfec4f1efafeff5ba08122` |
| `docs/optional/NATIVE_SETUP.md` | — | `a1edfd82eb8084bb381951220d1f7421` |
| `docs/TZ_v0.4_shell_tools_sandbox.md` | — | `d3c3e9850cae4d6ab414dfe325abc1c4` |

Окружение: Docker client/server 29.7.2, compose 5.5.1.

---

## 7. Стоп / вопросы автору

1. **`worker-2` с `CRASH_AFTER_CLAIM=1`** — оставить в compose (демо reclaim) или убрать, чтобы `up` давал чистый рабочий стек? Сейчас его приходится останавливать для e2e.
2. **`cli`/`planner` под `profiles: ["tools"]`** — оставить так (запуск через `run --rm`) или убрать профиль, чтобы они всегда были доступны?
3. **`newgrp docker` для агента** — рабочий обход, но хрупкий (зависит от `/etc/group`). Альтернатива: перезапустить службу гейтвея после `usermod`, тогда группа подхватится нативно. Перезапускать?
4. **v0.4 (shell tools + sandbox)** — ТЗ сохранено; Docker теперь доступен. Начинать реализацию?

---

## 8. Итог

Docker-гэп (§5.5, критерии 7–9 step 1), висевший с step 1, **закрыт**: полный стек в контейнерах, init-порядок ролей, раздельные DSN, e2e под реальными ролями с артефактом-кодом и `verify_chain` = true. Заодно закрыты 7 дефектов конфигурации, невидимых на native-прогоне, — ровно тот класс ошибок, про который Принцип 11 в `METHODOLOGY.md` («тесты под суперпользователем маскируют дефекты грантов»).
