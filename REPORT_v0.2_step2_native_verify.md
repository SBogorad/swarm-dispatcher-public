# Отчёт: независимая верификация step 2 (роли БД) + базовая линия

**Дата:** 2026-10-08
**Агент:** OpenClaw (nouthp88)
**Окружение:** native Postgres 18.6 (user-space, порт 55440), Python 3.12.14 (venv), psycopg2-binary 2.9.13, pytest 9.1.1
**Что НЕ проверено:** Docker / `postgres:16.4-alpine`, Rust-часть step 1 (`cargo`), CLI под ролями, compose-сценарий. См. «Не проверено».

---

## 1. Что сделано

1. Сохранены восстановленные автором файлы:
   - `tests/test_v0.py` (26 тестов) — md5 `e920b59abbd37db1861908a20f7ffe89`
   - `docs/ARCHITECTURE.md` (6 разделов, 9 mermaid-блоков) — md5 `9af85db97ba09c8ce5c36dc2ae5bc553`
2. Создано изолированное Python-окружение `.venv` (uv, CPython 3.12) с `psycopg2-binary`, `pytest`.
3. Подняты тестовые БД на native Postgres: `v0test` (schema), `v0roles` (schema + roles).
4. Прогнаны критерии 1, 2 и 8 ТЗ step 2.

---

## 2. Критерий 1 — `test_roles.py` (native PG 18.6)

```
$ ROLES_DB=v0roles ROLES_HOST=127.0.0.1 ROLES_PORT=55440 ADMIN_PW=pw \
    SWARM_PW_{WORKER,WATCHDOG,PLANNER,REVIEWER,OPERATOR,CLI}=pw_* \
    .venv/bin/python -m pytest -q step2/test_roles.py
30 passed in 0.86s
```

**Совпадает с результатом автора (30 passed на PG 16.13).** Ни одного skipped.
`roles.sql` и `test_roles.py` не менялись (md5 совпадают с присланными).

---

## 3. Базовая линия — `test_v0.py`

```
$ PGDSN="dbname=v0test user=swarm host=127.0.0.1 port=55440" \
    .venv/bin/python -m pytest -q tests/test_v0.py
26 passed in 0.98s
```

---

## 4. Критерий 2 — `test_v0.py` на базе с применёнными ролями

```
$ PGDSN="dbname=v0roles user=postgres host=127.0.0.1 port=55440 password=pw" \
    .venv/bin/python -m pytest -q tests/test_v0.py
26 passed in 0.96s
```

Регрессии нет: роли не сломали инварианты схемы.

Проверка каталога `v0roles`:
- функций в `public`: **20**, из них `SECURITY DEFINER`: **20**;
- владелец всех функций: **`swarm_owner`** (не суперпользователь);
- роли: `swarm_owner`, `swarm_worker`, `swarm_watchdog`, `swarm_planner`, `swarm_reviewer`, `swarm_operator`, `swarm_cli`.

---

## 5. Критерий 8 — мутации `roles.sql`

Каждая мутация применялась к свежей БД (`mdb`) поверх `schema_v0.sql`, затем
прогонялся `test_roles.py`. Критерий: `failed > 0`.

| Мутация | Что изменено | Результат | Поймано |
|---------|--------------|-----------|---------|
| **M1** | добавлен `grant execute on function submit_review to swarm_worker;` | `2 failed, 28 passed` | ✅ |
| **M3** | `search_path = public, pg_temp` → `pg_temp, public` | `2 failed, 28 passed` | ✅ |
| **M5** | убрано `security definer` из `alter function` | `11 failed, 19 passed` | ✅ |

Поймавшие тесты:
- **M1:** `test_function_matrix[swarm_worker]`, `test_worker_cannot_approve_own_work`
- **M3:** `test_catalog_invariants`, `test_temp_table_cannot_hijack_security_definer`
- **M5:** `test_cli_role_is_union_but_not_worker`, `test_task_status_shows_state_but_not_contract` + 9 прочих

**Все три мутации ловятся** — критерий 8 выполнен. Совпадает с ожиданием
автора (10 из 11 поломок ловятся; непойманная — `REVOKE` на схему `public`,
задокументирована как несущественная для PG ≥ 15).

---

## 6. Изменения в тестовом окружении (не в репозитории)

- Создана роль-суперпользователь `postgres` (пароль `pw`) в **native-кластере** —
  требуется `test_roles.py` как admin-хелпер. Кластер пользовательский, тестовый.
- Созданы БД `v0test`, `v0roles`. Временная `mdb` удалена после мутаций.
- В репозитории появился `.venv/` (Python-окружение). Стоит добавить в
  `.gitignore`, чтобы не коммитить.

---

## 7. Не проверено (осталось)

1. **Docker / compose** (критерии 4–7 ТЗ step 2): `postgres:16.4-alpine`,
   `docker compose up -d --build`, раздельные DSN по сервисам, `02-roles.sh`,
   e2e-сценарий под ролями. Требует sudo-пароль (Docker).
2. **Rust / step 1** (`cargo build --locked`, `DispatcherTx`, сырой JSON в `Lease`):
   ждёт ТЗ step 1 v2. Сейчас в `crates/` старая версия.
3. **CLI под ролями** (`swarm-cli status` через `task_status()`): требует
   сборки step 1.
4. **Критерий 9** (PG 16.4-alpine + 18.6): 18.6 закрыт этим отчётом, 16.4 — в compose.

---

## 8. Вывод

Критерии **1, 2, 8** ТЗ step 2 закрыты на native PG 18.6. `roles.sql` и
`test_roles.py` работают без изменений, мутации ловятся, регрессии в схеме нет.
Готов к продолжению: compose-часть step 2 и Rust-часть step 1 — по получении
ТЗ step 1 v2 и файлов от Claude.
