# Отчёт: верификация step 2b (идентификация воркера)

**Дата:** 2026-10-08
**Верификатор:** OpenClaw (nouthp88)
**ТЗ:** `docs/TZ_v0.2_step2b_worker_identity.md` (доставлено DeepSeek)
**Окружение:** native Postgres 18.6 (user-space, порт 55440), Python 3.12.14 (`.venv`), psycopg2-binary 2.9.13, Rust 1.98.1
**Статус:** ⚠️ **принято с оговорками** — SQL верифицирован полностью, доставленный тест-файл обрезан при передаче (см. §1).

---

## 1. Что доставлено (как есть, md5 зафиксирован)

| Файл | Строк | md5 | Состояние |
|------|-------|-----|-----------|
| `docs/TZ_v0.2_step2b_worker_identity.md` | 200 | `dfaa5bfc7b15e3fd6abcdd36dfe55f2d` | ✅ полный |
| `step2b/roles_2b.sql` | 185 | `a340a16b0b1f20675ccd4806013062ad` | ✅ полный |
| `step2b/test_roles_2b.py` (как доставлено) | 97 | `6176c13bcba716db0b171c5ad4cbc5f6` | ❌ **обрезан** |

**Обрыв теста.** Исходное сообщение (`input-12ответ…md`, 505 строк) обрывается на строке 505 внутри `test_w1_complete_self`; закрывающего ``` для python-блока нет. В доставленном виде файл содержит объявленными только:

```
test_w1_register_self
test_w1_claim_self
test_w1_heartbeat_self
test_w1_save_artifact_self
test_w1_complete_self   ← без тела/ассерта
```

То есть отсутствуют тесты 5–16 из ТЗ (5 негативных WK003, 2 нормализации, 4 «прочие роли», проверка каталога). Синтаксически файл парсится (`ast.parse` OK), но логически неполон и запуску не подлежит.

Сохранён в репозитории как `step2b/test_roles_2b.DELIVERED_partial.py` — как доставлено, без правок. **Требуется хвост файла от автора** (см. §7).

---

## 2. Установка и применение SQL

Порядок из ТЗ соблюдён: `schema_v0.sql → step2/roles.sql → step2b/roles_2b.sql`.

```
drop/create database v0_2b
schema_v0.sql   → OK
roles.sql       → OK
roles_2b.sql    → OK (CREATE ROLE / ALTER ROLE / GRANT ROLE)
```

Роли после применения: `swarm_owner, swarm_worker, swarm_watchdog, swarm_planner, swarm_reviewer, swarm_operator, swarm_cli, swarm_worker_w1, swarm_worker_w2`.

Скрипт идемпотентен (повторный прогон — NOTICE, не ошибка).

---

## 3. Независимая верификация SQL (16 критериев ТЗ + каталог)

Поскольку доставленный тест-файл обрезан, SQL проверен **собственным независимым харнессом** (24 проверки, свежая БД на каждый прогон). Результат: **24/25 PASS**.

### 3.1. Позитивные — своя роль / своё имя

| № | Проверка | Результат |
|---|----------|-----------|
| 1 | `swarm_worker_w1` → `register_worker('w1')` | ✅ PASS |
| 2 | `swarm_worker_w1` → `claim_task('w1')` | ✅ PASS |
| 3 | `swarm_worker_w1` → `heartbeat(…,'w1',…)` | ✅ PASS |
| 4 | `swarm_worker_w1` → `save_artifact(…,'w1',…)` | ✅ PASS |
| 5 | `swarm_worker_w1` → `complete_task(…,'w1',…)` | ✅ PASS |

### 3.2. Негативные — WK003

| № | Проверка | Результат |
|---|----------|-----------|
| 6 | `w1` → `register_worker('w2')` | ✅ WK003 |
| 7 | `w1` → `claim_task('w2')` | ✅ WK003 |
| 8 | `w1` → `heartbeat(…,'w2',…)` | ✅ WK003 |
| 9 | `w1` → `save_artifact(…,'w2',…)` | ✅ WK003 |
| 10 | `w1` → `complete_task(…,'w2',…)` | ✅ WK003 |

### 3.3. Нормализация

| № | Проверка | Результат |
|---|----------|-----------|
| 11 | `w1` → `claim_task('w-1')` (**буквально как в ТЗ**) | ❌ **FAIL: WK003** (см. §4) |
| 11b | `swarm_worker_w_1` → `claim_task('w-1')` (корректная пара) | ✅ PASS |
| 12 | `swarm_worker_gpu_1` → `claim_task('GPU-1')` (регистр+дефис) | ✅ PASS |

### 3.4. Прочие роли не затронуты

| № | Проверка | Результат |
|---|----------|-----------|
| 13 | `swarm_cli`: add_task / submit_review / reopen_task | ✅ PASS |
| 14 | `swarm_planner`: add_task / add_dep | ✅ PASS |
| 15 | `swarm_watchdog`: reclaim_expired / promote_ready | ✅ PASS |
| 16 | общая `swarm_worker`: register + claim без проверки | ✅ PASS (проверка не срабатывает, как и задумано) |

### 3.5. Каталог

| № | Проверка | Результат |
|---|----------|-----------|
| C1 | 22 функции в `public` (19 + normalize + assert + task_status) | ✅ PASS |
| C2 | новые функции — `SECURITY DEFINER`, owner `swarm_owner`, `search_path=public, pg_temp` | ✅ PASS |
| C3 | `assert_worker_identity` закрыт для воркера | ✅ PASS |
| C4 | `normalize_worker_id` открыт (PUBLIC) | ✅ PASS |

---

## 4. Единственный FAIL — тест 11 из ТЗ внутренне противоречив

ТЗ (§3.2, критерий 11) требует: «`swarm_worker_w1` может `claim_task('w-1')` (дефис → подчёркивание)».

Но по правилу §2.2 ожидаемое имя роли = `swarm_worker_` || normalize(`p_worker`):

```
normalize_worker_id('w-1') = 'w_1'
⇒ ожидаемая роль = swarm_worker_w_1
но session_user  = swarm_worker_w1
⇒ WK003 worker_identity_mismatch
```

Эмпирически подтверждено:

```
$ psql -U swarm_worker_w1 … -c "select * from claim_task('w-1', 60)"
ERROR:  worker_identity_mismatch: session_user=swarm_worker_w1 expected=swarm_worker_w_1
```

**Это дефект формулировки теста, а не SQL.** Корректная пара роль↔имя (`swarm_worker_w_1` → `w-1`, проверка 11b) проходит. Вариант 12 (`swarm_worker_gpu_1` → `GPU-1`) подтверждает нормализацию регистра+дефиса и проходит.

Автору: уточнить тест 11 — либо роль должна называться `swarm_worker_w_1`, либо (предпочтительно) в тесте использовать `swarm_worker_gpu_1`/`GPU-1` как основной кейс нормализации, а `w1`/`w-1` — как негативный (WK003).

---

## 5. Мутации (§3.3 ТЗ)

Каждая — отдельный мутант `roles_2b.sql` на свежей БД, затем откат. Все три пойманы:

| № | Мутация | Какие проверки упали | Поймано |
|---|---------|----------------------|---------|
| **M1** | убран `perform assert_worker_identity(p_worker);` из `heartbeat` | 8 (`w1 heartbeat('w2')` → уже не WK003) | ✅ |
| **M2** | в `assert_worker_identity` условие `… and false` (всегда no-op) | 6, 7, 8, 9, 10 (все WK003-проверки) | ✅ |
| **M3** | в `normalize_worker_id` убран `lower(...)` | 12 (`swarm_worker_gpu_1` → `'GPU-1'` → WK003) | ✅ |

Итоги прогонов мутантов: M1 → 23/25, M2 → 20/25, M3 → 23/25 (в каждом случае падают именно ожидаемые проверки). Оригинал — 24/25 (единственный FAIL — тест 11, см. §4).

---

## 6. Регрессии (критерии 2–4 ТЗ)

Все на свежей БД `v0_2b` (schema + roles + roles_2b) после откатов:

| Проверка | Результат |
|----------|-----------|
| `pytest tests/test_v0.py` (26 SQL-тестов схемы) | **26 passed** ✅ |
| `pytest step2/test_roles.py` (30 тестов ролей step 2) | **30 passed** ✅ |
| `cargo test --workspace --lib` | **8 passed** ✅ |
| `cargo test -p swarm-core --test integration -- --ignored` | **10 passed** ✅ |
| `cargo test -p swarm-core --test integration_alt -- --ignored` | **3 passed** ✅ |

Регрессий нет: правки step 2b не сломали ни схему, ни роли step 2, ни Rust-обёртку. Rust-код не менялся (проверка целиком в SQL).

---

## 7. Что не проверено / требуется

1. **Доставленный `test_roles_2b.py` обрезан** — тесты 5–16 отсутствуют. Требуется хвост файла от автора; после получения прогоню как есть (файл сохранён в `step2b/test_roles_2b.DELIVERED_partial.py` как доставлено, md5 `6176c13b…`).
2. **Тест 11** — противоречив (см. §4), нужно решение автора по формулировке.
3. **Docker / compose (Postgres 16.4-alpine)** — не проверено (нет sudo). Native PG 18.6 проверен полностью. Трекается в `docs/KNOWN_LIMITATIONS.md` §5.5, закрыть батчем.
4. **Ручные psql-проверки** из критерия 6 ТЗ выполнены (см. §3.2/§4), вывод приведён.

---

## 8. Что задокументировать в KNOWN_LIMITATIONS (§6 ТЗ)

После приёмки добавить раздел про ограничения step 2b (пока не добавлено, ждёт приёмки):

- `session_user` ≠ `current_user`: в `SECURITY DEFINER`-функциях `current_user` был бы `swarm_owner`; проверка по `session_user` верна, но требует понимания различия.
- Общая роль `swarm_worker` остаётся: если prod-среда использует её вместо индивидуальных ролей — проверка не срабатывает. Указать явно в операционных инструкциях.
- `SET SESSION AUTHORIZATION` подменяет `session_user`, но требует суперпользователя; обычный воркер не может.
- mTLS между воркерами не реализован; пароль БД — единственный барьер (v2.0).

---

## 9. Вывод

SQL `step2b/roles_2b.sql` **работоспособен и верифицирован**: установка чистая, 24/25 независимых проверок, все 3 мутации ловятся, регрессий нет (26/30/8/10/3). Проверка идентификации работает: чужие `register_worker`/`claim_task`/`heartbeat`/`save_artifact`/`complete_task` → WK003; своя роль проходит; нормализация регистра/дефиса подтверждена.

**Два блокера приёмки:**
1. Доставленный тест-файл обрезан — нет тестов 5–16 (§1).
2. Тест 11 ТЗ внутренне противоречив (§4).

Пока оба не закрыты, step 2b не считается принятым, Planner не начинаю.
