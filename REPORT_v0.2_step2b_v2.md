# Отчёт: верификация step 2b v2 (идентификация воркера)

**Дата:** 2026-10-08
**Верификатор:** OpenClaw (nouthp88)
**ТЗ:** `docs/TZ_v0.2_step2b_worker_identity.md` (+ патч §3 к SQL)
**Пакет:** доставлен DeepSeek, ревизия 2 (после исправления обрыва и теста 11)
**Окружение:** native Postgres 18.6 (user-space, порт 55440), Python 3.12.14 (`.venv`), psycopg2-binary 2.9.13, Rust 1.98.1
**Статус:** ✅ **принят** — полный набор проходит, все мутации ловятся, регрессий нет.

---

## 1. Что изменилось от первой доставки

| Дефект v1 | Исправление v2 |
|-----------|----------------|
| `test_roles_2b.py` обрезан (4 теста + недописанный) | полный файл, 19 тестов |
| Тест 11 внутренне противоречив | разбит на `test_11` (позитив для `swarm_worker_w_1`) + `test_11b` (негатив для `swarm_worker_w1`) |
| В SQL не было ролей `swarm_worker_w_1`, `swarm_worker_gpu_1` | патч §3: добавлены обе роли + пароли + членство |

Замечание: автор в резюме пишет «20 тестов», но сумма его же категорий = 19 (5+5+3+4+2). Фактически в файле **19 тестов** — расхождение только в тексте описания, не в файле.

---

## 2. Доставленные артефакты (md5)

| Файл | Строк | md5 |
|------|-------|-----|
| `step2b/test_roles_2b.py` (v2, полный) | 224 | — |
| `step2b/roles_2b.sql` (после патча §3) | 199 | `0cb5c0b4d176e3a13c2cc13689f25233` |
| `docs/TZ_v0.2_step2b_worker_identity.md` | 200 | `dfaa5bfc7b15e3fd6abcdd36dfe55f2d` |
| `step2b/test_roles_2b.DELIVERED_partial.py` (v1, обрезанный — сохранён для истории) | 97 | `6176c13bcba716db0b171c5ad4cbc5f6` |

Порядок применения (из ТЗ): `schema_v0.sql → step2/roles.sql → step2b/roles_2b.sql`.
Запуск SQL с четырьмя паролями: `-v w1_pw=… -v w2_pw=… -v w_1_pw=… -v gpu_1_pw=…`.

Патч применён точечно: секция «3. Роли для тестов» заменена на версию из сообщения (добавлены `swarm_worker_w_1`, `swarm_worker_gpu_1` и их grant). Остальной SQL не тронут.

---

## 3. Критерий 1 — полный `test_roles_2b.py`

```
......................... [100%]
19 passed in 0.65s
```

Полный список (все PASSED):

```
test_01_w1_register_self
test_02_w1_claim_self
test_03_w1_heartbeat_self
test_04_w1_save_artifact_self
test_05_w1_complete_self
test_06_w1_cannot_register_w2
test_07_w1_cannot_claim_w2
test_08_w1_cannot_heartbeat_w2
test_09_w1_cannot_save_artifact_as_w2
test_10_w1_cannot_complete_as_w2
test_11_hyphen_normalization
test_11b_hyphen_wrong_role_rejected
test_12_case_and_hyphen_normalization
test_13_cli_role_unaffected
test_14_planner_role_unaffected
test_15_watchdog_role_unaffected
test_16_common_worker_role_unchanged
test_17_catalog_identity_functions
test_18_assert_worker_identity_not_granted_to_public
```

Прогон на **свежей БД** `v0_2b` (пересоздана с нуля: schema → roles → roles_2b). Проверено, что в функциях БД нет мутантов (`select count(*) … prosrc like '%MUTANT%'` = 0).

**Важно:** при первом контрольном прогоне тест 8 упал — оказалось, БД была загрязнена мутантом M1 от предыдущего шага (я перепутал порядок пересоздания). После честного пересоздания — 19/19. Это ошибка процедуры верификации, а не пакета; учтено и перепроверено.

---

## 4. Критерий 2 — `test_roles.py` (не сломано)

```
30 passed in 0.82s
```

## 5. Критерий 3 — `tests/test_v0.py`

```
26 passed in 1.00s
```

## 6. Критерий 4 — cargo (без регрессий)

| Проверка | Результат |
|----------|-----------|
| `cargo test --workspace --lib` | 8 passed |
| `cargo test -p swarm-core --test integration -- --ignored` | 10 passed |
| `cargo test -p swarm-core --test integration_alt -- --ignored` | 3 passed |

---

## 7. Критерий 5 — мутации (3 из 3 ловятся)

Каждая — отдельный мутант на свежей БД, затем откат; БД между мутациями пересоздавалась.

| № | Мутация | Какие тесты упали | Поймано |
|---|---------|-------------------|---------|
| **M1** | убран `perform assert_worker_identity(p_worker);` из `heartbeat` | `test_08_w1_cannot_heartbeat_w2` (1 failed, 18 passed) | ✅ |
| **M2** | в `assert_worker_identity` условие `… and false` (всегда no-op) | `test_06`, `test_07`, `test_08`, `test_09`, `test_10`, `test_11b` (6 failed, 13 passed) | ✅ |
| **M3** | в `normalize_worker_id` убран `lower(...)` | `test_12_case_and_hyphen_normalization` (1 failed, 18 passed) | ✅ |

M2 теперь ловит и новый `test_11b` — это подтверждает, что негативный контроль из v2 работает.

---

## 8. Критерий 6 — ручные psql-проверки

```
$ psql -U swarm_worker_w1 -d v0_2b -c "select * from claim_task('w1', 60)"
  → OK (задача выдана), epoch=1

$ psql -U swarm_worker_w1 -d v0_2b -c "select * from claim_task('w2', 60)"
ERROR: worker_identity_mismatch: session_user=swarm_worker_w1 expected=swarm_worker_w2

$ psql -U swarm_worker_w1 -d v0_2b -c "select * from claim_task('w-1', 60)"
ERROR: worker_identity_mismatch: session_user=swarm_worker_w1 expected=swarm_worker_w_1

$ psql -U swarm_worker_w_1 -d v0_2b -c "select * from claim_task('w-1', 60)"
  → OK (нормализация дефиса)

$ psql -U swarm_worker_gpu_1 -d v0_2b -c "select * from claim_task('GPU-1', 60)"
  → OK (нормализация регистра + дефиса)
```

Каталог: 22 функции в `public`; `normalize_worker_id` и `assert_worker_identity` — `SECURITY DEFINER`, owner `swarm_owner`, `search_path=public, pg_temp`; `assert_worker_identity` не выдан никому (`PUBLIC`/`swarm_worker`/`swarm_worker_w1` → `has_function_privilege = false`).

---

## 9. Критерий 7 — две версии Postgres

- **Native PG 18.6** — проверено полностью (см. выше).
- **PG 16.4-alpine (compose)** — ⚠️ не проверено (нет sudo). Все прочие критерии закрыты на native. Трекается в `docs/KNOWN_LIMITATIONS.md` §5.5, закрывается батчем.

---

## 10. Что осталось незакрытым (не блокирует приёмку step 2b)

1. Docker/compose (PG 16.4-alpine, `02-roles.sh`, раздельные DSN, e2e) — нет sudo. Трекается.
2. Rust-код не менялся (проверка целиком в SQL) — регрессии zero.
3. Расхождение в тексте автора («20 тестов» vs фактические 19) — не влияет на файл.

---

## 11. Вывод

Step 2b **принят**. Все критерии ТЗ, выполнимые на native, закрыты:

| Критерий | Результат |
|----------|-----------|
| 1. `test_roles_2b.py` | **19 passed** ✅ |
| 2. `test_roles.py` (регрессия) | **30 passed** ✅ |
| 3. `test_v0.py` (регрессия) | **26 passed** ✅ |
| 4. cargo (lib/integration/alt) | **8 / 10 / 3 passed** ✅ |
| 5. Мутации M1/M2/M3 | **3 из 3 ловятся** ✅ |
| 6. Ручные psql-проверки | ✅ выполнены |
| 7. Вторая версия Postgres | ⚠️ Docker отложен (нет sudo) |

Идентификация воркера работает: чужие `register_worker`/`claim_task`/`heartbeat`/`save_artifact`/`complete_task` → `WK003`; своя роль проходит; нормализация регистра и дефиса подтверждена; общая роль `swarm_worker` не затронута; прочие роли без изменений.

После этого отчёта — можно переходить к Planner (ТЗ `docs/TZ_v0.2_step2_planner.md`, с уточнениями по замечаниям Claude: детерминированный `root_id`, лимиты в контракт корня явно, `with_transaction` для атомарной вставки).
