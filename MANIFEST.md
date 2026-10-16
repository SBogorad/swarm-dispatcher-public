# Карта репозитория

Полный список файлов с описанием: что внутри, зачем, когда читать.

**Актуально на:** v0.2 (в разработке)

---

## Как пользоваться

**Если вы новый участник:**
1. `README.md` — что это и как запустить.
2. `docs/PROJECT_STATE.md` — где мы сейчас.
3. `docs/METHODOLOGY.md` — как принимаются решения.
4. `docs/optional/NEXT_STEPS.md` — что делать дальше.
5. Остальное — по мере необходимости.

**Если вы ИИ-агент:**
1. `docs/optional/BRIEF_FOR_AGENTS.md` — что делать и как.
2. `docs/optional/ROADMAP.md` — где мы в плане.
3. Текущее ТЗ (`docs/TZ_vX.Y_*.md`) — конкретная задача.
4. `docs/ARCHITECTURE.md` — если нужно понять контекст.

---

## Корневые файлы

| Файл | Назначение |
|------|-----------|
| `README.md` | Входная точка: что это, быстрый старт, ограничения |
| `MANIFEST.md` | Этот файл: карта репозитория |
| `CHANGELOG.md` | История версий |
| `LICENSE` | MIT |
| `docker-compose.yml` | Postgres + watchdog + 2 воркера + cli |
| `Dockerfile` | Rust 1.98 builder + Debian runtime |
| `.env.example` | Шаблон переменных окружения |
| `Cargo.toml` | Workspace: rust-version = 1.85 |
| `Cargo.lock` | Зафиксированные версии |

---

## `schema/`

| Файл | Назначение | Статус |
|------|-----------|--------|
| `schema_v0.sql` | DDL + 19 функций. Источник истины | Готов, 26 тестов |
| `roles.sql` | Разделение прав БД | v1.0 |

---

## `crates/`

| Файл | Назначение | Статус |
|------|-----------|--------|
| `swarm-core/` | Библиотека: dispatcher, errors, types | Готов |
| `swarm-watchdog/` | Цикл reclaim + promote | Готов |
| `swarm-worker/` | Воркер-заглушка | Готов |
| `swarm-cli/` | seed / status / review / reopen / unblock | Готов |
| `swarm-planner/` | LLM-декомпозиция | v0.2 |
| `swarm-web/` | Веб-доска | v0.4 |

---

## `tests/`

| Файл | Назначение | Статус |
|------|-----------|--------|
| `test_v0.py` | 26 SQL-тестов на Postgres 16 | Готов |
| `integration.rs` | 5 Rust integration-тестов | Готов |
| `test_planner.py` | Тесты Planner | v0.2 |

---

## `bench/`

| Файл | Назначение | Статус |
|------|-----------|--------|
| `load_test.py` | 1000 задач, 50 воркеров | v1.0 |
| `RESULTS.md` | Результаты замеров | v1.0 |

---

## `docs/` — текущие документы

| Файл | Назначение | Когда читать |
|------|-----------|--------------|
| `PROJECT_STATE.md` | Что есть, что нет, что отложено | Первым после README |
| `METHODOLOGY.md` | 10 принципов работы | Вторым |
| `ARCHITECTURE.md` | 6 диаграмм Mermaid | При вопросах «как устроено» |
| `ARCHITECTURE_DECISIONS.md` | 11 ADR: почему так, а не иначе | При вопросах «почему» |
| `PLATFORM_COMPARISON.md` | Обзор 7 платформ | Для сравнения с альтернативами |
| `THREAT_MODEL.md` | 9 векторов атак + криптография | Перед v2 (OPA, approvals) |
| `BRIEF_FOR_AGENTS.md` | Что делать ИИ-агенту | Агенту-исполнителю |
| `PLAIN_EXPLANATION.md` | Простыми словами | Для не-технических читателей |
| `ROADMAP.md` | v0.1 → v1.0, фазы, оценки | Третьим |
| `NEXT_STEPS.md` | Что делать прямо сейчас | После ROADMAP |
| `KNOWN_LIMITATIONS.md` | Что НЕ работает | Перед принятием решений |
| `VERIFICATION_LOG.md` | Независимые прогоны | Для доказательств |
| `ERRORS.md` | Таблица кодов LS001…WK002 | При разборе ошибок |
| `DOC_STRATEGY.md` | Как хранить документацию | При добавлении файлов |
| `NATIVE_SETUP.md` | Развёртывание без Docker | Для сред без root |
| `TZ_v0.2_step1_fixes.md` | ТЗ: правки v0.1 | Агенту |
| `TZ_v0.2_step2_planner.md` | ТЗ: Planner | Агенту (v0.2) |
| `REPORT_TEMPLATE.md` | Шаблон тех. отчёта | При публикации |

---

## `docs/history/` — архив

| Файл | Что | Почему в архиве |
|------|-----|-----------------|
| `AUDITED_DOC_v15.docx` | 14 итераций архитектурной ревизии | Redis → Postgres, Python → Rust, 100 стр. → 25 |
| `REPORT_v0_e2e.md` | Отчёт верификации v0.1 | Исторический контекст |
| `swarm_fixes.diff` | Diff правок v0.1 | Внесён в канонический репозиторий |
| `README.md` | Описание архива | Навигация |

---

## Порядок чтения для нового участника

1. `README.md` — 5 минут.
2. `docs/PROJECT_STATE.md` — 10 минут.
3. `docs/METHODOLOGY.md` — 10 минут.
4. `docs/optional/ROADMAP.md` — 10 минут.
5. `docs/optional/NEXT_STEPS.md` — 5 минут.
6. `docs/ARCHITECTURE.md` — 20 минут.
7. `schema/schema_v0.sql` — 30 минут.
8. `tests/test_v0.py` — 20 минут.

**Итого: ~1.5 часа** до полного понимания проекта.

---

## Что делать после чтения

1. Запустить: `docker compose up -d --build` или `swarm-test start`.
2. Проверить: `pytest tests/test_v0.py -q` → 26 passed.
3. Взять задачу из `docs/optional/NEXT_STEPS.md` (ближайшая — step 1 фазы v0.2).
4. Перед PR — прочитать `docs/METHODOLOGY.md`, проверить соответствие.
