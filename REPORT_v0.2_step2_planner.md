# REPORT v0.2 — Step 3 (Planner)

**Дата:** 2026-10-08
**Верификатор:** OpenClaw (Арго)
**Окружение:** native Postgres 18.6 на `127.0.0.1:55440`, роль `swarm`, БД `v0planner` / `v0test` / `v0roles` / `v0_2b`. Rust workspace `swarm-dispatcher`, `--locked`. LLM-демо — реальный relay.fast (`deepseek-v4-flash`, OpenAI-compatible).
**ТЗ:** `docs/TZ_v0.2_step2_planner.md` (v2).
**Статус:** ✅ **Критерии 1–8 закрыты. Критерий 9 (Docker) отложен — нужен sudo.**

> Примечание: репозиторий **не git** (`git rev-parse` → `fatal: not a git repository`), поэтому «git diff» неприменим — вместо него перечислены файлы и md5.

---

## 1. Список файлов (что изменено/создано)

| Файл | Строк | md5 |
|---|---|---|
| `crates/swarm-planner/src/lib.rs` | 273 | `f5ceb12bc71fa8529f94661f4c063da4` |
| `crates/swarm-planner/src/parse.rs` | 341 | `0ed1c9d8f76ce460c9a71d574423e84e` |
| `crates/swarm-planner/src/llm.rs` | 227 | `265227ad609940fddcbcb23aad317e11` |
| `crates/swarm-planner/src/prompt.rs` | 74 | `7636e9c2d697dbed45ab55730205057f` |
| `crates/swarm-planner/src/main.rs` | 80 | `57b7d04f523d576a0d71e33682bc0cdb` |
| `crates/swarm-planner/src/error.rs` | 23 | `659687601cc51837e45edf675bd3b67f` |
| `crates/swarm-planner/tests/planner_integration.rs` | 201 | `2d91056afe669c353de64f5c59a89e43` |
| `crates/swarm-planner/tests/fixtures/planner_ok.json` | 31 | — |
| `crates/swarm-planner/tests/fixtures/planner_too_many.json` | 18 | — |
| `crates/swarm-planner/tests/fixtures/planner_dup_dep.json` | 31 | `5711b2b984c111a0905695e0afd18da9` (новый) |
| `Cargo.toml` (workspace) | — | добавлен `crates/swarm-planner` |

`crates/swarm-core` **public API не тронут** (только использован `with_transaction`, `add_task`, `add_dep`, `promote_ready`). Схема (`schema/schema_v0.sql`) и `roles.sql` **не менялись**.

---

## 2. Критерии готовности (полный вывод)

### K1. `cargo build --workspace --locked` — 0 errors, 0 warnings
```
$ cargo build --workspace --locked
warn+err = 0
Finished `dev` profile [unoptimized + debuginfo] target(s) in 25.20s
```

### K2. `cargo test --workspace --lib` — unit
```
swarm-core:    test result: ok. 8 passed; 0 failed
swarm-planner: test result: ok. 14 passed; 0 failed
```
**Имена unit-тестов Planner (14):**
```
parse::tests::deterministic_root_id_differs
parse::tests::deterministic_root_id_stable
parse::tests::parse_invalid_json
parse::tests::parse_valid_json
parse::tests::parse_valid_json_from_code_block
parse::tests::validate_depends_exist
parse::tests::validate_depth
parse::tests::validate_limits
parse::tests::validate_no_cycles
parse::tests::validate_single_root
prompt::tests::prompt_contains_limits
prompt::tests::user_prompt_optional_parts
tests::root_contract_has_limits
tests::subtask_contract_defaults
```

### K3. `cargo test -p swarm-planner --test planner_integration -- --test-threads=1`
```
test result: ok. 8 passed; 0 failed
```
**Имена (8):**
```
planner_creates_graph
planner_idempotent_same_text
planner_idempotent_different_text
planner_atomic_on_sql_error
dispatcher_transaction_rolls_back_on_cycle   ← добавлен (см. §4)
planner_respects_max_tasks
planner_retries_on_invalid_json
e2e_planner_to_review
```
ТЗ ожидало 7. Стало 8: добавлен `dispatcher_transaction_rolls_back_on_cycle` (сохранён исходный dispatcher-уровневый контроль атомарности, пока `planner_atomic_on_sql_error` переведён на путь через `Planner::plan`).

### K4. `pytest tests/test_v0.py` — регрессия
```
26 passed in 1.00s
```

### K5. `pytest step2/test_roles.py`
```
30 passed in 0.86s
```

### K6. `pytest step2b/test_roles_2b.py`
```
19 passed in 0.64s
```

### K7. Мутации M1–M4 — 4 из 4 ловятся

| # | Мутация | Поймавший тест | Результат |
|---|---|---|---|
| M1 | `root_id = Uuid::new_v4()` | `planner_idempotent_same_text` | ✅ 7 passed, 1 failed |
| M2 | убрать лимиты из контракта корня | `tests::root_contract_has_limits` | ✅ 13 passed, 1 failed |
| M3 | `with_transaction` → последовательные вызовы | `planner_atomic_on_sql_error` | ✅ 7 passed, 1 failed |
| M4 | убрать проверку single root | `parse::tests::validate_single_root` | ✅ 13 passed, 1 failed |

После откатов: `MUTANT`-остатков в `crates/` — **0** (проверено `grep -rl MUTANT crates/`).

**Оговорки по M1 и M2:**
- **M1** ловится интеграционным тестом, но **не** unit-тестом `deterministic_root_id_stable` — мутация внесена в `plan()`, а unit-тест проверяет саму функцию `deterministic_root_id` напрямую. ТЗ ожидало падение обоих; фактически падает один, и этого достаточно (поведение изменено — тест это видит).
- **M2** ловится unit-тестом `root_contract_has_limits`, а **не** `planner_respects_max_tasks`: последний проверяет **локальную** валидацию `validate_graph`, которая читает лимиты из `PlannerConfig` (env), независимо от контракта корня. Лимиты в контракте корня — **вторая линия защиты** (SQL `add_task` подставит `max_tasks=200` вместо 50, если контракт их не задаёт). Это не дефект, но ТЗ ожидало иной тест.

### K8. Демонстрация: реальный LLM (relay.fast), не mock
```
$ swarm-planner "Написать функцию на Rust, которая складывает два числа, и покрыть её тестом"
root_id: e42748d9-83bb-5657-958f-7f7a84f5854d
tasks created: 2
ready to execute: 1

$ swarm-planner "<тот же текст>"          # идемпотентность
graph already exists (root_id=e42748d9-83bb-5657-958f-7f7a84f5854d, tasks=2)

$ swarm-cli status
id                                     state      epoch    owner       att   rw
da4a13e5-5767-5eb1-bbe8-f0e609f5ffbd   pending    0        -             0    0
e42748d9-83bb-5657-958f-7f7a84f5854d   ready      0        -             0    0
```
Граф создан реальным LLM через `https://relay.fast` (`PLANNER_LLM_PROVIDER=openai-compatible`), `swarm-cli status` его видит, повторный запуск не создаёт дублей.

### K9. Docker/compose — **отложено**
Нужен sudo-пароль (пользователь не в группе `docker`, rootless нет). Трекается в `docs/KNOWN_LIMITATIONS.md` §5.5; закрывается батчем вместе с step 1/2/2b.

---

## 3. Исправления, внесённые при верификации

### 3.1. Баг root_id (критично)
Корень получал `Uuid::new_v5(&root_id, "t1")` — вложенный v5 вместо самого `root_id`. `PlanOutcome.root_id` не совпадал с `tasks.root_id` в БД, из-за чего падали `planner_creates_graph`, `planner_idempotent_*`.
**Фикс:** корень получает сам `root_id` (по ТЗ §3), подзадачи — `Uuid::new_v5(&root_id, t.id)`.

### 3.2. Семантика `PlanOutcome`
Добавлено поле `existing_tasks: usize`; `tasks_created` теперь всегда = создано **в этом запуске** (при идемпотентном пропуске = 0). `main.rs` печатает число задач существующего графа через `existing_tasks`.

### 3.3. Дефект теста атомарности (M3 не ловился)
Доставленный `planner_atomic_on_sql_error` дёргал `d.with_transaction(...)` **напрямую**, минуя `Planner::plan` → `insert_graph`. Поэтому мутация M3 (замена `with_transaction` на последовательные `add_task`) **не детектировалась**.
**Фикс:** тест переведён на путь через `Planner::plan` с новым фикстуром `planner_dup_dep.json` (граф проходит локальную валидацию, но падает на SQL — дубль зависимости → PK violation на `add_dep`). Исходная dispatcher-уровневая проверка сохранена отдельным тестом `dispatcher_transaction_rolls_back_on_cycle`.

---

## 4. Что не сработало / не проверено

1. **Docker/compose (K9)** — не проверено, нет sudo. Отложено.
2. **`crates/swarm-core/tests/integration_verified.rs`** (8 тестов) — при прогоне `cargo test --workspace --test '*'` падает с `ConnectionRefused`: в файле **жёстко зашит DSN** `host=localhost user=postgres password=pw dbname=v0test` (порт 5432 по умолчанию), а наш PG слушает `55440`. Это **предсуществующий** артефакт, к Planner отношения не имеет (Planner его не трогает). Рекомендация: заменить константу на чтение `TEST_DATABASE_URL`, либо пометить `#[ignore]`.
3. **`integration_verified.rs` + `integration.rs` (10) + `integration_alt.rs` (3)** — требуют `-- --ignored` и/или отдельного DSN; в этом прогоне помечены ignored/не поднимались (см. §2 K4–K6 — SQL-регрессии закрыты pytest-набором).
4. **M2** — ловится не тем тестом, что ожидало ТЗ (см. §2 K7).
5. **Сеть до relay.fast** — при демонстрации возможны задержки; использован `timeout 180`.

---

## 5. Стоп / вопрос автору

1. **K3 = 8 тестов вместо 7** — добавлен `dispatcher_transaction_rolls_back_on_cycle`. Ок?
2. **M2 ловится unit-тестом, а не `planner_respects_max_tasks`** — оставить как есть (контракт корня = вторая линия защиты), или добавить отдельный тест, который проверяет лимиты именно в контракте корня?
3. **`integration_verified.rs`** — починить DSN (на `TEST_DATABASE_URL`) или оставить как есть и пометить `#[ignore]`?
4. **Docker (K9)** — присылать sudo-пароль для закрытия батча step 1/2/2b/Planner, или ждать отдельного шага?

---

## 6. Итог

| Критерий | Статус |
|---|---|
| 1. build --workspace --locked (0/0) | ✅ |
| 2. unit (8 core + 14 planner) | ✅ |
| 3. planner_integration | ✅ 8 passed |
| 4. test_v0.py | ✅ 26 passed |
| 5. test_roles.py | ✅ 30 passed |
| 6. test_roles_2b.py | ✅ 19 passed |
| 7. Мутации M1–M4 | ✅ 4 из 4 |
| 8. Демо на реальном PG + реальном LLM | ✅ |
| 9. Docker | ⏸ отложено (sudo) |

**Планировщик реализован, верифицирован на native, критерии 1–8 закрыты.** До приёмки автором — стоп; **v0.3 (LLM-worker) не начинаю.**
