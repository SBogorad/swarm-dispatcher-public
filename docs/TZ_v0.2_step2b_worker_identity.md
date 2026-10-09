# ТЗ: идентификация воркера (шаг 2b фазы v0.2)

Начинать только после приёмки step 2. Читать целиком.

## 0. Зачем

После step 2 воркеры разделены по ролям, но `worker_id` остаётся
самопровозглашённым. Любой, у кого есть пароль `swarm_worker`, может:

- вызвать `heartbeat`, `save_artifact`, `complete_task` от имени чужого
  `worker_id`, если знает `task_id` и `epoch` (эпохи — малые целые);
- перезаписать `capabilities` любого воркера через `register_worker`.

Это терпимо, пока воркер не запускает чужой код. В v0.3 (LLM с
инструментами `shell`/`git`) это дыра. Step 2b закрывает её до v0.3.

## 1. Что уже готово и проверено (не переписывать)

- `step2/roles.sql`, `step2/test_roles.py` — 6 ролей, 30 тестов, 3 мутации.
  Проверено на PG 16.13 (Claude) и PG 18.6 (OpenClaw).
- `schema/schema_v0.sql` — 6 таблиц, 19 функций. Проверено 26 тестами.
- `crates/swarm-core/` — Rust-обёртка с `DispatcherTx<'a,'b>`.
- **Не менять**: `schema_v0.sql`, `roles.sql`, `test_roles.py`.

## 2. Дизайн

### 2.1. Роль на каждого воркера

- Общая роль `swarm_worker` остаётся (для админ-тестов).
- Создаётся роль `swarm_worker_<normalized_id>` для каждого воркера.
  Например, для `worker-1` → роль `swarm_worker_worker_1`.
- Каждая роль — член `swarm_worker` (`IN ROLE swarm_worker`), наследует
  её `EXECUTE`-права.
- Пароль у каждого воркера свой.

### 2.2. Проверка `session_user` внутри функций

Внутри `register_worker`, `claim_task`, `heartbeat`, `save_artifact`,
`complete_task` добавляется проверка: `session_user` соответствует
`p_worker`.

**Правила:**
- Проверка активна **только** если `session_user` начинается с
  `swarm_worker_`. Для `swarm_cli`, `swarm_planner`, `swarm_owner`,
  `postgres` — пропускается.
- Нормализация: `worker-1` → `worker_1`, `GPU.1` → `gpu_1`.
- Ожидаемое имя роли: `swarm_worker_` || normalized(`p_worker`).
- При несовпадении — `RAISE EXCEPTION 'worker_identity_mismatch'`
  с `ERRCODE = 'WK003'`.

### 2.3. Функция проверки

```sql
create or replace function normalize_worker_id(p_worker text)
returns text language sql immutable as $$
  select lower(regexp_replace(p_worker, '[-. ]', '_', 'g'))
$$;

create or replace function assert_worker_identity(p_worker text)
returns void language plpgsql as $$
declare v_expected text;
begin
  if session_user::text like 'swarm_worker_%' then
    v_expected := 'swarm_worker_' || normalize_worker_id(p_worker);
    if session_user::text <> v_expected then
      raise exception 'worker_identity_mismatch: session_user=% expected=%',
        session_user, v_expected using errcode = 'WK003';
    end if;
  end if;
end $$;
```

3. Что сделать

3.1. step2b/roles_2b.sql

Содержит:

1. normalize_worker_id(text) → text
2. assert_worker_identity(text) → void
3. CREATE OR REPLACE для пяти функций:
   · register_worker(p_id, p_caps, p_tags)
   · claim_task(p_worker, p_lease_sec)
   · heartbeat(p_task, p_worker, p_epoch, p_extend_sec)
   · save_artifact(p_task, p_worker, p_epoch, p_ref, p_storage, p_content_type, p_size)
   · complete_task(p_task, p_worker, p_epoch, p_result_ref, p_tokens)
   Тело каждой — копия из schema_v0.sql + одна строка
   perform assert_worker_identity(p_worker); в начале.
4. Создание ролей swarm_worker_w1, swarm_worker_w2 (для тестов) с
   IN ROLE swarm_worker.
5. ALTER FUNCTION ... OWNER TO swarm_owner; ALTER FUNCTION ... SECURITY DEFINER SET search_path = public, pg_temp; для новых функций.
6. REVOKE EXECUTE ON FUNCTION assert_worker_identity FROM PUBLIC;
   (это внутренняя функция — доступ только через обёртки).
7. GRANT EXECUTE ON FUNCTION normalize_worker_id TO PUBLIC; — она
   безвредная, может пригодиться.

Замечание: schema_v0.sql не трогаем. Следующая итерация схемы
(v0.3) должна включить проверку в тела функций. Пока — расширение через
CREATE OR REPLACE в roles_2b.sql.

Порядок применения:

```
schema_v0.sql → roles.sql → roles_2b.sql
```

3.2. step2b/test_roles_2b.py

Тесты (формат как в step2/test_roles.py):

Позитивные:

1. swarm_worker_w1 может register_worker('w1', ...).
2. swarm_worker_w1 может claim_task('w1', ...).
3. swarm_worker_w1 может heartbeat(..., 'w1', ...).
4. swarm_worker_w1 может save_artifact(..., 'w1', ...).
5. swarm_worker_w1 может complete_task(..., 'w1', ...).

Негативные (WK003):

6. swarm_worker_w1 НЕ может register_worker('w2', ...).
7. swarm_worker_w1 НЕ может claim_task('w2', ...).
8. swarm_worker_w1 НЕ может heartbeat(..., 'w2', ...).
9. swarm_worker_w1 НЕ может save_artifact(..., 'w2', ...).
10. swarm_worker_w1 НЕ может complete_task(..., 'w2', ...).

Нормализация:

11. swarm_worker_w1 может claim_task('w-1', ...) (дефис → подчёркивание).
12. swarm_worker_gpu_1 может claim_task('GPU-1', ...) (регистр + дефис).

Прочие роли не затронуты:

13. swarm_cli может add_task, submit_review, reopen_task.
14. swarm_planner может add_task, add_dep.
15. swarm_watchdog может reclaim_expired, promote_ready.
16. swarm_worker (общая) может всё, что раньше (проверка не сработает,
    потому что session_user = swarm_worker, не swarm_worker_%).

3.3. Мутации (в отчёт)

Проверить, что тесты ловят поломки:

· M1: Убрать perform assert_worker_identity(p_worker); из
  heartbeat → тест 8 падает.
· M2: В assert_worker_identity заменить like 'swarm_worker_%' на
  like 'swarm_worker_%' and false → тесты 6–10 падают.
· M3: В normalize_worker_id убрать lower(...) → тест 12 падает.

Если хоть одна не ловится — провал критерия, стоп и вопрос автору.

4. Критерии готовности

1. pytest step2b/test_roles_2b.py → в отчёте список имён тестов и
   итог (например, 16 passed).
2. pytest step2/test_roles.py → 30 passed (не сломано).
3. pytest tests/test_v0.py → 26 passed.
4. cargo test --workspace --locked → без регрессий.
5. Мутации M1/M2/M3 — 3 из 3 ловятся, указать какие тесты упали.
6. Ручные проверки (команда и вывод):
   · psql -U swarm_worker_w1 -c "select claim_task('w1', 10)" → OK
   · psql -U swarm_worker_w1 -c "select claim_task('w2', 10)" → WK003
7. Прогон на двух версиях Postgres: 16.4-alpine (compose) и 18.6
   (native). Docker-часть — если доступен sudo; иначе native-only с
   отметкой в отчёте.

5. Не делать

· Не менять schema_v0.sql, step2/roles.sql, step2/test_roles.py.
· Не добавлять mTLS, OPA, подписи Ed25519, мультитенантность.
· Не вводить таблицу worker_identities — соглашение об именах достаточно.
· Не трогать crates/swarm-core/ — Rust-обёртка не меняется (проверка
  встроена в SQL, воркер передаёт worker_id как раньше).

6. Известные ограничения после step 2b (записать в KNOWN_LIMITATIONS)

· session_user — не current_user. В SECURITY DEFINER функциях
  current_user был бы swarm_owner. Проверка по session_user — верна,
  но требует понимания этого различия при чтении кода.
· Общая роль swarm_worker остаётся. Если prod-среда использует общую
  роль вместо индивидуальных — проверка не сработает. Это нужно явно
  указать в операционных инструкциях.
· SET SESSION AUTHORIZATION может подменить session_user, но требует
  суперпользователя. Обычный воркер не может.
· mTLS между воркерами — не реализован. Пароль БД — единственный барьер.
  Это v2.0.

7. Формат отчёта

REPORT_v0.2_step2b.md одним файлом:

· Список изменённых/созданных файлов.
· Полный git diff или содержимое файлов.
· Вывод всех тестов.
· Результаты мутаций.
· Что не сработало или не проверено.
· Стоп и вопрос автору — если что-то не так.

Не переходить к Planner, пока step 2b не принят.

