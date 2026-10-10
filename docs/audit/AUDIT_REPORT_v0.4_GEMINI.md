# Внешний аудит v0.4 (Gemini / spark) — доказательная база, не ground truth

> **Статус документа.** Это **внешний** отчёт второго независимого аудитора (Gemini spark).
> Он — **доказательная база**, а не истина. Каждая находка требует сверки с кодом на
> актуальной ревизии. Пересечения с отчётом Claude отмечены ниже.
>
> **Ревизия на момент аудита:** уточняется аудитором (репозиторий существовал на `1909116`
> и ранее). Проверка проводилась на HEAD `5349cf6`.

## Сверка Gemini-находок с кодом (проведена на HEAD `5349cf6`)

| ID | Severity | Компонент | Код-проверка | Вердикт |
|----|----------|-----------|--------------|---------|
| FIND-01 | CRITICAL | `swarm-worker`: демон не вызывает `agent_loop` | `agent_loop` НЕ в `main.rs/worker.rs`; `WORKER_SHELL_TOOLS` не читается; в `worker.rs:148` — `call_llm_with_retries` | **CONFIRMED** |
| FIND-02 | HIGH | `schema`: перманентный дедлок `unblock_task` | дублирует наш **F4** | **CONFIRMED (=F4)** |
| FIND-03 | HIGH | `schema`: обход review-gate по одному `type` | дублирует наш **F2** | **CONFIRMED (=F2)** |
| FIND-04 | HIGH | `roles`: `record_tool_call` без `assert_worker_identity` | 0 вхождений identity-проверки в патче; только `assert_lease` | **CONFIRMED** |
| FIND-05 | HIGH | `sandbox`: неограниченный `read_to_end` → OOM | `sandbox.rs:426,433` — `read_to_end` без лимита до `truncate_output` | **CONFIRMED** |
| FIND-06 | MEDIUM | `worker`: бессмертный зомби-аренда | дублирует наш **F5** | **CONFIRMED (=F5)** |
| FIND-07 | MEDIUM | `planner`: нет `<untrusted>` в промпте | требуется отдельная проверка `prompt.rs` | см. ниже |
| FIND-08 | MEDIUM | `tests`: ложный `ok 10 passed` без `TEST_DATABASE_URL` | `integration.rs` 10 + `integration_alt.rs` 3 = 13 вхождений `else { return }` | **CONFIRMED** |
| FIND-09 | MEDIUM | `Cargo.toml`: рассинхрон MSRV 1.85 vs 1.94.1 | `Cargo.toml:15 =1.94.1`; `README.md:27,151` и `KNOWN_LIMITATIONS.md:225` = 1.85 | **CONFIRMED** |
| FIND-10 | LOW | `docker-compose`: патч `roles_patch_submit_review` не смонтирован | в compose только `planner_status` + `tool_call_audit` | **CONFIRMED** |
| FIND-11 | LOW | `worker`: падение потока heartbeat при сетевом сбое | дублирует наш **F5** (часть) | **CONFIRMED (=F5)** |

## Что Gemini нашёл дополнительно (не было у Claude)

- **FIND-01** — мёртвый `agent_loop` в продакшен-демоне (CRITICAL по последствиям).
- **FIND-04** — `record_tool_call` без проверки идентичности воркера (подделка аудита).
- **FIND-05** — неограниченная буферизация stdout/stderr (OOM хоста).
- **FIND-08** — нечестное прохождение интеграционных тестов без БД.
- **FIND-09** — рассинхронизация MSRV в документации.

## Часть 2. Глобальное исследование отрасли (сжатое резюме)

**Вопрос:** решена ли задача `swarm-dispatcher` как единый продукт в мире?

**Ответ:** Нет, как единый законченный **открытый продукт** — отсутствует. Отдельные
компоненты решены в разных стеках:

- **Durable Execution на PostgreSQL:** River Queue / Oban / pg-boss (`FOR UPDATE SKIP LOCKED`),
  Hatchet, Restate/Temporal (Epoch Fencing против зомби). Temporal опирается на
  детерминированный Event Replay, что плохо подходит для недетерминированного инференса LLM.
  DBOS и Absurd Workflows (Armin Ronacher, 2025–2026) доказывают жизнеспособность отказа
  от внешних координаторов.
- **Агентские фреймворки:** LangGraph (чекпоинты в Postgres, `interrupt`), CrewAI/AutoGen/MetaGPT
  (диалоговые цепочки в памяти, без транзакционных FSM). Nous Research Hermes Agent
  (Kanban Worker Lanes) — ближайший концепт, но под одну локальную машину. Veritas Kanban —
  изоляция через git worktree, но без системной изоляции прав.
- **Распределённый инференс:** Petals / Parallax (swarm parallelism, чувствителен к latency),
  Exo (кластер одной модели). Решают «одну большую модель», а не оркестрацию задач.

**Ценность архитектуры проекта:** «MoE на уровне задач и узлов» (Task-level MoE with
Lease-based FSM) — обоснованная и востребованная идея. Разнородные задачи распределяются
по подходящим узлам по контракту, а не сливаются в медленный тензорный кластер.

**Рекомендации исследования:** подключить `agent_loop` (FIND-01); внедрить Outbox Pattern;
ограничить максимальную аренду (`max_lease_duration`, FIND-06); изолировать хранилище
артефактов S3/CAS (пересечение с нашим **F1**).

---

*Источник: файл `AUDIT_REPORT_V0.4_AND_GLOBAL_RESEARCH.md`, переданный владельцем 2026-10-11.
Тесты-репродукторы (6 файлов) перечислены в отчёте; авторитет — у кода на HEAD.*
