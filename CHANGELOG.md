# Changelog

Все значимые изменения проекта фиксируются в этом файле.
Формат основан на [Keep a Changelog](https://keepachangelog.com/ru/1.1.0/),
версии следуют [Semantic Versioning](https://semver.org/lang/ru/).

---

## [Unreleased]

Планируется v0.6: Distributed Worker MVP для batch-задач на разных машинах
или виртуальных машинах. Критерии и ограничения описаны в
`docs/DISTRIBUTED_WORKER_MVP.md`.

Release-tag требует acceptance harness с `--full-workspace-test`, строгим
`release_gate: STRICT`, нулевым `blocked_count` и валидным attestation.

---

## [0.5.2] — 2026-10-11 (в работе)

Hardening по итогам трёх внешних аудитов v0.5.1 и прогонов штатного harness.

### Security

- **Строгий whitelist подкоманд git/cargo** (`f474e97`). F11 проверял только
  ОПЦИИ, поэтому `git config` (подкоманда) проходил насквозь — а
  `git config core.pager "sh -c ..."` / `git config alias.x '!...'` дают RCE.
  Переход от blacklist опций к deny-by-default whitelist подкоманд
  (`ALLOWED_GIT_SUBCMDS`/`ALLOWED_CARGO_SUBCMDS`); `cargo xtask` тоже закрыт.
- **Запрет записи cargo-конфигурации** (`bf191f4`). `sandbox::write_file`
  отклоняет пути с компонентом `.cargo` и файлы `config.toml`/`config`:
  иначе агент подменял `[build] rustc = "..."` и обходил allowlist (вектор
  идёт мимо argv).

### Changed (ломающее изменение контракта)

- **`submit_review`: каждая проверка обязана возвращаться с `id`.** Если у
  проверки в контракте был `id`, рецензент ДОЛЖЕН вернуть тот же `id`; если
  `id` не было — совпасть должны `type` и `run`. Ранее фикстуры без `run`
  проходили гейт; после ужесточения это привело к регрессии фикстур
  `tests/test_v0.py` (закрыто `b9ea4d0`, Волна 4). Клиентам рецензента,
  возвращающим checks без `id` и без `run`, требуется обновление.

### Added

- Тесты слоёв allowlist: L1/L7/L8/L9/L10/L11/L12 (мутационно проверены,
  выживших нет) + поведенческий арбитр whitelist подкоманд (`56c317b`).
- Артефакт `harness-*.tests.txt` и поле `test_names` в attestation —
  конкретные имена тестов, а не только названия шагов (`56da946`).

### Fixed

- **FIND-01 в harness был вакуумным** (`bf191f4`). Негативная проверка искала
  `swarm_worker::sandbox` в форме `::`, но rustc мангалит как
  `swarm_worker7sandbox` → grep всегда давал 0, шаг PASS-ил при реальной
  утечке. Теперь: положительный контроль (строка runtime-гейта обязана быть
  в бинаре) + негативный в реальной legacy-форме мангалинга.

---

## [0.5.1] — 2026-10-11

Hardening по итогам аудита v0.5.0 (три независимых ревьюера + штатный harness).

### Security

- **Runtime-гейт `WORKER_SHELL_TOOLS=1`** (`1051d6a`). Компиляционный бан
  shell-tools снят, но включение стало осознанным: без флага воркер
  отказывает в запуске sandbox ДО обращения к Docker.
- **N3:** `name_prefix` sandbox обязан содержать worker-идентификатор;
  равенство дефолту отклоняется — иначе `cleanup_stale` одного воркера мог
  задеть контейнеры соседа.
- **N1:** `git -C <dir>` больше не ложно-блокируется (короткие флаги
  сравниваются с учётом регистра: `-C` ≠ `-c`).

### Fixed

- **Регрессия F7: тип проверки `unit`** (`92f27a0`). F7 сузил реестр до
  `('cmd','test','lint','manual')`, но `unit` — легитимный тип базового
  контракта `step2/test_review_gate.py`; `add_task` падал с `CT001`. `unit`
  возвращён и в SQL-реестр, и в Rust `ALLOWED_CHECK_TYPES` (N5).
- **conftest блокировал базовые сьюты** (`ee8e007`). Глобальный
  `W4_*`-гейт в `pytest_configure` валил сбор любого `pytest tests/test_v0.py`.
  Гейт сделан селективным (только волна-4-арбитры), честность сохранена
  (громкий BLOCKED-warning, не тихий skip).
- **N2:** `worker_sandbox` требует живых Docker+PG — помечен `#[ignore]`.

---

## [0.5.0] — 2026-10-11

Sandbox hardening, Волна 5 (F6/F7/F11/F12) + снятие compile-gate shell-tools.

- F6: `wrap_untrusted` экранирует `</`/`>` в untrusted-тексте задач.
- F7: реестр допустимых типов проверок контракта (закрыт).
- F11: запрет конфигурационных опций git/cargo (`-c`, `--config`, `-Z`, ...).
- F12: `--pids-limit` в sandbox; префикс контейнера с worker-идентификатором.
- Модули `agent_loop`/`sandbox`/`allowlist`/`audit` компилируются всегда;
  feature `shell-tools` оставлен пустым для обратной совместимости.

---

## [0.4.1] — 2026-10-09

- Agent Loop прерывает DSML batch при первом policy reject; причина отказа
  передаётся LLM для исправления отдельным следующим вызовом.
- DSML/JSON parsing стал строгим: malformed quoted strings, произвольный текст
  вместо JSON и неоднозначные аргументы возвращают `InvalidToolCall`.
- Prompt Agent Loop явно требует native DSML для tool calls и строгий JSON
  `done=true` для финального ответа.
- Перенесён неиспользуемый `step2b/roles_2b_patch_section3.sql` в
  `docs/history/obsolete/`.
- Проверки: 46 unit + 11 Docker integration; реальный DeepSeek Agent Loop —
  4 итерации / 3 tool calls, `cargo test` в sandbox, `verify_chain=true`.

---

## [0.4.0] — 2026-10-08

Shell tools + Docker sandbox (ТЗ `docs/TZ_v0.4_shell_tools_sandbox.md`).

### Added

- **Agent Loop** (`crates/swarm-worker/src/agent_loop.rs`): LLM в цикле
  возвращает tool call (`run_shell`, `read_file`, `write_file`) или финальный
  ответ; лимит итераций `WORKER_MAX_ITERATIONS` (default 20).
- **Allowlist команд** (`allowlist.rs`): deny-by-default, env
  `WORKER_ALLOWED_COMMANDS`, явный deny-список, запрет shell-операторов
  (`>`, `|`, `;`, `&&`, `` ` ``, `$(`), запрет `..`, рекурсивная проверка
  `bash -c "..."` (иначе первое слово `bash` обходило allowlist).
- **Docker sandbox** (`sandbox.rs`): один контейнер на задачу,
  `--cap-drop ALL`, `--read-only`, `--security-opt no-new-privileges:true`,
  `--network none`, tmpfs `/tmp` и `/workspace`, лимиты памяти/CPU, uid 1001.
  Таймаут команды (`WORKER_CMD_TIMEOUT_SEC`, WK005) и лимит вывода 10 MB.
- **Аудит tool calls** (`audit.rs` + `step2/roles_patch_tool_call_audit.sql`):
  событие `kind='tool_call'` в `events` (только hash + размер вывода).
  Пишется через SECURITY DEFINER-функцию `record_tool_call`, потому что
  `append_event` намеренно не выдан никому (журнал нельзя подделать снаружи).
- **Образ песочницы** (`sandbox/Dockerfile`): ubuntu 24.04 + Rust 1.85,
  непривилегированный пользователь 1001.
- **Тесты:** +23 unit (`swarm-worker` 12 → 35), новый
  `crates/swarm-worker/tests/worker_sandbox.rs` (9 integration, реальный Docker).
- **Мутации** M1–M6: `scripts/run_mutations_v04.sh` — 6 из 6 ловятся.

### Changed

- Порядок применения SQL: добавлен пятый файл
  `step2/roles_patch_tool_call_audit.sql` (README, NATIVE_SETUP, compose).
- `swarm-worker`: `postgres` и `uuid` переведены из dev- в обычные зависимости
  (аудит пишется отдельным соединением).

### Removed

- `schema/roles.sql` — заглушка, никем не применялась →
  `docs/history/obsolete/schema_roles.sql.stub`.
- `step2b/test_roles_2b.DELIVERED_partial.py` — обрезанный остаток доставки →
  `docs/history/obsolete/`.

### Security

- `WORKER_SHELL_TOOLS=0` по умолчанию: включение требует Docker-сокета в
  контейнере воркера (= root на хосте). См. `docs/KNOWN_LIMITATIONS.md` §5.6.

---

## [0.1.0] — 2026-10-08

Первый публичный тег. Работающий фундамент: схема БД, Rust-обёртка,
watchdog, worker-заглушка, CLI, Docker Compose.

### Added

- **Схема Postgres** (`schema/schema_v0.sql`): 6 таблиц, 19 функций,
  коды ошибок LS001…WK002.
  - `tasks`, `artifacts`, `task_deps`, `workers`, `events`, `reviews`.
  - Guarded FSM с 6 состояниями.
  - Fencing token через `lease_epoch`.
  - Хэш-цепочка событий (`prev_hash`, `hash`, `verify_chain`).
  - Recursive cascade в `reopen_task`.
  - Cycle check в `add_dep`.
- **Rust-workspace** (`crates/`):
  - `swarm-core` — библиотека: `SwarmDispatcher`, ошибки, типы.
  - `swarm-watchdog` — цикл `reclaim_expired` + `promote_ready`.
  - `swarm-worker` — воркер-заглушка (без LLM).
  - `swarm-cli` — `seed`, `status`, `review`, `reopen`, `unblock`.
- **Тесты**:
  - 26 SQL-тестов на Postgres 16 (`tests/test_v0.py`).
  - 8 unit-тестов Rust.
  - 5 integration-тестов Rust на живом Postgres.
- **Docker Compose**: postgres + watchdog + 2 воркера + cli.
- **Native-развёртывание**: без Docker и без root (Postgres 18.6 в
  user-space).
- **Документация** (`docs/`):
  - `PROJECT_STATE.md`, `METHODOLOGY.md`, `ARCHITECTURE.md`.
  - `ARCHITECTURE_DECISIONS.md` — 11 ADR.
  - `PLATFORM_COMPARISON.md`, `THREAT_MODEL.md`.
  - `BRIEF_FOR_AGENTS.md`, `PLAIN_EXPLANATION.md`.
  - `ROADMAP.md`, `NEXT_STEPS.md`, `DOC_STRATEGY.md`.
  - `TZ_v0.2_step1_fixes.md` — ТЗ на текущий шаг.

### Fixed (при независимой верификации v0.1)

- `errors.rs`: `Database(#[source] postgres::Error)` →
  `Database(#[from] postgres::Error)`. Без этого `?` не компилировался.
- `worker/main.rs`: E0521 — клон `Lease` перед `thread::spawn`.
  Ссылка не может быть перемещена в поток (`'static`).
- `errors.rs` (тест): `sqlstate_known_codes_map` проверял текст сообщения
  вместо варианта ошибки. Переписан на `matches!(err, DispatcherError::*())`.
- `watchdog/main.rs`: убран неиспользуемый `use tracing::warn`.

### Changed

- `Dockerfile`: `rust:1.80-slim` → `rust:1.98-slim`. Транзитивная
  `block-buffer 0.12` требует `edition2024` (Rust ≥ 1.85).
- `Cargo.toml`: `rust-version = "1.75"` → `"1.85"`. Декларативно неверный
  MSRV исправлен; фактический минимум определяется транзитивными
  зависимостями.
- `docker-compose.yml`: удалён deprecated `version: "3.9"`;
  CLI-сервис без `command` (запуск через `swarm-cli <cmd>`).
- `schema/schema_v0.sql`: права `644` (было `600`) — Postgres в контейнере
  не мог прочитать init-скрипт. Правится через `COPY --chmod=644` в
  Dockerfile.

### Known Limitations

См. `docs/KNOWN_LIMITATIONS.md`. Главное:

- `save_artifact` пишет только metadata в БД, файл на диск не пишется.
- Worker — заглушка (`sleep`), реального LLM нет.
- Planner (LLM-декомпозиция) отсутствует.
- Веб-доска отсутствует (только `cli status`).
- OPA, approvals, мультитенантность — v2.

### Verified by

Независимая верификация OpenClaw на Omarchy + syntog (2026-10-08).
Подробности — `docs/VERIFICATION_LOG.md`.

- `cargo build`: 0 errors, 0 warnings.
- Unit-тесты: 8 passed.
- Integration-тесты: 5 passed.
- Native e2e: OK (Postgres 18.6, user-space).
- Docker Compose e2e: OK (4 контейнера).
- Fencing + reclaim: подтверждено (`{"reclaimed":1}` в логе watchdog).
- `verify_chain=true` по всем задачам.
