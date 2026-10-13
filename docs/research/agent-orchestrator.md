# Research: agent-orchestrator (Orchestrator.inc) — integration patterns

**Issue:** #8 · **Дата:** 2026-10-14 · **Статус:** research, не блокирует разработку
**Связано:** #34, #12, #4, #3, #5
**Источник:** https://github.com/OrchestratorInc/agent-orchestrator (Apache-2.0, Go+TypeScript, ~13.1k★ на 2026-10-13)

> Примечание: issue ссылается на `Untrivial-ai/agent-orchestrator`, но репозиторий
> переехал/принадлежит организации **`OrchestratorInc`** (продукт «Orchestrator.inc»).
> Проверено через GitHub API: `full_name = OrchestratorInc/agent-orchestrator`,
> homepage `orchestrator.inc`.

---

## 1. Что это такое (по факту из репозитория)

Локальное **desktop-приложение + локальный демон**, а не серверная платформа.
README: «local desktop workspace». Каждой кодирующей задаче — свой агент, workspace
и цикл обратной связи; поверх — «project orchestrator» как планирующий агент и живой
Kanban. Ключевые факты из дерева репозитория и README:

- **Языки:** Go (~23.5 MB), TypeScript (~16.2 MB), + SQLite-миграции (PLpgSQL-подобный SQL
  в `backend/internal/storage/sqlite/migrations/`, ~45 KB). Значит: бэкенд-демон на Go,
  UI на TS/Electron.
- **Хранилище — SQLite, не Postgres.** `backend/internal/storage/sqlite/db.go`,
  миграции `0001_init.sql … 0030_session_cleanup_facts.sql` (goose).
- **Изоляция — git worktree.** `backend/internal/adapters/workspace/gitworktree/`:
  `workspace.go`, `remove.go`, `discard.go`, с per-repository teardown lock
  (`teardownLocks`) и защитой от path traversal (`ErrUnsafePath`).
- **CDC-лог.** Таблица `change_log` (append-only, `seq` = монотонный ключ), наполняется
  **SQL-триггерами**, а не кодом приложения; consumers читают по своему offset (SSE
  `Last-Event-ID`). Это прямая аналогия нашему `events`.
- **Планировщик автоматизаций.** `backend/internal/service/automation/scheduler.go`:
  `ListDueAutomations` → `MaterializeAutomationRuns` → `ClaimNextAutomationRun`
  (с `spawnLease = 2 * time.Minute`), `MarkAutomationRunRunning` / `Complete` / `Fail` /
  `Release`. Есть понятие **lease** и **claim** — концептуально родственное нашему
  `claim_task` / `lease_epoch`.

---

## 2. Архитектурные паттерны, релевантные swarm-dispatcher

### 2.1. «Один воркер = один git worktree» (изоляция файловой системы)

У нас `docs/KNOWN_LIMITATIONS.md` и аудиты (FIND-01, F16) фиксируют: sandbox не подключён
к циклу воркера, а `git worktree` — известный паттерн изоляции. AO доводит его до
продуктового уровня: worktree на сессию, отдельная ветка, теardown с блокировкой по
репозиторию.

**Что взять:** модель «задача → своя ветка/worktree» как штатный путь, а не опция.
Это закрывает часть риска коллизий между параллельными воркерами на одном репозитории.

### 2.2. Факты выводятся, а не хранятся (derived status)

В `0001_init.sql` прямо сказано: «display status is derived on read from this row plus
PR facts». В таблице `sessions` хранятся только `activity_state` и `is_terminated`;
позиция карточки на Kanban вычисляется из сессии + PR + CI + review.

**Что взять:** наш `cli status` — это уже «derived view». Идея расширяется: состояние
наблюдения (где воркер, что с PR) не должно дублироваться в БД — оно выводится из
первичных фактов (`tasks`, `events`, `reviews`). Это соответствует нашему Принципу 2
(schema-first, единственный источник истины).

### 2.3. CDC через триггеры, а не emit-код

AO пишет `change_log` триггерами «DB-native capture, atomic with the change — never by
application emit-code». У нас `append_event` вызывается из функций в той же транзакции
(`schema_v0.sql:110`) — тот же принцип, другая реализация (функция vs триггер).
Подтверждает наш выбор: события пишутся атомарно с изменением.

### 2.4. Lease/claim у планировщика автоматизаций

`scheduler.go`: `spawnLease = 2 * time.Minute`, `ClaimNextAutomationRun`. Это тот же
паттерн fencing/lease, что у нас, но применённый к запуску автоматизаций, а не к задачам.
Идея переиспользуема для нашего watchdog/planner.

---

## 3. Механизмы координации: Go vs Rust/PostgreSQL

| Аспект | agent-orchestrator | swarm-dispatcher |
|---|---|---|
| Хранилище | SQLite (локально, одна машина) | PostgreSQL 16 (единственный источник истины) |
| Распределённость | Локальный демон, один хост | Гетерогенный кластер, много воркеров |
| Захват работы | `ClaimNextAutomationRun` + lease 2 мин | `claim_task` + `FOR UPDATE SKIP LOCKED` + `lease_epoch` |
| Изоляция | git worktree на сессию | Docker sandbox (пока не в цикле) + общий volume |
| Аудит | `change_log` (триггеры, SSE) | `events` (хэш-цепочка, `verify_chain`) |
| Fencing | lease у automation run | монотонный `lease_epoch` |
| Ревью | PR + agent review, возврат правок владельцу | `submit_review` + `swarm-reviewer` |

**Ключевое различие.** AO — **single-host, локальный** инструмент разработчика
(SQLite, desktop). Мы — **batch-диспетчер для гетерогенного кластера** с Postgres как
единственной точкой истины. Прямая интеграция «взять их БД» невозможна: у них SQLite.

---

## 4. Интеграция CI/CD и ревью

AO отслеживает PR, CI-runs, reviews и mergeability, отображая их на Kanban, и умеет
возвращать замечания ревью «тому же владельцу». Схема (`pr`, `pr_checks`, `pr_comment`)
— нормализованная модель внешних фактов GitHub.

**Что взять (без копирования кода):**
1. **Нормализовать внешние факты PR/CI в таблицы** (`pr`, `pr_checks`) вместо хранения
   сырого JSON. У нас Git-интеграция отсутствует (`docs/KNOWN_LIMITATIONS.md` §1.4) —
   это ориентир на v0.5+.
2. **Замкнуть цикл «review → правки → тому же воркеру».** У нас `swarm-reviewer`
   закрывает `review → done` (Issue #26), но не возвращает правки автору итеративно.
   Паттерн AO «return requested changes to the same owner» — конкретное улучшение.

---

## 5. Kanban UI для визуализации задач

У нас веб-доски нет (`docs/KNOWN_LIMITATIONS.md` §1.5, «Когда: v0.4»). AO показывает,
что Kanban **выводится** из фактов (session + PR + CI + review), а не хранится.
Для нас эквивалент: доска по `tasks.state` + `events` + `reviews`, без новых таблиц.

**Вывод:** идея доски совместима с нашей моделью; конкретный UI AO не переносим
(TS/Electron против нашего Rust/CLI), но принцип «derived board» — да.

---

## 6. Сравнение с текущей моделью swarm

| Наша сущность | Аналог в AO | Замечание |
|---|---|---|
| task DAG (`tasks`, `task_deps`, `add_dep`) | сессии/автоматизации | У AO граф задач не выражен как DAG подзадач; это наш差异化 |
| workers + capabilities | «worker session» (агент+модель) | AO не матчит железо; мы матчим `needs` vs `capabilities` |
| watchdog (`reclaim_expired`, `promote_ready`) | automation scheduler | У AO — расписание; у нас — reclaim истёкших аренд |
| `lease_epoch` fencing | lease (2 мин) | Мы строже: монотонный счётчик против зомби |
| `verify_chain` | `change_log` | У AO нет криптографической цепочки |

---

## 7. Рекомендация

| Направление | Вердикт | Обоснование |
|---|---|---|
| Прямая интеграция с AO (код/БД) | **Отвергнуть** | Другой стек (Go/SQLite/desktop), другой сценарий (single-host vs кластер) |
| Паттерн «задача → git worktree» | **Внедрять** (v0.5+) | Закрывает риск коллизий; совместим с нашим sandbox-планом |
| Derived status / derived Kanban | **Внедрять** | Соответствует schema-first; доска строится из первичных фактов |
| CDC через триггеры | **Отложить** | У нас уже есть эквивалент (`append_event` в транзакции); триггеры — альтернатива, не срочно |
| Нормализация PR/CI фактов | **Отложить** | Актуально, когда появится Git-интеграция (v0.5+) |
| Цикл «review → правки → автору» | **Внедрять** | Конкретное улучшение `swarm-reviewer` поверх Issue #26 |

**Итог:** agent-orchestrator — ценный источник **паттернов**, но не объект интеграции.
Брать: git-worktree-на-задачу, derived status/Kanban, итеративный цикл ревью.
Не брать: стек, SQLite-хранилище, desktop-модель.

---

## Источники (проверены 2026-10-14)

1. Repository — https://github.com/OrchestratorInc/agent-orchestrator (Apache-2.0)
2. `backend/internal/storage/sqlite/migrations/0001_init.sql` — схема (projects, sessions, pr, pr_checks, change_log)
3. `backend/internal/service/automation/scheduler.go` — lease/claim у планировщика
4. `backend/internal/adapters/workspace/gitworktree/workspace.go` — изоляция worktree, teardown lock, path-traversal guard
5. README — продуктовая модель (worker / orchestrator / Kanban)
