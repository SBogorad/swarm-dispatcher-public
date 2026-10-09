# Swarm Dispatcher — состояние проекта

## Что это

Reference implementation распределённой оркестрации ИИ-агентов через
канбан-подобный контроль состояний. Один источник истины — Postgres 16.
Без Redis, без внешнего durable execution, без OPA (в v0).

Цель — работающий прототип для сценариев, где важна не скорость, а
выполнимость на слабом гетерогенном железе.

## Что уже существует

| Артефакт | Статус | Что внутри |
|----------|--------|-----------|
| `schema_v0.sql` | Готов, 26 тестов проходят | DDL + 14 SQL-функций |
| `test_v0.py` | Готов | 26 тестов инвариантов на Postgres 16 |
| `TZ_executor_v0.md` | Готов | ТЗ для агента-исполнителя (Python-обёртка, сторож, воркер) |
| `ARCHITECTURE_DECISIONS.md` | Готов | Ключевые решения и обоснования |
| `AUDIT_HISTORY.md` | Готов | Что было найдено и исправлено |
| `CLAUDE_DIALOGUE.md` | Готов | Ключевые обмены с Claude (диагнозы, правки) |
| `NEXT_STEPS.md` | Готов | Что делать дальше |

## Что в `schema_v0.sql`

**Таблицы:** `tasks`, `artifacts`, `task_deps`, `workers`, `events`, `reviews`.

**Ключевые механизмы:**
- Fencing token через `tasks.lease_epoch` (монотонный счётчик, не UUID).
- Дедупликация через `UNIQUE (task_id, event_id)` — атомарно с переходом.
- Хэш-цепочка событий по задаче (`prev_hash`, `hash`) с проверкой `verify_chain`.
- Аренда через `lease_until = now() + interval` (время из БД, не из воркера).
- Recursive cascade с отсечением зомби-воркеров (`lease_epoch + 1`).
- Cycle check с защитой от гонок (advisory lock по `root_id`).

**Функции:** `add_task`, `add_dep`, `promote_ready`, `register_worker`,
`claim_task`, `heartbeat`, `save_artifact`, `complete_task`, `submit_review`,
`reclaim_expired`, `reopen_task`, `unblock_task`, `append_event`, `verify_chain`.

**Коды ошибок (SQLSTATE):** LS001, AR001, CT001, GR000–GR009, RV001–RV003,
WK001, WK002. Полная таблица — в `TZ_executor_v0.md`, раздел 3.3.

## Что НЕ в v0 (сознательно отложено)

- OPA / Policy Engine
- Approvals с подписями Ed25519 / Trust Registry
- OpenTelemetry
- Rate limiting
- Мультитенантность (`tenant_id`)
- Реальный LLM в воркере
- Веб-интерфейс
- Нагрузочные тесты

## Что дальше (ближайший шаг)

Python-обёртка `swarm/` по ТЗ в `TZ_executor_v0.md`. Состав:
- `dispatcher.py` — методы, соответствующие SQL-функциям
- `errors.py` — SQLSTATE → класс исключения
- `types.py` — TaskContract, Needs, Budget, Lease
- `watchdog.py` — цикл reclaim + promote
- `worker_stub.py` — демонстрационный воркер без LLM
- `cli.py` — seed / status / review / reopen / unblock

## История версий

Документ прошёл 14 итераций архитектурной ревизии (v1–v14) и 16 аудитов
(v1–v16). Итогом стал переход от «обзора платформ» к работающему
reference stack на Postgres.

Ключевые этапы:
1. **v1–v5** — каталог платформ, формальная модель состояний, инварианты.
2. **v6–v10** — криптографическая верификация, trust registry, replay-защита.
3. **v11–v14** — Postgres-first, отказ от Redis, fencing tokens, хэш-цепочки.
4. **Аудит Claude** — найден класс ошибок, невидимых при чтении кода без запуска.
5. **schema_v0.sql + test_v0.py** — работающий прототип, 26 тестов проходят.

## Методология

- **Арбитр — запуск, а не мнение.** Все спорные правки подтверждаются тестами
  на Postgres 16.
- **Schema-first.** Схема и SQL-функции — источник истины; Python — обёртка.
- **Один метод = одна транзакция.** Долгие операции (inference) — вне транзакции;
  аренда держится через `lease_epoch`, а не через открытую транзакцию.
- **Никаких `:latest`** в Docker-образах, версии пиннуются.
