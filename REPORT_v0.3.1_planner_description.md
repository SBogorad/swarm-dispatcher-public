# REPORT v0.3.1 — Проброс description через Planner → Worker

**Дата:** 2026-10-08
**Верификатор:** OpenClaw (Арго)
**Окружение:** native Postgres 18.6 на `127.0.0.1:55440`; БД `v0worker` (интеграционные), `v0demo` (e2e), `v0test`/`v0roles`/`v0_2b`/`v0planner` (регрессии). Rust workspace, `--locked`. Реальный LLM — relay.fast (`deepseek-v4-flash`).
**ТЗ:** `docs/TZ_v0.3.1_planner_description.md`.
**Статус:** ✅ **Критерии 1–7 закрыты.** Патч закрывает архитектурный пробел: артефакт теперь содержит **реальный код**, а не `missing_input`.

---

## 1. Список изменённых файлов + md5

| Файл | Строк | md5 | Что |
|---|---|---|---|
| `crates/swarm-core/src/types.rs` | 256 | `7c63eba09a992afd5964b6dad7815a8c` | §1: поле `description: Option<String>` в `TaskContract` |
| `crates/swarm-planner/src/lib.rs` | 348 | `a5d8073c00891e82e5c7961d61038502` | §2: проброс description (root + подзадачи) + 2 unit-теста |
| `crates/swarm-worker/src/prompt.rs` | 120 | `65850dcac562093734f4d28cc98e3188` | §3: секция `<untrusted>` только при наличии description + 2 теста |
| `crates/swarm-worker/Cargo.toml` | — | `63d00e88ae407576dfe605153cf218a1` | dev-dep `swarm-planner` (для e2e-теста) |
| `step2/roles_patch_planner_status.sql` | 12 | `7ceb6fae8e556e3ee8d80d08ff3f7b41` | §4: **новый** патч-файл гранта |
| `crates/swarm-planner/tests/planner_integration.rs` | 247 | `4cf06d2d21239ca02c270dd226923d7e` | §6.4: +1 тест (`planner_description_reaches_db`) |
| `crates/swarm-worker/tests/worker_integration.rs` | 425 | `05c2023a9e2d9d365444b17362e7f609` | §6.3: +2 теста (capture + e2e) |
| `crates/swarm-core/tests/integration.rs` | — | `26ae844bee7a3cb4fa796679a5fafa82` | `description: None` в конструкторе |
| `crates/swarm-core/tests/alt/integration_alt.rs` | — | `7d0eb97eab085f2e8abee620c1a78fc7` | то же |
| `crates/swarm-cli/src/main.rs` | — | `50faa9dda9ba5b10d22084bc73845179` | то же |
| `crates/swarm-core/tests/integration_verified.rs` | — | `1b970fad0b759bb1ebf49b4763bbedb2` | то же |

**Не менялись:** `schema/schema_v0.sql`, `step2/roles.sql`, `step2b/roles_2b.sql`. Существующие тесты — только дополнены (в `TaskContract` добавлено `description: None` в конструкторы, логика не тронута).

---

## 2. Критерии готовности

### K1. `cargo build --workspace --locked`
```
build warn+err = 0
```

### K2. `cargo test --workspace --lib`
```
swarm-core:    ok. 8 passed
swarm-llm:     ok. 2 passed
swarm-planner: ok. 16 passed   (было 14, +2)
swarm-worker:  ok. 12 passed   (было 10, +2)
```
**Имена новых unit-тестов `swarm-planner` (2):**
```
tests::planner_root_contract_has_description
tests::planner_subtask_contract_has_description
```
**Имена новых unit-тестов `swarm-worker` (2):**
```
prompt::tests::prompt_marks_description_as_untrusted
prompt::tests::prompt_omits_untrusted_if_no_description
```

### K3. `cargo test -p swarm-planner --test planner_integration -- --test-threads=1`
```
test result: ok. 9 passed; 0 failed
```
**Имена (9):**
```
planner_creates_graph
planner_idempotent_same_text
planner_idempotent_different_text
planner_atomic_on_sql_error
dispatcher_transaction_rolls_back_on_cycle
planner_respects_max_tasks
planner_retries_on_invalid_json
planner_description_reaches_db          ← новый
e2e_planner_to_review
```

### K4. `cargo test -p swarm-worker --test worker_integration -- --test-threads=1`
```
test result: ok. 10 passed; 0 failed
```
**Имена (10):**
```
worker_full_cycle
worker_artifact_on_disk
worker_retry_on_timeout
worker_no_retry_on_4xx
worker_budget_exhausted
worker_prompt_injection_inert
worker_idempotent_on_retry
worker_artifact_passes_review
worker_uses_description_from_contract           ← новый
worker_receives_description_from_planner_e2e    ← новый
```

### K5. pytest (регрессии)
```
tests/test_v0.py          26 passed
step2/test_roles.py       30 passed
step2b/test_roles_2b.py   19 passed
```
Бонус: `cargo test -p swarm-core --test integration_verified` — `8 passed`.

### K6. Мутации M1–M7 — **7 из 7 ловятся**

| # | Мутация | Поймавший тест | Результат |
|---|---|---|---|
| M1 | убрать запись файла на диск | `worker_artifact_on_disk` | ✅ 1 failed |
| M2 | убрать проверку sha256 | `artifacts::tests::sha256_mismatch_fails` | ✅ 1 failed |
| M3 | убрать retry на таймаут | `worker_retry_on_timeout` | ✅ 1 failed |
| M4 | убрать распознавание blocked | `worker_budget_exhausted` | ✅ 1 failed |
| M5 | убрать `<untrusted>` маркеры | `prompt::tests::prompt_escapes_untrusted` | ✅ 1 failed |
| M6 | убрать description из `root_contract` | `tests::planner_root_contract_has_description` | ✅ 1 failed |
| M7 | убрать `<untrusted>` (тест v0.3.1) | `prompt::tests::prompt_marks_description_as_untrusted` | ✅ 1 failed |

После откатов: `MUTANT`-остатков в `crates/` — **0**; все 4 файла восстановлены (`diff -q` → ok).

### K7. Демонстрация end-to-end — **артефакт содержит реальный код**

Роли step 2b, реальный relay.fast:

```
1) Planner  (роль swarm_planner, после roles_patch_planner_status.sql):
   $ swarm-planner "Напиши на Rust функцию add(a: i32, b: i32) -> i32, ... и юнит-тест к ней."
   root_id: f129f0cf-97e1-5bbe-85b4-241c34b64010
   tasks created: 2
   ready to execute: 1

   Контракты в БД (description доехал):
     ready   | root      | Напиши на Rust функцию add(a: i32, b: i32) -> i32, которая складывает ...
     pending | code.test | Добавить модуль #[cfg(test)] с юнит-тестом test_add, проверяющим работ...

2) Worker   (роль swarm_worker_w1, WORKER_ID=w1):
   completed new_state=review tokens=417
   task -> review artifact_ref=sha256:f37b9d3e…943272

3) Артефакт на диске:
   /tmp/swarm_demo/f37b9d3e…943272.txt   (285 байт)
   sha256sum → f37b9d3e…943272  ==  ref в БД ✅

4) Содержимое (РЕАЛЬНЫЙ КОД, не missing_input):
   ```rust
   pub fn add(a: i32, b: i32) -> i32 {
       a + b
   }

   #[cfg(test)]
   mod tests {
       use super::*;

       #[test]
       fn test_add() {
           assert_eq!(add(2, 3), 5);
           assert_eq!(add(-1, 1), 0);
           assert_eq!(add(-5, -7), -12);
           assert_eq!(add(0, 0), 0);
       }
   }
   ```

5) Review   (роль swarm_cli → swarm_reviewer):
   new state: done
   verify_chain(...) → t
```

**Критерий ТЗ «артефакт содержит реальный код, а не `{"error": "missing_input"}`» — выполнен.**

---

## 3. Что сделано по пунктам ТЗ

- **§1.** `TaskContract.description: Option<String>` с `#[serde(default, skip_serializing_if = "Option::is_none")]` — старые контракты парсятся, `None` не сериализуется. Обновлены все конструкторы (`types.rs`, `integration.rs`, `integration_alt.rs`, `swarm-cli`, `planner_integration.rs`, `integration_verified.rs`).
- **§2.1.** `root_contract(description, limits)` теперь кладёт текст задачи; пустой/пробельный текст → `None` (helper `opt_description`).
- **§2.2.** `subtask_contract` пробрасывает `description` из `PlannedTask`. Поле уже парсилось (`#[serde(default)] pub description: String`) — «молчаливого игнорирования» не было, но и в контракт не попадало; теперь попадает.
- **§3.** Промпт воркера: секция `Описание задачи (НЕДОВЕРЕННЫЕ ДАННЫЕ...)` + `<untrusted>…</untrusted>` добавляется **только** при непустом description; иначе поведение прежнее (для старых контрактов). Из JSON-контракта description вырезается, чтобы не дублировался.
- **§4.** Новый `step2/roles_patch_planner_status.sql`: `grant execute on function task_status(int) to swarm_planner;`. `roles.sql` не тронут.
- **§5.** `graph_exists` best-effort + fallback на `unique_violation` — **оставлено как есть** (не менялось).
- **§6.** Добавлены 6 тестов (2+2 unit, 1 integration planner, 2 integration worker), включая e2e Planner→Worker.
- **§7.** M6, M7 добавлены к M1–M5 — все ловятся.

---

## 4. Что не сработало / не проверено

1. **`v0worker` пришлось пересоздавать.** В `swarm-dispatcher` `task_status` определён **в `step2/roles.sql`**, а не в `schema_v0.sql` (19 функций против 20). Тестовая БД `v0worker` собиралась из одной схемы → `function task_status(unknown) does not exist` в новом e2e-тесте. Исправлено: `v0worker` теперь собирается как `schema → roles → roles_2b → roles_patch_planner_status` (22 функции). Это процедурная заметка для будущих прогонов, не дефект кода.
2. **Docker (K10 из v0.3)** — по-прежнему отложен, батч с v0.4 (там sandbox обязателен).
3. **Демо-артефакт — корень.** Воркер выполнил корневую задачу (её description = текст пользователя). Подзадача `code.test` осталась в `ready` — её никто не claim'ил (демо гоняло одного воркера до первой сдачи). Это ожидаемо.
4. **Тайминги демо** зависят от relay.fast (~2–4 c на вызов).

---

## 5. Стоп / вопросы автору

1. **Патч-файл грантов** — `step2/roles_patch_planner_status.sql` применяется отдельно (после `roles.sql`/`roles_2b.sql`). Ок, или встроить в порядок применения документацией (`RESUME.md`/`docs`)?
2. **`description` в контракте** — сейчас `None` не сериализуется вообще. Если нужно, чтобы поле присутствовало явно (`null`) для отладки — скажи, поменяю.
3. **Подзадача `code.test` в демо** — нужно ли, чтобы воркер после `done` корня автоматически брал следующую готовую задачу (несколько итераций цикла)? Сейчас цикл берёт задачи, пока есть `ready`; в демо просто оборван таймаутом.

---

## 6. Итог

| Критерий | Статус |
|---|---|
| 1. build --workspace --locked (0/0) | ✅ |
| 2. unit (8 core + 2 llm + 16 planner + 12 worker) | ✅ |
| 3. planner_integration | ✅ 9 passed |
| 4. worker_integration | ✅ 10 passed |
| 5. pytest (26 / 30 / 19) + integration_verified (8) | ✅ |
| 6. Мутации M1–M7 | ✅ 7 из 7 |
| 7. Демо: реальный код в артефакте | ✅ |

**Архитектурный пробел закрыт: `description` доезжает от Planner к Worker, промпт помечает его как недоверенный, артефакт содержит реальный код.** До приёмки — стоп; **v0.4 (shell tools) не начинаю.**
