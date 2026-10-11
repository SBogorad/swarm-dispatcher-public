# ПРОТОКОЛ АУДИТА И ВЕРИФИКАЦИИ
## Комплексная проверка надежности и безопасности архитектуры Swarm Dispatcher v0.4

**Дата составления протокола:** 11 октября 2026 г.
**Объект аудита:** Репозиторий `swarm-dispatcher`
**Базовая ревизия (исходное состояние):** `HEAD 1909116` / `master db5ffe3`
**Итоговая ревизия (верифицированное состояние):**
`HEAD 188e63604adf9782e1d451de57e9d2e24e4ea10e`
(ветка `feature/v0.6-distributed-worker-mvp`, +65 коммитов к master)
**Статус протокола:** все замечания закрыты, триады сошлись, регресс пройден.

> **Прим. (актуализация SHA):** исходно указан `7eb8617`; после добавления
> re-audit-пакета финальный HEAD — `188e636`. Коммит не меняет `.rs/.sql`
> (только `docs/audit/*` и builder-скрипт), выводы в силе.

---

## 1. Методология и границы тестирования (Test Boundaries & Transparency)

### 1.1. Что запускалось непосредственно в песочнице аналитика:
1. **Статический и семантический анализ кода:**
   - Полный аудит SQL-схемы (`schema/schema_v0.sql`, `step2/roles.sql`,
     `step2b/roles_2b.sql`, патчи).
   - Анализ AST и графа вызовов Rust-кода (`crates/swarm-worker`,
     `crates/swarm-core`, `crates/swarm-planner`).
2. **Изолированные поведенческие скрипты воспроизведения (Python):**
   - `test_worker_daemon_missing_agent_loop.py` — **запущен локально (PASS)**:
     в v0.4 демон воркера не импортировал и не вызывал `agent_loop::run`.
   - `test_sandbox_unbounded_output_oom.py` — **запущен локально (PASS)**:
     воспроизведено выделение 50 МБ памяти процесса до применения усечения.
   - `test_integration_dry_run_honesty.py` — **запущен локально (PASS)**:
     выявлен паттерн `let Some(dsn) = dsn() else { return };`.
3. **Что НЕ запускалось в песочнице аналитика:**
   - Компиляция полного воркспейса через `cargo` (нет Rust 1.94+ и PG).

### 1.2. Что запускалось на интеграционном стенде (OpenClaw / Linux / Docker):
1. **PostgreSQL 16.15 (Alpine):**
   - Все SQL-процедуры, роли, права, политики.
   - Тесты F2, F3, F4, F15, FIND-04 под реальными ролями
     `swarm_worker_w1`, `swarm_worker_w2`, `swarm_reviewer`, `swarm_operator`.
2. **Компиляция и прогон Rust-тестов:**
   - `cargo test --workspace`.
   - Бинарный сканирующий тест на отсутствие символов `shell-tools` в бинарнике
     воркера без фичи (`wave4_FIND01_symbol_ban.txt`).
3. **Межконтейнерное взаимодействие (Docker Compose):**
   - Видимость артефактов через том `swarm-artifacts` между `worker-1` и `cli`.

---

## 2. Сводный реестр дефектов и протокол их закрытия

Триадный критерий: **RED на pre-fix → GREEN на исправленном → RED при откате.**

| ID аудита | Внешний ID | Категория | Severity | Коммит | Затронутые файлы | Триада | Результат |
|---|---|---|---|---|---|---|---|
| **FIND-04** | SD-05 | Security / Audit | **HIGH** | `566a4fb` | `step2/roles_patch_tool_call_audit.sql` | RED(1F/2P)→GREEN(3P)→RED(1F/2P) | `assert_worker_identity(p_worker)`; подделка чужих записей — `WK003`. |
| **F2** | SD-02 | Integrity / Gate | **HIGH** | `33e8ef9` | `schema/schema_v0.sql` | RED(3F/2P)→GREEN(5P)→RED(3F/2P) | Ключ `coalesce(id, type\|\|'\|'\|\|run)`; `count(pass) >= req.n`. |
| **F15** | SD-04 | Contract Form | **MEDIUM** | `33e8ef9` | `schema/schema_v0.sql` | RED(7F/3P)→GREEN(10P)→RED(7F/3P) | `validate_contract` строго проверяет тип массива и непустой `type`. |
| **F16г** | — | Architecture | **MEDIUM** | `33e8ef9` | `step2/roles_patch_submit_review.sql` | initdb | Мёртвый патч удалён (нет перекрытия схемы). |
| **F3** | SD-01 | Auth / Roles | **HIGH** | `418ae18` | `step2b/roles_2b.sql`, `step2/roles.sql` | RED(1F/4P)→GREEN(5P)→RED(1F/4P) | Deny-by-default; `swarm_worker` nologin. |
| **F4** | SD-03 | Accounting / FSM | **HIGH** | `3909166` | `schema_v0.sql`, `dispatcher.rs`, `worker.rs` | RED(7F/1P)→GREEN(8P)→RED(7F/1P) | Честный `fail_task`; `p_tokens >= 0`. |
| **F5** | — | Concurrency / Lease | **HIGH** | `21e11f7` | `crates/swarm-worker/src/worker.rs` | RED(2F/1P)→GREEN(3P)→RED(2F/1P) | `LeaseHealth` + барьер старта; fail-fast. |
| **F1** | — | Persistence | **CRITICAL** | `3ce2825` | `docker-compose.yml`, `Dockerfile`, `worker.rs` | RED(tmpfs)→GREEN(volume)→RED(tmpfs) | Общий volume `swarm-artifacts` (UID 1001); `volume://…`; runtime `debian:trixie-slim`. |
| **FIND-01** | — | Architecture / Sandbox | **CRITICAL** | `855daca` | `Cargo.toml`, `lib.rs`, `error.rs`, harness | RED(символы)→GREEN(0 маркеров) | `shell-tools` за `#[cfg(feature)]`. |
| **FIND-05** | — | Stability / DoS | **HIGH** | `80305d2` | `crates/swarm-worker/src/sandbox.rs` | bounded read | `p.take(max+64 KiB)` + дренаж. |
| **FIND-08** | — | Test Integrity | **MEDIUM** | `80305d2` | `tests/integration.rs`, `integration_alt.rs` | RED(panic)→GREEN(DSN) | `require_dsn()` вместо `else { return }`. |
| **FIND-09** | — | Docs Sync | **LOW** | `7eb8617` | `README.md`, `KNOWN_LIMITATIONS.md` | Проверено | MSRV `1.94.1+`. |
| **FIND-10** | — | Infra Sync | **LOW** | `7eb8617` | `docker-compose.yml`, harness | Проверено | Наборы патчей согласованы. |

---

## 3. Детальные протоколы испытаний

### 3.1. Ролевая модель и аутентичность (F3, FIND-04)
- **Стенд:** PostgreSQL 16.15, БД `w4_f3fix_*`.
- **F3:** вызов `claim_task`/`register_worker` под общей ролью `swarm_worker`
  блокируется `WK003`; логин под `swarm_worker` отклоняется сервером.
- **FIND-04:** `swarm_worker_w1` вызывает `record_tool_call` на задаче,
  арендованной `w2`, с `p_worker='w2'` → `WK003`, событие не пишется;
  легитимный вызов `w1` на своей задаче — успех.

### 3.2. Целостность приёмки и контрактов (F2, F15)
- Контракт: 2 проверки `cmd` (`cargo test`, `cargo clippy`).
- Один `pass` → `RV003: checks_failed`; два `pass` на одну команду → `RV003`;
  `pass` на обе → `done`.
- F15: `[42]`, `["cmd"]`, `[{"run":"echo"}]` без типа, пустой тип → `CT001`.

### 3.3. Учёт токенов и восстановление (F4)
- `fail_task(..., 'llm_timeout')` → `blocked`,
  `block_reason='llm_error: llm_timeout'`, `spent_tokens` честный.
- `unblock_task` без капа при исчерпанном бюджете → `CT001: budget_exhausted`.
- `unblock_task(..., 20000)` → `ready` с новым капом, задача захватывается.

### 3.4. Сохранность артефактов (F1)
- `worker-1` сохраняет `volume://swarm-artifacts/sha256_...`; после
  `docker compose restart worker-1` контейнер `cli` читает файл из общего тома,
  sha256 совпадает.

### 3.5. Безопасность песочницы (FIND-01, FIND-05)
- `strings` на бинарнике воркера (default build): 0 вхождений маркеров
  (`AllowlistRejected`, `agent_system_prompt`, `run_shell`, `WORKER_SHELL_TOOLS`).
  С `--features shell-tools` символы присутствуют.
- Тест ~10 MiB вывода при лимите 64 KiB: буфер ≤ 128 KiB, без OOM.

---

## 4. Заключение

1. Архитектурный долг и уязвимости v0.3–v0.4 на ревизии `188e636`
   ликвидированы.
2. Первичные логи триад — в `evidence/`; контрольные суммы — `MANIFEST.sha256`.
3. Рекомендация: ветка `feature/v0.6-distributed-worker-mvp` на `188e636`
   аттестована; следующая фаза — Волна 5 (харденинг песочницы и управляемое
   включение shell-tools).
