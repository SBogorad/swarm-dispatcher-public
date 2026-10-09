# RESUME: точка восстановления проекта Swarm Dispatcher

**Дата создания:** 2026-10-08
**Обновлено:** 2026-10-08 15:50 GMT+7
**Текущее состояние:** v0.1 закрыт; **step 1 и step 2 фазы v0.2 приняты**; следующий — **step 2b (идентификация воркера)**
**Что делать при перезапуске:** см. раздел 5

---

## 1. Проект одной строкой

**Swarm Dispatcher** — Postgres-first оркестратор графа задач для
batch-сценариев на слабом гетерогенном железе. Fencing tokens,
хэш-цепочка событий, Guarded FSM. Rust + Postgres 16+.

**Цель владельца (Розентальс Павел Юрьевич):**
- Доска для управления ИИ-агентами: видеть, кто взял задачу, что сделано.
- Распределение задач на разные машины.
- Фронтир как база (не срезать до минимума).

---

## 2. Где мы сейчас

| Артефакт | Статус |
|----------|--------|
| `schema/schema_v0.sql` | ✅ Готов, 26 тестов проходят, 6 таблиц, 19 функций |
| `crates/swarm-core/` | ✅ Готов, dispatcher + errors + types |
| `crates/swarm-watchdog/` | ✅ Готов, reclaim + promote |
| `crates/swarm-worker/` | ✅ Готов (заглушка без LLM) |
| `crates/swarm-cli/` | ✅ Готов, seed/status/review/reopen/unblock |
| `Dockerfile` + `docker-compose.yml` | ✅ Готовы |
| `docs/` | ✅ Готовы (6 основных + `docs/optional/` 15 + `docs/history/`) |
| **Тег `v0.1`** | ✅ Поставлен |
| **E2E верификация v0.1** | ✅ OpenClaw на Omarchy + syntog, все тесты прошли |
| **Step 1 v0.2 (правки)** | ✅ Принят. `REPORT_v0.2_step1.md`. 0 err/0 warn, 8 lib + 10 integration + 3 alt, 26 SQL, 30 ролей, M1–M3 пойманы |
| **Step 2 v0.2 (роли)** | ✅ Принят. `step2/roles.sql` + `test_roles.py`, 30 тестов на PG 16.13 и 18.6 |
| **Docker-проверка step 1–2** | ⚠️ Отложена (нет sudo). Трекается: `docs/KNOWN_LIMITATIONS.md` §5.5 |
| **Step 2b (идентификация воркера)** | ⏳ Следующий. Ждёт решения автора: A (Claude) / B1 (OpenClaw) |

**Доказательство работы:**
- Compose e2e: `{"reclaimed":1}` в логе watchdog.
- Fencing: epoch 0→1→2→3, зомби-воркер отсечён.
- `verify_chain=true` по всем задачам.

---

## 3. Кто участники

| Роль | Кто | Что делает |
|------|-----|-----------|
| **Владелец** | Розентальс Павел Юрьевич | Приоритеты, приёмка |
| **Реализатор** | OpenClaw / агент на ноутбуке (nouthp88) | Реализация ТЗ, верификация |
| **Архитектор** | Чат-ассистент (я, DeepSeek) | ТЗ, патчи, разбор ошибок |
| **Второе мнение** | Claude, DeepSeek (внешние) | Аудит, находки |

---

## 4. Ключевые файлы (что читать первым)

**Для нового чат-ассистента, чтобы продолжить:**

1. `RESUME.md` — этот файл.
2. `docs/PROJECT_STATE.md` — что есть сейчас.
3. `docs/METHODOLOGY.md` — принципы работы.
4. `docs/ROADMAP.md` — план v0.1 → v1.0.
5. `docs/NEXT_STEPS.md` — что делать прямо сейчас.
6. Последнее ТЗ — что делать конкретно.
7. `docs/ARCHITECTURE.md` — 6 диаграмм.

**Для OpenClaw / агента на ноутбуке:**

1. `docs/BRIEF_FOR_AGENTS.md` — что делать и как.
2. Текущее ТЗ (`docs/TZ_v0.2_step1_fixes.md`).
3. `docs/ARCHITECTURE.md` — если нужен контекст.

---

## 5. Что делать при перезапуске чата

### 5.1. Если чат с архитектором (со мной) оборвался

**Прикрепить в новое сообщение:**
1. `RESUME.md` (этот файл).
2. `docs/PROJECT_STATE.md`.
3. `docs/NEXT_STEPS.md`.

**Написать:**
> Продолжаем проект Swarm Dispatcher. v0.1 закрыт. Начинается v0.2
> (правки v0.1 + Planner). Что должно быть сделано: см. RESUME.md и
> NEXT_STEPS.md. Текущее ТЗ передано OpenClaw — ждём отчёт.
> Продолжаем с этого места.

**Я (новый чат-ассистент) должен:**
1. Прочитать приложенные файлы.
2. Проверить, пришёл ли отчёт от OpenClaw.
3. Если пришёл — разобрать, внести правки, написать следующее ТЗ.
4. Если нет — ждать, предложить параллельную работу (например, начать
   код `swarm-planner`).

### 5.2. Если чат с OpenClaw / агентом на ноутбуке оборвался

**Прикрепить:**
1. `docs/TZ_v0.2_step1_fixes.md` — текущее ТЗ.
2. `docs/BRIEF_FOR_AGENTS.md`.
3. `RESUME.md`.

**Написать:**
> Продолжаем. Отчёт по v0.1 принят. Сейчас — step 1 фазы v0.2
> (правки v0.1). ТЗ — в приложении. После приёмки step 1 — step 2
> (Planner). Репозиторий: `~/Projects/rust/swarm-dispatcher/`.

---

## 6. Что должно быть передано OpenClaw прямо сейчас

Если чат оборвётся **до** того, как вы передали ТЗ — вот что передать:

**Файл 1:** `docs/TZ_v0.2_step1_fixes.md` (7 правок v0.1).

**Файл 2:** `RESUME.md` (этот).

**Инструкция для OpenClaw (копировать целиком):**

```

Продолжаем. v0.1 закрыт с тегом.

Твоя задача — step 1 фазы v0.2: 7 правок v0.1. Полное ТЗ —
в файле TZ_v0.2_step1_fixes.md.

Кратко:

1. Cargo.toml: rust-version 1.75 → 1.85.
2. Dockerfile: rust 1.80 → 1.98 + COPY --chmod=644 schema.
3. docker-compose.yml: убрать version, убрать command из cli.
4. README: обновить команды + требование Rust 1.85+.
5. errors.rs: #[source] → #[from].
6. worker/main.rs: клон Lease перед thread::spawn (E0521).
7. errors.rs тест: проверка варианта, не текста.

После правок прогнать:

· cargo build (0 err, 0 warn)
· cargo test --lib (8 passed)
· cargo test --test integration -- --ignored (5 passed)
· pytest tests/test_v0.py (26 passed)
· docker compose up -d --build

Отчёт — одним markdown-файлом REPORT_v0.2_step1.md:

· diff каждой правки
· выводы тестов
· замечания

Не переходить к Planner, пока step 1 не принят.

```

---

## 7. Состояние документации

**Что уже сохранить в репозиторий (если ещё не):**

Всего написано **18+ файлов**:
- 12 из серии «v0.1/v0.2» (CHANGELOG, LICENSE, KNOWN_LIMITATIONS,
  VERIFICATION_LOG, ERRORS, history/README, NATIVE_SETUP,
  BRIEF_FOR_AGENTS, MANIFEST, README, TZ_step2_planner,
  REPORT_TEMPLATE).
- 6 из серии «стратегия» (ROADMAP, TZ_step1_fixes, DOC_STRATEGY +
  предыдущие PROJECT_STATE, METHODOLOGY, ARCHITECTURE).

**Если какие-то не сохранились** — при запросе в новом чате я могу
восстановить любые из них, если дать контекст. Но лучше сохранить
все сейчас.

**Приоритет сохранения** (если не успеваете всё):
1. `RESUME.md` (этот файл) — критично.
2. `docs/TZ_v0.2_step1_fixes.md` — передать OpenClaw.
3. `docs/ROADMAP.md` — план.
4. `docs/METHODOLOGY.md` — принципы.
5. Остальное — по мере.

---

## 8. Что дальше (по плану)

**Обновление 2026-10-08 (Docker-гэп закрыт):**

- **Docker-гэп (§5.5) — ✅ ЗАКРЫТ.** Оператор добавил себя в группу
  `docker`; `docker compose up -d --build` — полный стек healthy,
  `scripts/02-roles.sh` + `03-roles-2b.sh` + `03-role-patches.sh` применены,
  раздельные DSN по ролям, e2e в контейнерах под реальными ролями
  (Planner → Worker → артефакт с кодом → review → done, `verify_chain`=true).
- Найдено и исправлено 7 дефектов конфигурации (см.
  `docs/KNOWN_LIMITATIONS.md` §5.5): пароли ролей в `.env`; DSN
  суперпользователя вместо роли; `worker_id` vs роль step 2b; LLM-конфиг
  воркера; права tmpfs артефактов; несобираемый `cli` (profiles); planner
  в образе/compose.
- v0.4 (shell tools + sandbox) — ТЗ сохранено
  (`docs/TZ_v0.4_shell_tools_sandbox.md`). Docker теперь доступен
  (через `newgrp docker`); реализация ещё не начиналась.
- Следующее: v0.4 (shell tools), v0.5 (git worktree), v1.0 (нагрузка).

**Обновление 2026-10-08 (после v0.3.1):**

- Step 1, 2, 2b, 3 — ✅
- v0.3 (LLM-worker) — ✅
- v0.3.1 (description) — ✅ (артефакт содержит реальный код)
- v0.4 (shell tools + sandbox) — **ТЗ выдано, в работе**. ⚠️ sudo недоступен
  (`sudo -n` → NOT available, пользователь не в docker-группе) — Docker-часть
  (§8/§10 ТЗ) заблокирована, см. вопрос автору.
- Порядок применения: `schema → roles → roles_2b → roles_patch_planner_status`
  (зафиксирован в README, NATIVE_SETUP §4, compose + `scripts/03-*.sh`).
- Docker-гэп закрывается в v0.4 (нужен sudo).
- Следующее: v0.5 (git worktree), v1.0 (нагрузка).

**Обновление 2026-10-08 (после v0.3):**

- Step 1 ✅, Step 2 ✅, Step 2b ✅, Step 3 (Planner) ✅, v0.3 (LLM-worker) ✅.
- v0.3.1 (патч: description через Planner → Worker) — **реализован и
  верифицирован** (`REPORT_v0.3.1_planner_description.md`). Ждёт приёмки.
- Архитектурный пробел выявлен OpenClaw: контракт корня не содержал
  description, e2e давал missing_input. Патч закрывает — артефакт теперь
  содержит реальный код.
- Грант `task_status` для `swarm_planner` — отдельный патч
  `step2/roles_patch_planner_status.sql` (применять после roles.sql/roles_2b.sql).
- Следующее: v0.4 (shell tools + sandbox), v0.5 (git), v1.0 (нагрузка).
- Docker-гэп закрывается вместе с v0.4 (там sandbox обязателен).

**Обновление 2026-10-08 (после v0.3, ранее):**

- Step 1 ✅, Step 2 ✅, Step 2b ✅, Step 3 (Planner) ✅.
- v0.3 (LLM-worker) — ТЗ выдано, **реализовано и верифицировано**
  (`REPORT_v0.3_llm_worker.md`). Ждёт приёмки автора.
- Docker-гэп (§5.5) трекается, батч после v0.3.
- Следующее: v0.4 (shell tools), v0.5 (git), v1.0 (нагрузка).
- Ключевой архитектурный шаг v0.3: вынос LLM-клиента в `swarm-llm`.

**Обновление 2026-10-08 15:50:**
- Step 1 v0.2 (правки): ✅ **принят** автором. Отчёт `REPORT_v0.2_step1.md`.
- Step 2 v0.2 (роли): ✅ **принят**. 30 тестов на PG 16.13 (Claude)
  и 18.6 (OpenClaw).
- Docker-гэп (критерии 7–9 step 1): отложен, не блокер. Трекается в
  `docs/KNOWN_LIMITATIONS.md` §5.5. Закрыть батчем после step 2b.
- Step 2b v0.2 (идентификация воркера): **следующий шаг**. Состав:
  роль `swarm_worker_<id>` на каждого воркера, сверка `session_user`
  внутри функций (`claim_task`, `heartbeat`, `save_artifact`,
  `complete_task`, `register_worker`), тесты на чужое имя/capabilities/
  аренду, 3+ мутации. Файлы: `step2b/roles_2b.sql`,
  `step2b/test_roles_2b.py`, `docs/TZ_v0.2_step2b_worker_identity.md`.
  **Кто пишет — ждёт решения автора (A: Claude / B1: OpenClaw).**
- Planner — **только после приёмки step 2b**. v0.3 (LLM-воркер) — после
  Planner.
- Порядок: step 1 ✅ → step 2 ✅ → step 2b → Planner → LLM-воркер.

**Изначальный план:**

**Сейчас:** OpenClaw вносит правки v0.1 (step 1).

**После приёмки step 1:** я пишу ТЗ на Planner + первую версию
`swarm-planner`.

**После Planner:** Worker с реальным LLM (v0.3).

**Потом:** Веб-доска (v0.4), Git-интеграция (v0.5).

**Финал:** Нагрузочный тест + публикация (v1.0).

**Оценка:** 4–5 недель до v1.0.

---

## 9. Что НЕ надо делать

- **Не срезать фичи** (хэш-цепочку, fencing, approvals) — владелец хочет
  фронтир как базу.
- **Не заменять swarm на OpenClaw** — это разные системы для разных задач.
- **Не тащить Docker на OpenClaw** — для одного агента это оверкилл.
- **Не менять schema_v0.sql** без согласования — 26 тестов на ней.
- **Не править существующие тесты, чтобы они прошли** — прав баг в коде.

---

## 10. Ключевые решения (не пересматривать)

1. Postgres — единственный источник истины.
2. Без Redis.
3. Fencing через `lease_epoch`.
4. Хэш-цепочка по задаче.
5. Rust для инфраструктуры, Python (потом) для LLM.
6. Транзакции короткие, inference вне транзакции.
7. Schema-first, Python — обёртка.
8. Тесты как арбитр.
9. MSRV 1.85+.
10. Два пути развёртывания: Docker (production) + Native (dev).

---

## 11. Проверка при перезапуске

**Проверить, что всё работает:**

```bash
cd swarm-dispatcher

# Сборка
cargo build --release 2>&1 | tail -3
# Ожидается: Finished

# Unit-тесты
cargo test --lib 2>&1 | tail -3
# Ожидается: 8 passed

# SQL-тесты (нужен Postgres)
pytest tests/test_v0.py -q
# Ожидается: 26 passed

# Docker
docker compose up -d --build
docker compose run --rm --no-deps cli swarm-cli status
# Ожидается: таблица задач
```

Если всё работает — система готова, продолжаем v0.2.

Если что-то падает — написать в чат с указанием:

· Что именно падает (команда).
· Что ожидалось.
· Что получено (лог).
· Что было сделано до этого.

---

12. Контактные точки

Репозиторий: ~/Projects/rust/swarm-dispatcher/ (локально).

Внешний репозиторий: (если создан — вписать ссылку).

Postgres native: порт 55440, user-space.

Docker Compose: стандартные порты.

Скрипт управления native-средой: ~/.local/bin/swarm-test.

---

13. Резюме одной строкой

Swarm Dispatcher v0.1 закрыт и верифицирован. v0.2 в работе:
OpenClaw вносит правки, архитектор пишет Planner. Точка восстановления —
этот файл. При перезапуске — приложить в чат, продолжить с раздела 5.
