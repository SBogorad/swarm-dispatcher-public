# Карта репозитория

Полный список файлов с описанием: что внутри, зачем, когда читать.

**Актуально на:** v0.5.2 + Unreleased (в работе)

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
5. `docs/AGENTS.md` — правила работы с этим репозиторием.

---

## Корневые файлы

| Файл | Назначение |
|------|-----------|
| `README.md` | Входная точка (английский): что это, быстрый старт, ограничения |
| `README.ru.md` | Входная точка (русский): тот же материал, не перевод-калька |
| `CONTRIBUTING.md` | Как проходит работа: гейт, скоуп PR, стандарт тестов |
| `SECURITY.md` | Приватный канал для уязвимостей + принятые ограничения |
| `CODE_OF_CONDUCT.md` | Contributor Covenant 2.1 |
| `MANIFEST.md` | Этот файл: карта репозитория |
| `CHANGELOG.md` | История версий |
| `RESUME.md` | Снимок состояния на конец v0.2 (исторический) |
| `LICENSE` | MIT |
| `Cargo.toml` | Workspace, `rust-version = 1.94.1` |
| `Cargo.lock` | Зафиксированные версии |
| `Dockerfile` | Rust builder + Debian runtime |
| `docker-compose.yml` | Боевой контур: postgres + watchdog + 2 воркера + cli |
| `docker-compose.demo.yml` | Демо-контур: postgres + демо-UI + воркеры под своими ролями БД |
| `.env.example` | Шаблон переменных окружения |
| `sgconfig.yml` | Конфиг ast-grep (архитектурные проверки) |
| `deploy.sh` | Скрипт развёртывания |
| `REPORT_*.md` | Снимки прогонов/верификаций (исторические, не поддерживаются) |

---

## `schema/`

| Файл | Назначение | Статус |
|------|-----------|--------|
| `schema_v0.sql` | DDL + 25 функций. Источник истины | Готов |
| `migrations/` | 13 инкрементальных миграций (001–013) | Применяются на пустом томе |

Разделение прав БД лежит не здесь, а в `step2/roles.sql` и `step2b/roles_2b.sql`.

---

## `crates/` — Rust workspace

| Крейт | Назначение | Статус |
|------|-----------|--------|
| `swarm-core/` | Библиотека: dispatcher, errors, types | Готов |
| `swarm-llm/` | Клиент LLM (HTTP, `relay.fast`), ретраи, обрезка тел ответов | Готов |
| `swarm-planner/` | LLM-декомпозиция + валидатор плана | Готов |
| `swarm-worker/` | Воркер: agent loop, allowlist, playbooks, cost ledger | Готов |
| `swarm-reviewer/` | Сервис-ревьюер: закрывает `review` → `done` | Готов |
| `swarm-watchdog/` | Цикл `reclaim_expired` + `promote_ready` | Готов |
| `swarm-cli/` | seed / status / review / reopen / unblock | Готов |

Веб-доски (`swarm-web`) в репозитории **нет**: есть демо-консоль на Python
(`demo/`), продовой веб-доски нет — см. `docs/KNOWN_LIMITATIONS.md` §1.5.

---

## `tests/` — арбитры (Postgres + статические)

| Файл | Назначение |
|------|-----------|
| `test_v0.py` | SQL-тесты инвариантов на Postgres 16 (38 тестов) |
| `conftest.py` | Общие фикстуры/DSN |
| `fixtures/` | Данные для тестов |
| `test_{contract_f15,fail_unblock_f4,find04_*,review_gate_*}.py` | Арбитры волн (триады) |
| `test_worker_identity_f3.py` | Identity воркера: `fail_task`/`fail_task_state` по чужой аренде |
| `test_dag_handoff_iteration3.py` | DAG handoff: `task_parent_artifacts` |
| `test_cost_reconciliation.py` | Достоверность cost ledger |
| `test_model_{catalog,quality}.py` | Выбор модели по измеренной цене успеха |
| `test_f8_orphan_llm_spend.py` | Бюджет защищает реальный расход, а не только `complete` |
| `test_no_duplicate_function_bodies.py` | Дрейф тел функций `schema_v0.sql` ↔ `roles_2b.sql` |
| `test_known_limitations_refs.py` | Дрейф ссылок `file:line` в `docs/KNOWN_LIMITATIONS.md` |
| `test_docs_link_drift.py` | Дрейф ссылок по всем живым документам |
| `test_demo_worker_roles.py` | Демо-воркеры под своими ролями БД, а не под суперпользователем |
| `test_*.sh` | Оболочные арбитры (F1 artifact visibility, F6 prompt injection) |

Rust-интеграция лежит рядом с крейтами: `crates/*/tests/*.rs`
(например `crates/swarm-core/tests/integration_verified.rs`,
`crates/swarm-planner/tests/planner_integration.rs`,
`crates/swarm-worker/tests/worker_{integration,sandbox,runtime_gate}.rs`).

---

## `scripts/`

| Файл | Назначение |
|------|-----------|
| `acceptance_harness.sh` | STRICT acceptance-гейт (зеркало CI) |
| `acceptance_allowlist.txt` | Скоуп гейта: какие пути он рассматривает |
| `apply_migrations.sh` | Раннер миграций для ЖИВОГО тома (реестр, план, откат) |
| `compose_roles_arbiter.sh` | Живой арбитр ролей (DSN-роль + права в каталоге) |
| `02-roles.sh`, `03-roles-2b.sh`, `03-role-patches.sh`, `05-migrations.sh` | initdb-скрипты контейнера postgres (пустой том) |
| `run_mutations_{v04,sql}.sh` | Мутационные прогоны (Rust M1–M6, SQL S1–S5) |
| `build_audit_{bundle,index,report}.py`, `build_audit_zip.sh` | Сборка аудит-пакета |
| `secret_scan.sh` | Скан секретов в диапазоне диффа |
| `scratch_pg.sh` | Безопасный scratch-контейнер Postgres |
| `start_demo.sh` | Запуск демо-контура |
| `model_quality_report.py`, `fetch_model_catalog.py` | Отчёты по качеству/каталогу моделей |
| `reconcile_llm_cost.py` | Сверка расхода с `relay.fast` |

---

## `step2/`, `step2b/` — RBAC и идентификация воркера

| Файл | Назначение |
|------|-----------|
| `step2/roles.sql` | Роли БД + `task_status` (принят, не менять) |
| `step2/roles_patch_planner_status.sql` | Патч гранта планировщика |
| `step2/roles_patch_tool_call_audit.sql` | Патч гранта аудита `tool_call` |
| `step2/test_roles.py` | 30 тестов ролей |
| `step2/test_review_gate.py` | Арбитр review-гейта |
| `step2b/roles_2b.sql` | Идентификация воркера (`assert_worker_identity`) |
| `step2b/test_roles_2b.py` | Тесты идентификации |

---

## `demo/` — демо-контур

| Путь | Назначение |
|------|-----------|
| `demo/engine/reviewer.py` | Демо-ревьюер |
| `demo/lib/decompose.py` | Демо-декомпозиция |
| `demo/bridge/api.py` | HTTP-мост к Postgres |
| `demo/ui/app.py` | Демо-UI |
| `demo/db/client.py` | Клиент БД |
| `demo/schema_extras.sql` | Доп. объекты демо-схемы |
| `PRESENTATION.md`, `README.md` | Описание и сценарий демонстрации |
| `Dockerfile`, `requirements.txt` | Образ демо |

---

## `bench/`

| Файл | Назначение |
|------|-----------|
| `load_test.py` | Нагрузочный тест (1000 задач) |
| `c2_test.py`, `fault_test.py` | C2- и fault-тесты |
| `README.md`, `REPORT.md` | Методика и отчёт |
| `results/` | Артефакты прогонов (не версионируются) |

---

## `ops/`

| Файл | Назначение |
|------|-----------|
| `db_backup.sh` | Бэкап БД |
| `docker-compose.shell-tools.yml` | Контур shell-tools (не включается по умолчанию) |
| `docker-socket-proxy.md` | Описание socket-proxy |

---

## Прочее

| Путь | Назначение |
|------|-----------|
| `sandbox/Dockerfile` | Образ `agent-sandbox` для изолированного исполнения |
| `artifacts/` | Артефакты прогонов (адресуются по `sha256`) |
| `rust_fixes/` | Патчи, оставшиеся от миграции v0.1 |
| `tasks/` | YAML-задачи self-improve-контура |
| `outbox/` | Пакеты аудита/релизов (не версионируются) |
| `.github/workflows/release-gate.yml` | CI: STRICT-гейт + мутационное тестирование |

---

## `docs/` — текущие документы

Кроме явно помеченных, все документы живут в `docs/optional/` — это
«необязательный круг чтения», а не мусор.

| Файл | Назначение | Когда читать |
|------|-----------|--------------|
| `PROJECT_STATE.md` | Что есть, что нет, что отложено | Первым после README |
| `METHODOLOGY.md` | 10 принципов работы | Вторым |
| `ARCHITECTURE.md` | Диаграммы Mermaid | При вопросах «как устроено» |
| `AGENTS.md` | Правила работы ИИ-агента с репо | Агенту, перед PR |
| `KNOWN_LIMITATIONS.md` | Что НЕ работает сейчас | Перед принятием решений |
| `VERIFICATION_LOG.md` | Независимые прогоны | Для доказательств |
| `DISTRIBUTED_WORKER_MVP.md` | Критерии v0.6 | Перед распределённым контуром |
| `TZ_v0.2_step1_fixes.md` | ТЗ: правки v0.1 | Историческое ТЗ |
| `TZ_v0.2_step2_planner.md` | ТЗ: Planner | Историческое ТЗ |
| `TZ_v0.2_step2b_worker_identity.md` | ТЗ: идентификация воркера | Историческое ТЗ |
| `TZ_v0.3_llm_worker.md` | ТЗ: воркер с LLM | Историческое ТЗ |
| `TZ_v0.3.1_planner_description.md` | ТЗ: описание планировщика | Историческое ТЗ |
| `TZ_v0.4_shell_tools_sandbox.md` | ТЗ: shell-tools + sandbox | Историческое ТЗ |
| `docs/optional/ARCHITECTURE_DECISIONS.md` | ADR: почему так, а не иначе | При вопросах «почему» |
| `docs/optional/PLATFORM_COMPARISON.md` | Обзор платформ | Для сравнения с альтернативами |
| `docs/optional/THREAT_MODEL.md` | Векторы атак + криптография | Перед v2 (OPA, approvals) |
| `docs/optional/BRIEF_FOR_AGENTS.md` | Что делать ИИ-агенту | Агенту-исполнителю |
| `docs/optional/ROADMAP.md` | v0.1 → v1.0, фазы, оценки | Третьим |
| `docs/optional/NEXT_STEPS.md` | Что делать прямо сейчас | После ROADMAP |
| `docs/optional/ERRORS.md` | Таблица кодов LS001…WK002 | При разборе ошибок |
| `docs/optional/DOC_STRATEGY.md` | Как хранить документацию | При добавлении файлов |
| `docs/optional/NATIVE_SETUP.md` | Развёртывание без Docker | Для сред без root |
| `docs/optional/AUDIT_HISTORY.md` | История дефектов | Для контекста аудита |
| `docs/optional/RUST_FINDINGS.md` | Находки по Rust | При работе с крейтами |
| `docs/optional/CLAUDE_DIALOGUE.md` | Архив диалога | Историческое |
| `docs/optional/REPORT_TEMPLATE.md` | Шаблон тех. отчёта | При публикации |
| `docs/optional/TZ_v0.2_step2_roles.md` | ТЗ: роли БД | Историческое ТЗ |
| `docs/ci/CI_COST.md` | Стоимость CI | При изменении workflow |
| `research/*.md` | Research-заметки (рынок, TEE, zkML) | По интересу |
| `audit/**`, `history/**` | Замороженные срезы: аудит-пакеты и архив | Только как свидетельства |

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
2. Проверить: `pytest tests/test_v0.py -q` → 38 passed.
3. Взять задачу из `docs/optional/NEXT_STEPS.md`.
4. Перед PR — прочитать `docs/METHODOLOGY.md` и `docs/AGENTS.md`, проверить соответствие.
