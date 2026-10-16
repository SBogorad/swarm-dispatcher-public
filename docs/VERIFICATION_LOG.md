# Журнал независимых верификаций

Каждая запись — независимый прогон системы третьей стороной.
Цель: иметь проверяемые доказательства работоспособности.

Формат: дата, кто, что, результат, артефакты.

---

## V-001: SQL-схема (Postgres 16, compose)

**Дата:** 2026-10-07
**Кто:** автор (внутренняя верификация)
**Что:** 26 тестов `tests/test_v0.py` на Postgres 16.
**Результат:** 26 passed.
**Артефакты:** логи в `docs/history/`.

**Проверенные инварианты:**
- Fencing token: зомби-воркер получает `LS001`.
- Хэш-цепочка: `verify_chain=true` после операций.
- Cycle check: `GR001` на цикл и самозависимость.
- Recursive cascade: `reopen_task` блокирует всё поддерево.
- Self-review: `RV002` при попытке.

---

## V-002: Rust-обёртка (unit + integration)

**Дата:** 2026-10-07
**Кто:** автор (внутренняя верификация)
**Что:** `cargo test --lib` + `cargo test --test integration`.
**Результат:** 8 unit + 5 integration passed.
**Артефакты:** логи в `docs/history/`.

**Проверенные инварианты:**
- Маппинг SQLSTATE → Rust-исключение.
- `TaskContract::validate` (пустой checks отвергается).
- Сериализация с `skip_serializing_if = "Option::is_none"`.
- `claim` + fencing + `save_artifact` + `complete` end-to-end.
- `submit_review` self-review отвергается.

---

## V-003: End-to-end (native + compose)

**Дата:** 2026-10-08
**Кто:** OpenClaw / агент на ноутбуке (nouthp88)
**Платформы:**
- Native: Omarchy, Postgres 18.6 в user-space, 127.0.0.1:55440.
- Compose: Postgres 16.4-alpine, 4 контейнера.

**Что:** `cargo build`, unit, integration, native e2e, compose e2e.
**Результат:** все проверки пройдены.

**Ключевое: compose-таймлайн**

```

05:29:10  watchdog {"reclaimed":0,"promoted":1}   корень → ready
05:29:10  worker-2 CRASH_AFTER_CLAIM=1            ЗАБРАЛ и УПАЛ
05:29:30  watchdog {"reclaimed":1,"promoted":0}   ВОССТАНОВИЛ
05:29:32  worker-1 claimed epoch 3                ПОДХВАТИЛ
05:29:37  worker-1 completed → review

```

**Fencing-счётчик:** `ready(0) → claim(1) → reclaim(2) → claim(3)` —
монотонно растёт, зомби-воркер отсечён.

**Что нашла сборка (не было в прогнозе автора):**
- E0521 — `lease` escapes в поток heartbeat. Клон `Lease` перед
  `thread::spawn`.

**Новые находки в Docker:**
- `Dockerfile` `rust:1.80-slim` не работает (нужен 1.85+ из-за
  `edition2024`).
- `schema_v0.sql` с правами `600` не читается Postgres в контейнере.
- `docker compose run --rm cli seed` требует `--no-deps cli swarm-cli seed`.

**Подтверждённые ограничения v0.1:**
- `save_artifact` пишет только metadata, не файл.
- Нет rate limiting, OPA, approvals, мультитенантности.

**Артефакты:** утрачены — файлы `docs/history/REPORT_v0_e2e.md` и `docs/history/swarm_fixes.diff`
в репозиторий не попали (проверено 2026-10-17). Запись оставлена как есть: это
журнал верификаций, а не список существующих файлов.

---

## V-004: [placeholder для следующей верификации]

Сюда добавлять запись после каждой новой верификации:
- v0.2 после Planner.
- v0.3 после Worker с LLM.
- v1.0 после нагрузочного теста.

Формат тот же: дата, кто, что, результат, артефакты.

---

## V-005: v0.4 shell tools + DSML Agent Loop

**Дата:** 2026-10-09
**Кто:** OpenClaw / агент на ноутбуке (nouthp88)
**Платформы:** Omarchy, Docker Engine, compose Postgres 16.4-alpine; отдельная
тестовая БД `v0sandbox` в том же Postgres-контейнере.

**Что проверено:**
- DSML-парсер DeepSeek: native `<｜｜DSML｜｜ invoke ...>` с `args` JSON,
  именованные параметры, escaped quotes, несколько invoke, malformed input.
- Обратная совместимость с JSON tool calls.
- Agent Loop с реальным Docker sandbox.
- Audit `tool_call` через SECURITY DEFINER-функцию и `verify_chain`.
- Полный пример с реальным `deepseek-v4-flash` через relay.fast.

**Результат:**
- `cargo test -p swarm-worker --lib --locked`: **42 passed**.
- `cargo test -p swarm-worker --test worker_sandbox --locked -- --test-threads=1`:
  **10 passed, 0 failed**.
- Полный Agent Loop с реальным LLM: **5 итераций, 4 tool calls**, sandbox
  выполнил `cargo init`, запись кода, `cargo test`; итоговый artifact сохранён.
- `tool_call` в БД: **4 события**.
- `verify_chain`: **true**.
- Artifact: `sha256:74898478772be1e807cdb14cfec025568c6c34d590106124280d8c5ad593c50d`
  (524 Б, `/tmp/demo_artifacts_v04/...txt`).
- После прогона тестовые sandbox-контейнеры удалены: **0 осталось**.

**Артефакты:**
- `crates/swarm-worker/src/parse_step.rs` — DSML + JSON parser.
- `crates/swarm-worker/tests/worker_sandbox.rs` — DSML integration test.
- `crates/swarm-worker/examples/agent_loop_demo.rs` — воспроизводимое демо.
- `step2/roles_patch_tool_call_audit.sql` — audit function.

**Ограничение:** полный compose e2e для запуска shell tools внутри контейнера
воркера не выполнялся; compose worker по умолчанию имеет `WORKER_SHELL_TOOLS=0`,
а Docker socket намеренно не монтируется. Проверка sandbox выполнена с хоста,
где Docker доступен напрямую.

---

## V-006: v0.4.1 strict parsing и batch rejection

**Дата:** 2026-10-09
**Кто:** OpenClaw / агент на ноутбуке (nouthp88)

**Что проверено:**
- DSML batch прерывается на первом policy reject.
- Следующий запрос LLM получает `REJECTED:` и сообщение о том, что
  оставшиеся вызовы batch не выполнялись.
- Некорректные quoted DSML strings дают `InvalidToolCall`.
- Truncated quoted strings дают `InvalidToolCall`.
- Строгий JSON-режим не извлекает JSON из произвольной прозы.
- Relay.fast metadata-параметр `tool` рядом с JSON-параметром `args`
  поддерживается без ослабления остальных проверок.
- Исторический SQL-черновик перенесён в `docs/history/obsolete/`.

**Результат:**
- `cargo test -p swarm-worker --lib --locked`: **46 passed**.
- `cargo test -p swarm-worker --test worker_sandbox --locked -- --test-threads=1`:
  **11 passed, 0 failed**.
- `cargo build --workspace --locked`: успешно.
- Реальный `deepseek-v4-flash` через relay.fast: **4 итерации, 3 tool calls**,
  `cargo test` внутри sandbox, artifact сохранён, **3 audit-события**,
  `verify_chain=true`.
- После проверки: временная БД `v0sandbox` удалена, sandbox-контейнеров **0**.

**Артефакты:** `REPORT_v0.4.1.md`,
`docs/history/obsolete/roles_2b_patch_section3.sql`.

---

## Как добавить новую верификацию

1. Скопировать шаблон выше.
2. Заполнить.
3. Приложить логи в `docs/history/`.
4. Обновить `CHANGELOG.md` (раздел «Verified by»).

**Правила:**
- Верификация должна быть **независимой**. Автор не верифицирует сам себя.
- Верификация должна быть **воспроизводимой**. Все команды — в отчёте.
- Верификация должна быть **честной**. Если что-то не прошло — записать.
