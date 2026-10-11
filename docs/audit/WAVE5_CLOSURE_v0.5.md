# Волна 5 — закрытие (Sandbox Hardening)

**Проект:** `swarm-dispatcher`
**Ветка:** `feature/v0.5-sandbox-hardening`
**HEAD:** `05824b7`
**Дата:** 2026-10-11
**Базис:** аудит v0.4 (`docs/audit/AUDIT_REPORT_swarm_dispatcher_v0_4.md`)

---

## 1. Что закрыто

| ID | Замечание | Коммит | Итог |
|---|---|---|---|
| **F6** | Prompt injection: `</untrusted>` можно закрыть досрочно | `d507c7c` | Нейтрализация угловых скобок в `wrap_untrusted`; изоляция входа планировщика; разметка вывода инструментов песочницы |
| **F7** | Planner: контракты принимают фиктивные/абсурдные проверки | `61c64cc` | Реестр допустимых типов проверок, `MAX_CHECKS_PER_TASK=32`, clamping attempts/max_rework/min_ram_gb/coder_class |
| **F11** | Allowlist: config-опции `git`/`cargo` обходят deny-список | `8344f51` | `DENIED_GIT_OPTS`/`DENIED_CARGO_OPTS` (`-c`, `--config`, `--exec-path`, `--upload-pack`, `-Z`, `--rustc`, …) |
| **F12** | Sandbox: неполные лимиты ресурсов и таймаут | `17e3986` | `--pids-limit` (fork-бомбы), `timeout -k 5 -s TERM` (KILL внутри контейнера), изоляция префикса по `worker_id` |
| **gate** | feature-gate `shell-tools` | `05824b7` | **Снят**: все блокеры закрыты, shell-tools компилируются всегда |

## 2. Верификация (живое окружение)

| Проверка | Команда | Результат |
|---|---|---|
| Компиляция без feature | `cargo check -p swarm-worker` | OK |
| Lib-тесты | `cargo test -p swarm-worker --lib` | **66 passed** |
| Planner | `cargo test -p swarm-planner --lib` | **24 passed** |
| Sandbox e2e | `cargo test -p swarm-worker --test worker_sandbox -- --test-threads=1` | **11 passed** |
| Python-арбитры | `tests/test_v0.py`, `step2b` | зелёные |

Окружение: Docker 29.7.2, образ `agent-sandbox:1.85`, PostgreSQL 16 (контейнер), rustc 1.99.0.

## 3. Триадные доказательства (RED → GREEN → RED after revert)

- `evidence/wave5_F6_prompt_injection_triad.txt`
- `evidence/wave5_F7_planner_contracts_triad.txt`
- `evidence/wave5_F11_vcs_allowlist_triad.txt`
- `evidence/wave5_F12_sandbox_limits_triad.txt`
- `evidence/wave5_shell_tools_gate_removed.txt`

## 4. Остаточный риск

- План `shell-tools` включён в бинарь воркера. Все четыре блокера закрыты и покрыты
  автотестами; ручная приёмка на живом Docker пройдена.
- Известные ограничения вне рамок волны: F13 (мутационные тесты allowlist), F14
  (покрытие воркера), F16 (документация), F17 (идемпотентность планировщика), F18
  (тело ответа в `LlmError::Http`).
