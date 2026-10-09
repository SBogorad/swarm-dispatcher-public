# Отчёт: step 1 фазы v0.2 (правки v0.1)

**Дата:** 2026-10-08
**Агент:** OpenClaw (nouthp88)
**ТЗ:** `docs/TZ_v0.2_step1_fixes.md` (версия 2, финальная)
**Окружение:** native Postgres 18.6 (user-space, порт 55440), Rust 1.98.1 / cargo 1.98.1, Python 3.12.14 (`.venv`), psycopg2-binary 2.9.13, pytest 9.1.1
**Что НЕ проверено:** Docker / `docker compose up` (критерии 7–8), end-to-end в compose (критерий 9), запуск под сервисными ролями в контейнерах. Нужен sudo-пароль. См. §8.

---

## 1. Изменённые файлы

| Файл | Что сделано |
|------|-------------|
| `Cargo.toml` | `rust-version` 1.75 → **1.85** |
| `crates/swarm-core/src/errors.rs` | `Database(#[from]` → `#[source]`; добавлен комментарий-правило про `map_err` |
| `crates/swarm-core/src/types.rs` | `Lease.contract: TaskContract` → `serde_json::Value` + `typed_contract()`; добавлен `TaskStatusRow` |
| `crates/swarm-core/src/dispatcher.rs` | `connect` через `map_err`; `claim` без `from_value`; добавлен `list_task_status()`; добавлен `DispatcherTx<'a,'b>` + `with_transaction` |
| `crates/swarm-core/src/lib.rs` | Реэкспорт `DispatcherTx`, `TaskStatusRow` |
| `crates/swarm-core/Cargo.toml` | Зарегистрирован test-target `integration_alt` |
| `crates/swarm-core/tests/integration.rs` | +5 новых тестов (см. §4) |
| `crates/swarm-core/tests/alt/integration_alt.rs` | Новый файл: 3 альтернативных теста |
| `crates/swarm-cli/src/main.rs` | `status()` переведён на `list_task_status(200)` (`task_status()`) |
| `Dockerfile` | `cargo build --release --locked --bins` |
| `docker-compose.yml` | Убран `version`; postgres монтирует `roles.sql` + `02-roles.sh`, получает `SWARM_PW_*`; у `cli` убран `command` |
| `scripts/02-roles.sh` | Новый: применение `roles.sql` после `01-schema.sql` |
| `.env.example` | Добавлены 6 переменных `SWARM_PW_*` |
| `.gitignore` | Новый: `.venv/`, `__pycache__/`, `target/`, `.env` |
| `docs/` | Консолидация: 6 файлов + `docs/optional/` (15) + `docs/history/` |
| `README.md` | Убран посторонний хвост («Файл 11/12»); числа, роли, команды |
| `docs/KNOWN_LIMITATIONS.md` | 3.2: убраны непроверенные «10 tasks/sec, запас 100x» |
| `docs/optional/ARCHITECTURE_DECISIONS.md`, `REPORT_TEMPLATE.md` | non-repudiation → tamper-evidence |
| `docs/TZ_v0.2_step1_fixes.md` | Заменено на v2; v1 → `docs/history/TZ_v0.2_step1_fixes_v1.md` |

`Cargo.lock` присутствует в репозитории, все сборки — `--locked`.

---

## 2. Критерий 1 — `cargo build --workspace --locked`

```
$ cargo build --workspace --locked
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.35s
```

**0 errors, 0 warnings** (проверено с `touch lib.rs` для принудительной пересборки).
Собраны все 4 бинарника: `swarm-watchdog`, `swarm-worker`, `swarm-cli`, `swarm-core`.

---

## 3. Критерий 2 — `cargo test --workspace --lib`

```
running 8 tests
test errors::tests::sqlstate_known_codes_map ... ok
test errors::tests::sqlstate_unknown_returns_none ... ok
test types::tests::contract_empty_checks_rejected ... ok
test types::tests::contract_deserialize_minimal ... ok
test types::tests::contract_valid ... ok
test types::tests::contract_serialize_omits_none ... ok
test types::tests::verdict_as_str ... ok
test types::tests::task_state_roundtrip ... ok

test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

---

## 4. Критерий 3 — `cargo test -p swarm-core --test integration -- --ignored --test-threads=1`

```
running 10 tests
test add_dep_cycle_rejected ... ok
test add_task_and_list ... ok
test claim_and_fencing ... ok
test complete_and_review_self_rejected ... ok
test concurrent_claims_are_disjoint ... ok
test contract_accepted_by_sql_but_not_by_rust_must_not_strand_the_task ... ok
test verify_chain_after_operations ... ok
test with_transaction_commits_whole_graph_atomically ... ok
test with_transaction_rolls_back_on_error ... ok
test with_transaction_rolls_back_on_sql_error_midway ... ok

test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

Было 5 тестов, добавлено 5 (§8.1 ТЗ). Итого **10 passed**.

Альтернативные тесты (`tests/alt/integration_alt.rs`, target `integration_alt`):

```
running 3 tests
test full_cycle_claim_save_complete_review ... ok
test sqlstate_mapping ... ok
test zombie_is_fenced_and_heartbeat_false ... ok

test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

---

## 5. Критерий 4 — `pytest tests/test_v0.py`

```
26 passed in 1.02s
```

---

## 6. Критерий 5 — `pytest step2/test_roles.py`

```
30 passed in 0.83s
```

Роли не сломаны правками step 1.

---

## 7. Критерий 6 — мутации (§8.2 ТЗ)

Все три мутации проверены на реальном Postgres. Каждая — отдельная правка
кода, прогон, откат (исходники восстановлены и сверены с бэкапом).

| № | Мутация | Какие тесты упали | Поймано |
|---|---------|-------------------|---------|
| **M1** | `claim` десериализует контракт (`serde_json::from_value(...).unwrap()`) | `contract_accepted_by_sql_but_not_by_rust_must_not_strand_the_task` | ✅ |
| **M2** | `with_transaction` коммитит даже при ошибке (ошибка замыкания проглочена) | `with_transaction_rolls_back_on_error` | ✅ |
| **M3** | `Lease::typed_contract` не разбирает (возвращает заглушку) | `full_cycle_claim_save_complete_review` (в `integration_alt`) | ✅ |

Детали:

- **M1:** паника `called Result::unwrap() on an Err value: Error("invalid type: floating point 2.5, expected u32", ...)` — ровно тот сценарий, ради которого `Lease.contract` сделан сырым JSON.
- **M2:** упал **только** тест с локальной ошибкой (`left: 1, right: 0` — вставка закоммитилась). Тест с SQL-ошибкой посреди графа **не** упал — Postgres сам откатывает транзакцию после SQL-ошибки. Это подтверждает замечание Claude: ловит мутацию именно тест с локальной ошибкой; тест с SQL-ошибкой проверяет другое (отсутствие частичного графа).
- **M3:** `assertion left == right failed: left: "MUTANT", right: "t"`.

После откатов: `cargo build` чист, lib 8 passed, integration 10 passed, integration_alt 3 passed.

---

## 8. Не проверено (критерии 7–9)

1. **Критерий 7** — `docker compose up -d --build`, 4 контейнера healthy. Нужен sudo-пароль (пользователь не в группе docker).
2. **Критерий 8** — `docker compose run --rm --no-deps cli swarm-cli seed/status` в контейнере.
3. **Критерий 9** — end-to-end: seed → `CRASH_AFTER_CLAIM=1` → `{"reclaimed":1}` → второй воркер → review → approve → done.

Косвенно готово к этим критериям: compose монтирует `schema_v0.sql` + `roles.sql` + `02-roles.sh`, пароли ролей проброшены, `cli` без `command`. End-to-end на **native** (без Docker) проверялся ранее в v0.1 и повторён в интеграционных тестах (claim/reclaim/review/done).

---

## 9. Замечания

1. **`docs/README`-склейка.** В сохранённом от автора `README.md` после строки 199 были приклеены блоки «Файл 11: `TZ_v0.2_step2_planner.md`» и «Файл 12: `REPORT_TEMPLATE.md`» (артефакт склейки нескольких «Файл N»-сообщений). Оба документа уже есть отдельными файлами в `docs/optional/`. Хвост из README убран (README: 199 строк). **Прошу подтвердить**, что это ожидаемо.
2. **Числа в одном месте.** «26 тестов», «19 функций» остались в README; в остальных документах — ссылки. Полная сверка дублей — отдельной задачей, если нужно.
3. **`DispatcherTx` — ровно 3 метода** (`add_task`, `add_dep`, `promote_ready`), как требует ТЗ.
4. **`task_status()`** добавлен в `list_task_status()`; старый `list_tasks()` (прямой SELECT) оставлен для тестов под суперпользователем.
5. **Dockerfile:** `COPY --chmod=644 schema/...` в текущей версии отсутствовал — убирать было нечего. Схема монтируется через volume.

---

## 10. Вывод

Step 1 v0.2 выполнен. Критерии **1–6** закрыты с полным выводом: сборка без ошибок/предупреждений, 8 unit + 10 integration + 3 alt, 26 SQL-тестов, 30 тестов ролей, 3 из 3 мутаций ловятся. Критерии 7–9 (Docker/compose) ждут sudo-пароля.
