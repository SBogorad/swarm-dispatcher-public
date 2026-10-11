# Swarm Dispatcher

Reference implementation распределённой оркестрации задач для сценариев,
где важна выполнимость на слабом гетерогенном железе, а не latency.

## Что это

Postgres-центричный диспетчер задач с канбан-подобным контролем состояний.
Один источник истины, fencing tokens, хэш-цепочка событий, recursive cascade.
Без Redis, без внешнего durable execution, без OPA (в v0.1).

**Подходит для:**
- Batch-обработки больших задач (рефакторинг, анализ корпуса, генерация).
- Гетерогенного железа (разные модели у разных воркеров, разные RAM/GPU).
- Сценариев, где «8 часов на выполнение» — приемлемо.
- Локальных кластеров без облачных зависимостей.

**Не подходит для:**
- Интерактивных ассистентов (latency критична).
- Real-time систем.
- Одного воркера на одной машине (оверкилл, используйте проще).

---

## Требования

- **Rust 1.94.1+** (из-за `edition2024` в транзитивных зависимостях; MSRV
 задан в `Cargo.toml` как `rust-version = "1.94.1"`).
- **Postgres 16+** (проверено на 16.4 и 18.6).
- **Docker + Docker Compose** (для production-пути).
- ИЛИ **Linux + Postgres в user-space** (для dev-пути без root).

---

## Быстрый старт (Docker Compose)

```bash
# 1. Клонировать
git clone <repo> && cd swarm-dispatcher

# 2. Создать .env
printf 'DB_PASSWORD=%s\n' "$(head -c 24 /dev/urandom | base64 | tr -d '/+=')" > .env

# 3. Поднять стек
docker compose up -d --build

# 4. Проверить, что всё работает
docker compose logs watchdog | tail -5

# 5. Создать демо-задачи
docker compose run --rm --no-deps cli swarm-cli seed

# 6. Посмотреть статус
docker compose run --rm --no-deps cli swarm-cli status
```

Через 60 секунд должно быть видно: задачи в review и ready, в логах
сторожа — {"reclaimed": 1}.

Важно: CLI запускается через docker compose run --rm --no-deps cli
swarm-cli <command>. Не через docker compose run --rm cli seed — там
compose подменяет entrypoint.

Порядок применения SQL (важно)

Роли и патчи применяются строго в этом порядке (это же порядок в
`docker-compose.yml` через `/docker-entrypoint-initdb.d/`):

```
schema/schema_v0.sql
  → step2/roles.sql
  → step2b/roles_2b.sql
  → step2/roles_patch_planner_status.sql
  → step2/roles_patch_tool_call_audit.sql   (v0.4: аудит tool calls)
```

`step2/roles.sql` не меняется (принят в step 2). Новые гранты
добавляются отдельными патч-файлами вида `step2/roles_patch_*.sql`.
При применении вручную (без Docker) — выполнить все пять файлов
по порядку, иначе роли не получат нужных прав.

---

Быстрый старт (без Docker)

```bash
# 1. Установить Postgres в user-space (см. docs/NATIVE_SETUP.md)
# 2. Собрать
cargo build --release

# 3. Запустить Postgres
~/.local/bin/swarm-test start

# 4. Создать задачи
~/.local/bin/swarm-test seed

# 5. Запустить сторож и воркеры (в отдельных терминалах)
~/.local/bin/swarm-test watchdog
WORKER_ID=worker-1 ~/.local/bin/swarm-test worker

# 6. Проверить статус
~/.local/bin/swarm-test cli status
```

Подробнее — docs/NATIVE_SETUP.md.

---

Что уже работает

· Postgres 16+ — единственный источник истины.
· 19 SQL-функций (schema_v0.sql) + `task_status` (roles.sql) + `record_tool_call`
  (roles_patch_tool_call_audit.sql); всего 23 после применения
  roles.sql → roles_2b.sql → roles_patch_planner_status.sql → roles_patch_tool_call_audit.sql:
  add_task, add_dep, claim_task, heartbeat,
  save_artifact, complete_task, submit_review, reclaim_expired,
  reopen_task, unblock_task, promote_ready, register_worker,
  append_event, verify_chain + 6 вспомогательных.
· 30 тестов на инварианты: pytest tests/test_v0.py -> 30 passed (v0.5).
· Rust-тесты воркспейса: 118 lib + интеграционные (`integration_verified`, `integration_alt`, `worker_*`); полный прогон — `scripts/acceptance_harness.sh --full-workspace-test` (536 имён тестов, STRICT).
· v0.4: shell tools в Docker-sandbox (allowlist deny-by-default, timeout,
  лимит вывода, audit `tool_call`); unit 35 + integration 9.
· Fencing token через lease_epoch — защита от зомби-воркеров.
· Хэш-цепочка событий — non-repudiation.
· Recursive cascade — откат всего поддерева графа.

---

Что не в v0.1

· OPA / Policy Engine (v2).
· Approvals с Ed25519 / Trust Registry (v2).
· OpenTelemetry (v1).
· Rate limiting (v1).
· Мультитенантность (v2).
· Реальный LLM в воркере (v0.3).
· Planner / LLM-декомпозиция (v0.2).
· Веб-доска (v0.4).

Полный список — docs/ROADMAP.md и docs/KNOWN_LIMITATIONS.md.

---

Структура

```
swarm-dispatcher/
├── README.md                    # этот файл
├── MANIFEST.md                  # карта репозитория
├── CHANGELOG.md                 # история версий
├── LICENSE                      # MIT
├── Cargo.toml                   # workspace, rust-version = 1.94.1
├── Dockerfile                   # rust:1.98-slim + Debian runtime
├── docker-compose.yml           # postgres + watchdog + 2 воркера + cli
├── .env.example
│
├── schema/
│   └── schema_v0.sql            # DDL + 19 функций
│
├── step2/
│   ├── roles.sql                # роли + task_status (принят, не менять)
│   └── roles_patch_planner_status.sql  # патч гранта (после roles/roles_2b)
│
├── step2b/
│   └── roles_2b.sql             # идентификация воркера
│
├── crates/
│   ├── swarm-core/              # библиотека
│   ├── swarm-watchdog/          # сторож
│   ├── swarm-worker/            # воркер
│   └── swarm-cli/               # CLI
│
├── tests/                       # Python-арбитры (Postgres)
│   ├── test_v0.py               # 30 SQL-тестов инвариантов
│   └── test_{contract_f15,fail_unblock_f4,find04_*,review_gate_*,
│       worker_identity_f3}.py   # арбитры волны 4 (триады)
│
├── crates/*/tests/              # Rust-интеграция по крейтам
│   ├── swarm-core/tests/integration_verified.rs
│   ├── swarm-core/tests/alt/integration_alt.rs
│   ├── swarm-worker/tests/{runtime_gate,worker_sandbox,worker_integration}.rs
│   └── swarm-planner/tests/planner_integration.rs
│
└── docs/                        # см. MANIFEST.md
```

---

Коды ошибок

Код Когда возникает
LS001 Запись с чужим, устаревшим или истёкшим lease
AR001 complete с неизвестным result_ref
CT001 Нет checks или нечисловое поле в контракте
GR000–GR009 Ошибки графа (цикл, глубина, лимиты)
RV001–RV003 Ошибки ревью (устаревшее, self-review, checks)
WK001–WK002 Ошибки воркера (невалидный, не зарегистрирован)

Полная таблица — docs/ERRORS.md.

### Контракт рецензента (ломающее изменение)

Начиная с v0.4.2 (ужесточение гейта `submit_review`, коммит `b9ea4d0`),
каждая проверка из `checks` контракта должна вернуться с совпадающим ключом:

- если в контракте у проверки был `id` — рецензент **обязан** вернуть тот же
  `id`;
- если `id` не было — должны совпасть `type` и `run` (команда проверки).

Возврат checks без `id` и без `run` отклоняется (`RV002/RV003`). Старые
клиенты рецензента, отправлявшие `{"type": "cmd", "status": "pass"}` без
`run`, должны его добавить.

---

Известные ограничения v0.1

1. save_artifact пишет только metadata, файл на диск не пишется.
2. Worker — заглушка (sleep), реального LLM нет.
3. Planner (LLM-декомпозиция) отсутствует.
4. Веб-доски нет — только cli status.
5. worker_id не аутентифицирован.
6. Нет backup / restore.

Полный список — docs/KNOWN_LIMITATIONS.md.

---

Дальше

План развития — docs/ROADMAP.md. Ближайший шаг — v0.2: правки v0.1

· Planner. Текущее ТЗ — docs/TZ_v0.2_step1_fixes.md.

Для ИИ-агентов: docs/BRIEF_FOR_AGENTS.md — что делать и как.

Для человека: docs/PLAIN_EXPLANATION.md — простыми словами.

Для новых участников: MANIFEST.md → docs/PROJECT_STATE.md →
docs/METHODOLOGY.md.

---

Лицензия

MIT. См. LICENSE.

```

---
