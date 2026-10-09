# REPORT v0.3 — LLM-worker

**Дата:** 2026-10-08
**Верификатор:** OpenClaw (Арго)
**Окружение:** native Postgres 18.6 на `127.0.0.1:55440`; БД `v0worker` (интеграционные), `v0demo` (end-to-end), `v0test`/`v0roles`/`v0_2b`/`v0planner` (регрессии). Rust workspace, `--locked`.
**ТЗ:** `docs/TZ_v0.3_llm_worker.md`.
**Статус:** ✅ **Критерии 1–9 закрыты. Критерий 10 (Docker) отложен — нужен sudo.**

---

## 1. Список файлов + md5

### Новый crate `swarm-llm` (§2 — вынос LLM-клиента из Planner)
| Файл | Строк | md5 |
|---|---|---|
| `crates/swarm-llm/Cargo.toml` | 11 | `1ab92f61bd2f9873eff6638dd6a72b64` |
| `crates/swarm-llm/src/lib.rs` | 396 | `8e17e963cf38a2dda30848614d17a07e` |
| `crates/swarm-llm/src/error.rs` | 43 | `98500d1bc89e4dcb757b7632ee67603d` |

### `swarm-worker` (§3–7)
| Файл | Строк | md5 |
|---|---|---|
| `crates/swarm-worker/Cargo.toml` | 23 | `a33361ca08454793f1cf325ba498d039` |
| `crates/swarm-worker/src/lib.rs` | 14 | `0ec1f79f2cf7e1ea513d64fcee1f9e06` |
| `crates/swarm-worker/src/main.rs` | 126 | `fa5f1b3177e029f10eb786d976a3ec9b` |
| `crates/swarm-worker/src/worker.rs` | 254 | `dac0f273fcb19a38bb4d3e9adb088372` |
| `crates/swarm-worker/src/artifacts.rs` | 173 | `9aa1011b1580118eb0c2b8cfe8ea7059` |
| `crates/swarm-worker/src/prompt.rs` | 73 | `8c78de1b5e5718d776970d7b2f11c772` |
| `crates/swarm-worker/src/retry.rs` | 87 | `6b54b162d4311736b1f72963e0d1b861` |
| `crates/swarm-worker/src/error.rs` | 26 | `44c0eaa2441a19f5c6b7f5ca35ccc613` |
| `crates/swarm-worker/tests/worker_integration.rs` | 320 | `d75113588ff4732b1f6bd98351e1d5be` |

### Изменённые
| Файл | md5 | Что |
|---|---|---|
| `Cargo.toml` | — | добавлен `crates/swarm-llm` в members |
| `crates/swarm-planner/Cargo.toml` | — | `reqwest` → `swarm-llm` |
| `crates/swarm-planner/src/llm.rs` | `4504aed0930a9716090987892ac9ef57` | тонкий шим над `swarm-llm` (прежний API) |
| `crates/swarm-planner/src/lib.rs` | `4456eaf9f830c98812601a1402378c84` | идемпотентность через unique_violation (см. §3) |
| `crates/swarm-core/src/errors.rs` | `5e084e9615c7b872d47e6fdc6c747861` | +`is_insufficient_privilege`, +`is_unique_violation` |
| `crates/swarm-core/tests/integration_verified.rs` | `da0824fa614a81c03bbafe7a01ac0f3c` | DSN из `TEST_DATABASE_URL` (приложение ТЗ) |

Схема (`schema/schema_v0.sql`), `step2/roles.sql`, `step2b/roles_2b.sql` — **не менялись**. Существующие тесты — **не менялись**, только добавлены.

---

## 2. Критерии готовности

### K1. `cargo build --workspace --locked`
```
warn+err = 0
Finished `dev` profile [unoptimized + debuginfo] target(s)
```

### K2. `cargo test --workspace --lib`
```
swarm-core:    ok. 8 passed
swarm-llm:     ok. 2 passed
swarm-planner: ok. 14 passed
swarm-worker:  ok. 10 passed
```
**Имена unit-тестов `swarm-worker` (10):**
```
artifacts::tests::artifact_path_from_ref
artifacts::tests::atomic_write_leaves_no_tmp
artifacts::tests::existing_file_not_overwritten
artifacts::tests::sha256_matches
artifacts::tests::sha256_mismatch_fails
prompt::tests::prompt_escapes_untrusted
prompt::tests::system_prompt_mentions_untrusted
retry::tests::backoff_grows_and_caps
retry::tests::client_errors_do_not_retry
retry::tests::transient_errors_retry
```
**`swarm-llm` (2):** `tests::mock_returns_file_contents`, `tests::unknown_provider_errors`.

### K3. `cargo test -p swarm-worker --test worker_integration -- --test-threads=1`
```
test result: ok. 8 passed; 0 failed
```
**Имена (8):**
```
worker_full_cycle
worker_artifact_on_disk
worker_retry_on_timeout
worker_no_retry_on_4xx
worker_budget_exhausted
worker_prompt_injection_inert
worker_idempotent_on_retry
worker_artifact_passes_review
```
ТЗ ожидало 7. Стало 8: добавлен `worker_artifact_passes_review` (артефакт проходит review → done).

### K4. `cargo test -p swarm-planner --test planner_integration` — регрессия после переноса LLM
```
test result: ok. 8 passed; 0 failed
```

### K5–K7. pytest (регрессии)
```
tests/test_v0.py          26 passed
step2/test_roles.py       30 passed
step2b/test_roles_2b.py   19 passed
```
**Бонус:** `cargo test -p swarm-core --test integration_verified` — `8 passed` (после фикса DSN, см. §3).

### K8. Мутации M1–M5 — 5 из 5 ловятся

| # | Мутация | Поймавший тест | Результат |
|---|---|---|---|
| M1 | убрать запись файла на диск | `worker_artifact_on_disk`, `worker_idempotent_on_retry` | ✅ 2 failed |
| M2 | убрать проверку sha256 | `artifacts::tests::sha256_mismatch_fails` | ✅ 1 failed |
| M3 | убрать retry на таймаут | `worker_retry_on_timeout` | ✅ 1 failed |
| M4 | убрать распознавание blocked | `worker_budget_exhausted` | ✅ 1 failed |
| M5 | убрать `<untrusted>` маркеры | `prompt::tests::prompt_escapes_untrusted`, `worker_prompt_injection_inert` | ✅ 2 failed |

После откатов: `MUTANT`-остатков в `crates/` — **0**.

### K9. End-to-end демонстрация (реальный LLM relay.fast, реальные роли)

Пайплайн под настоящими ролями step 2b (не суперпользователем):

```
1) Planner  (роль swarm_planner):
   root_id: 10c2754b-ffa5-52d6-af58-5bb5c10c4e83
   tasks created: 1
   ready to execute: 1
   повтор → "graph already exists (root_id=…, tasks=0)"   # идемпотентно

2) Worker   (роль swarm_worker_w1, WORKER_ID=w1):
   worker registered
   start work task_id=10c2754b… epoch=1
   completed new_state=review tokens=289
   task -> review artifact_ref=sha256:224a1c99a5c4115b84a2888ba7d7148ff4ace151c216c93aed5d4d2fbe38c460

3) Файл на диске:
   /tmp/swarm_artifacts/224a1c99…c460.txt   (269 байт)
   sha256sum → 224a1c99…c460   ==   ref в БД ✅

4) Review   (роль swarm_cli → swarm_reviewer):
   new state: done
   verify_chain(...) → t
```

### K10. Docker/compose — **отложено** (нужен sudo; батч с step 1/2/2b/Planner, §5.5 KNOWN_LIMITATIONS).

---

## 3. Что нашлось и было исправлено при верификации

### 3.1. Реальный гэп в грантах: Planner под своей ролью не мог читать
Демо под настоящей ролью `swarm_planner` упало: `graph_exists` вызывает `task_status(1000)`, а по `roles.sql` грант на `task_status` есть **только у `swarm_operator`** (не у `swarm_planner`). То есть идемпотентная проверка работала лишь под суперпользователем в тестах.
**Фикс (без изменения схемы/ролей, ТЗ §0):** `graph_exists` стал best-effort — при `insufficient_privilege` (SQLSTATE 42501) продолжаем; фактическое существование графа определяется по `unique_violation` (23505) **строго на вставке корня** (перехват не задевает другие PK-нарушения, например дубль зависимости). Добавлены `DispatcherError::is_unique_violation` и `is_insufficient_privilege`.

### 3.2. `integration_verified.rs` (приложение ТЗ) — починено
Было: `const DSN = "host=localhost user=postgres password=… dbname=v0test"` → порт 5432 → `ConnectionRefused`. Стало: `fn dsn()` из `TEST_DATABASE_URL` с fallback на прежний DSN. Тест: **8 passed** на порту 55440.

### 3.3. Шим в Planner
`swarm-planner/src/llm.rs` стал тонкой обёрткой над `swarm-llm` с прежним публичным API (`LlmConfig`, `LlmClient::new`, `config()`, `complete(system,user)->Result<String>`), поэтому код и тесты Planner не пришлось трогать — регрессия зелёная.

### 3.4. Уточнены точки мутаций M2 и M4
- M2 сначала не ловился (тест дёргал `verify_artifact` напрямую, а мутация убирала вызов в `write_artifact`). Добавлена проверка целостности и на пути «файл уже существует»; тест теперь ломает оба пути.
- M4 сначала стоял на недостижимой через `claim` ветке (`spent >= cap`). Перенесён в реальную точку — распознавание `blocked` от `complete_task`.

---

## 4. Что не сработало / не проверено

1. **Docker/compose (K10)** — не проверено, нет sudo. Отложено.
2. **Демо-артефакт — не «полезный код», а `missing_input`.** Пайплайн отработал механически верно (файл, sha256, review, done), но Planner кладёт в контракт **корня** `kind:"root"` без `description` (`root_contract()` игнорирует текст задачи — `let _ = description;`). Воркер получил контракт без полезной нагрузки, и LLM честно вернул:
   ```
   {"error": "missing_input", "details": "…поле <untrusted> пустое…"}
   ```
   Это **ожидаемое поведение защиты** (воркер не выдумывает данные), но означает, что e2e-демо не показывает генерацию реального кода. **Вопрос автору:** нужно ли, чтобы Planner пробрасывал `description` (текст задачи) в контракт корня? Тогда воркер сможет реально выполнять корневую задачу. Сейчас это архитектурный пробел между Planner и Worker.
3. **`worker_artifact_passes_review`** добавлен сверх ТЗ (8 вместо 7) — ок?
4. **Тайминги демо** зависят от relay.fast (реальный вызов ~4 c, есть запас).

---

## 5. Стоп / вопросы автору

1. **`description` в контракте корня** — пробрасывать текст задачи от Planner к воркеру? Иначе e2e даёт `missing_input` (см. §4.2). Это единственное, что мешает показать реальную генерацию кода в демо.
2. **K3 = 8 тестов вместо 7** (добавлен `worker_artifact_passes_review`) — ок?
3. **Идемпотентность без `task_status`** — оставить определение через `unique_violation` (текущий фикс), или выдать `swarm_planner` грант на `task_status` отдельным шагом? Схему/роли в v0.3 не менял.
4. **Docker-батч** — присылать sudo или ждать?

---

## 6. Итог

| Критерий | Статус |
|---|---|
| 1. build --workspace --locked (0/0) | ✅ |
| 2. unit (8 core + 2 llm + 14 planner + 10 worker) | ✅ |
| 3. worker_integration | ✅ 8 passed |
| 4. planner_integration (регрессия) | ✅ 8 passed |
| 5. test_v0.py | ✅ 26 passed |
| 6. test_roles.py | ✅ 30 passed |
| 7. test_roles_2b.py | ✅ 19 passed |
| 8. Мутации M1–M5 | ✅ 5 из 5 |
| 9. e2e: Planner → Worker → файл → review → done | ✅ (с оговоркой §4.2) |
| 10. Docker | ⏸ отложено (sudo) |

**LLM-клиент вынесен в `swarm-llm`, LLM-worker реализован и верифицирован на native под реальными ролями.** До приёмки — стоп; **v0.4 (shell tools) не начинаю.**
