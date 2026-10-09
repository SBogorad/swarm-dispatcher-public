# ТЗ: роли БД и раздельные учётные данные (шаг 2 фазы v0.2)

Начинать только после приёмки step 1. Читать целиком.

## 0. Что уже готово и проверено (не переписывать)

- `step2/roles.sql` — 6 ролей, владелец `swarm_owner`, `SECURITY DEFINER` + `search_path = public, pg_temp`, `REVOKE ... FROM PUBLIC`, выдача по ролям, функция `task_status()` для чтения.
- `step2/test_roles.py` — 30 тестов.
- Проверено автором на Postgres 16.13 (Ubuntu): `test_roles.py` 30 passed; `test_v0.py` 26 passed на базе с применёнными ролями (под суперпользователем); 10 намеренных поломок `roles.sql`, все ловятся тестами. Не проверено: Postgres 16.4-alpine и 18.6, Docker, native-окружение. Это работа агента (см. §6).
- **Файлы `roles.sql` и `test_roles.py` менять нельзя**, кроме пути в `psql`-вызовах и значений окружения. Если что-то не работает, остановиться и описать проблему в отчёте.
- `schema_v0.sql`, `test_v0.py` и тела функций не менять.

## 1. Матрица прав (источник истины: `roles.sql`)

| Роль | Может вызывать |
|---|---|
| `swarm_worker` | `claim_task`, `heartbeat`, `save_artifact`, `complete_task`, `register_worker` |
| `swarm_watchdog` | `reclaim_expired`, `promote_ready` |
| `swarm_planner` | `add_task`, `add_dep`, `promote_ready` |
| `swarm_reviewer` | `submit_review`, `verify_chain` |
| `swarm_operator` | `reopen_task`, `unblock_task`, `verify_chain`, `task_status` |
| `swarm_cli` | объединение planner + reviewer + operator (членство). Для человека с CLI, не для сервисов. Прав воркера нет. |

Никто не имеет прямого доступа к таблицам. Внутренние функции (`append_event`, `assert_lease`, `lock_root`, `validate_contract`, `event_hash`, `c_num`) не выдаются никому. Таблицы и функции принадлежат `swarm_owner` (не суперпользователь, без входа).

## 2. Что сделать

### 2.1 Инициализация в Docker Compose
- Смонтировать `roles.sql` в контейнер postgres как `/sql/roles.sql:ro` и добавить скрипт `docker-entrypoint-initdb.d/02-roles.sh`, который запускается **после** `01-schema.sql`:
  ```sh
  psql -v ON_ERROR_STOP=1 -U "$POSTGRES_USER" -d "$POSTGRES_DB" \
    -v worker_pw="$SWARM_PW_WORKER"     -v watchdog_pw="$SWARM_PW_WATCHDOG" \
    -v planner_pw="$SWARM_PW_PLANNER"   -v reviewer_pw="$SWARM_PW_REVIEWER" \
    -v operator_pw="$SWARM_PW_OPERATOR" -v cli_pw="$SWARM_PW_CLI" -f /sql/roles.sql
  ```
  (Скрипт — образец, в контейнере не проверялся. В официальном образе `.sh` из `initdb.d` запускается, если исполняемый, иначе подгружается через `source`. Скрипты там выполняются только при пустом каталоге данных.)
- Пароли: шесть переменных `SWARM_PW_*` в `.env.example` (без значений) и в `docker-compose.yml` через `${SWARM_PW_WORKER:?задай в .env}`, чтобы без них compose не стартовал. Пароль суперпользователя postgres не передавать ни одному сервису.

### 2.2 DSN по сервисам
`worker-1`, `worker-2` → `swarm_worker`; `watchdog` → `swarm_watchdog`; `cli` → `swarm_cli`. Подставлять роль и её пароль в `DATABASE_URL` каждого сервиса.

### 2.3 CLI и Rust-код
- Прямых SQL к таблицам теперь нет доступа. Проверить `swarm-cli status` и все места с `SELECT ... FROM tasks` (кроме тестов под суперпользователем). Заменить на `SELECT * FROM task_status(n)`.
- Код функций менять не нужно: воркер, сторож и CLI уже используют только перечисленные в §1 функции. Если окажется иначе, не расширять права, а описать в отчёте.
- Интеграционные Rust-тесты и `test_v0.py` продолжают идти под суперпользователем (так задумано).

### 2.4 Native-окружение
Обновить `docs/NATIVE_SETUP.md` и скрипт `swarm-test reset`: после `schema_v0.sql` применять `roles.sql`, пароли генерировать (`head -c 24 /dev/urandom | base64 | tr -d '/+='`), хранить в файле с правами 600. Порт native-базы задаётся переменной `ROLES_PORT` для тестов.

### 2.5 Документация
- `docs/KNOWN_LIMITATIONS.md`: заменить пункт 2.1 на реальное состояние (см. §5).
- README: таблица ролей из §1 и один абзац «как запустить тесты ролей».
- Не плодить новые документы.

## 3. Запуск тестов ролей
```bash
ROLES_DB=swarm ROLES_HOST=127.0.0.1 ROLES_PORT=5432 ADMIN_PW=<пароль суперпользователя> \
SWARM_PW_WORKER=... SWARM_PW_WATCHDOG=... SWARM_PW_PLANNER=... \
SWARM_PW_REVIEWER=... SWARM_PW_OPERATOR=... SWARM_PW_CLI=... \
pytest -q tests/test_roles.py
```
Тесты чистят таблицы (`truncate`), поэтому гонять только на тестовой базе.

## 4. Критерии готовности
1. `pytest -q tests/test_roles.py` → в отчёте список имён тестов и итог `30 passed`. Ни одного skipped.
2. `pytest -q tests/test_v0.py` на базе **с** применёнными ролями → `26 passed`.
3. `cargo test --workspace --locked` → итог с числом passed, регрессий нет; список имён тестов.
4. Ручные проверки (команда и полный вывод в отчёте):
   - `psql -U swarm_worker -c "select add_task(...)"` → `permission denied`;
   - `psql -U swarm_planner -c "select * from claim_task('w',1)"` → `permission denied`;
   - `psql -U swarm_watchdog -c "select submit_review(...)"` → `permission denied`;
   - `psql -U swarm_worker -c "select * from tasks"` → `permission denied`.
5. Compose: `docker compose up -d --build` поднимает все сервисы. В `select usename, count(*) from pg_stat_activity group by 1` видны `swarm_worker` и `swarm_watchdog`, а сервисных подключений от `postgres` нет.
6. End-to-end в compose под ролями: seed → воркер падает (`CRASH_AFTER_CLAIM=1`) → сторож возвращает (`reclaimed: 1`) → второй воркер выполняет → `review` → `cli review ... approve` → `done`. `cli status` работает (через `task_status`).
7. Отрицательная проверка в compose: запустить `cli` с DSN роли `swarm_worker` и командой `seed` → ошибка прав доступа.
8. **Мутации (независимая проверка тестов в вашем окружении).** Для каждой из трёх копий `roles.sql` применить базу и прогнать `test_roles.py`, результат (число failed) записать в отчёт. Все три должны дать failed > 0:
   - M1: после `grant execute on function reclaim_expired, promote_ready to swarm_watchdog;` добавить `grant execute on function submit_review to swarm_worker;`
   - M3: заменить `set search_path = public, pg_temp'` на `set search_path = pg_temp, public'`
   - M5: убрать `security definer` из `alter function`.
   Если хоть одна мутация не ловится, это провал критерия, а не успех.
9. Прогон п. 1–2 на **обеих** версиях Postgres (16.4 в compose и 18.6 в native). Расхождения описать.

## 5. Известные ограничения после step 2 (записать в KNOWN_LIMITATIONS)
- Роли закрывают горизонтальные ошибки (planner не может брать задачи, воркер не может одобрять работу). Идентификации отдельного воркера нет: `worker_id` самопровозглашённый.
- Любой, у кого есть пароль `swarm_worker`, может: вызвать `heartbeat` и `complete_task` от имени чужого `worker_id`, если знает `task_id` и `epoch` (эпохи малые целые, угадываются); перезаписать capabilities любого воркера через `register_worker`.
- Закрывается отдельным шагом (2b): роль на воркера плюс сверка `p_worker` с `session_user` внутри функций. Не смешивать с этим шагом.
- Строка `revoke all on schema public from public` на Postgres ≥ 15 ничего не меняет (там и так нет `CREATE` у PUBLIC) и тестами не проверяется; оставлена как защита для Postgres ≤ 14.

## 6. Не делать
mTLS, OPA, подписи Ed25519, мультитенантность, роль на воркера, изменение функций схемы, таймауты ролей (`statement_timeout`) — отдельно.

## 7. Формат отчёта
`REPORT_v0.2_step2.md` одним файлом: список изменённых файлов, git diff, полный вывод критериев 1–9, отдельный блок «что не получилось или не проверено». Если что-то не сработало, остановиться и спросить автора.
