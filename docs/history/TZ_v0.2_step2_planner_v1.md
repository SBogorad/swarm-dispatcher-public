# ТЗ: Planner (шаг 2 фазы v0.2)

Планировщик: LLM-агент, который получает **большую задачу текстом** и
создаёт **граф контрактов** через `add_task` / `add_dep`.

Начинать **только после** приёмки step 1 (`TZ_v0.2_step1_fixes.md`).

---

## 1. Что делает Planner

**Вход:** текстовая задача («Сделать рефакторинг модуля X», «Проанализировать
корпус документов Y», «Сгенерировать API для Z»).

**Выход:** граф задач в БД. Каждая задача — контракт с:
- `kind` (тип задачи),
- `checks` (непустой список критериев приёмки),
- `needs` (RAM, класс модели, affinity-теги),
- `budget` (лимиты: attempts, max_depth, max_children, max_tasks).

**Ограничения:**
- Не более `max_tasks` задач в графе (по умолчанию 50).
- Глубина ≤ `max_depth` (по умолчанию 5).
- Ширина ≤ `max_children` (по умолчанию 10).
- Idempotency: повторный запуск с тем же `root_id` не создаёт дублей.

---

## 2. Архитектура

### 2.1. Новый крейт `swarm-planner`

```

crates/swarm-planner/
├── Cargo.toml
└── src/
├── main.rs         # CLI: swarm-planner "задача"
├── lib.rs          # Planner struct
├── llm.rs          # обёртка над LLM API
├── prompt.rs       # системный промпт, примеры
└── parse.rs        # парсинг ответа LLM → граф

```

### 2.2. Зависимости

```toml
[dependencies]
swarm-core = { path = "../swarm-core" }
tokio = { version = "1", features = ["rt", "macros"] }   # для HTTP
reqwest = { version = "0.12", features = ["json", "rustls-tls"] }
serde = { workspace = true }
serde_json = { workspace = true }
anyhow = "1"
tracing = { workspace = true }
tracing-subscriber = { workspace = true }
```

2.3. Env

Переменная Обязательно Пример
DATABASE_URL да postgres://...
PLANNER_LLM_PROVIDER да openai / anthropic / openai-compatible
PLANNER_LLM_API_KEY да sk-...
PLANNER_LLM_MODEL да gpt-4o-mini / claude-3.5-sonnet
PLANNER_LLM_BASE_URL для openai-compatible http://localhost:8080/v1
PLANNER_BUDGET_TOKENS нет (default 50000) —
PLANNER_MAX_RETRIES нет (default 2) —

---

3. Контракт LLM

3.1. Системный промпт

```
Ты — планировщик задач. Получаешь большую задачу и разбиваешь её на граф
подзадач (контрактов). Каждая подзадача должна быть:

1. Атомарной (одна ясная цель).
2. Проверяемой (список checks — команды/файлы, которые дадут pass/fail).
3. Иметь явные зависимости (что должно быть сделано до неё).

Ответ должен быть валидным JSON по схеме:
{
  "tasks": [
    {
      "id": "t1",
      "kind": "code.refactor",
      "description": "...",
      "checks": [{"type": "cmd", "run": "cargo test"}],
      "needs": {"min_ram_gb": 8, "coder_class": 2},
      "budget": {"attempts": 3, "max_rework": 2},
      "depends_on": []
    },
    ...
  ]
}

Правила:
- Не более 50 задач.
- Глубина ≤ 5.
- У каждой задачи ≥ 1 check.
- depends_on ссылается на id из этого же ответа.
- Первая задача (root) должна иметь depends_on: [].
```

3.2. Пользовательский промпт

```
Задача: <текст от пользователя>

Контекст (опционально):
- Ограничения: <из env PLANNER_CONTEXT>
- Ожидаемый результат: <из env PLANNER_EXPECTED>
```

3.3. Ответ

JSON по схеме выше. Если ответ невалиден — retry с сообщением об ошибке
(до PLANNER_MAX_RETRIES раз).

---

4. Алгоритм

```
1. Прочитать задачу из argv или stdin.
2. Сгенерировать root_id = UUIDv4.
3. Проверить: нет ли уже задач с этим root_id (idempotency).
4. Вызвать LLM с промптом.
5. Спарсить ответ.
6. Валидировать:
   a. JSON schema.
   b. Все depends_on ссылаются на существующие id.
   c. Нет циклов (topological sort).
   d. Лимиты (max_tasks, max_depth, max_children).
7. При ошибке валидации — retry (до PLANNER_MAX_RETRIES).
8. Транзакционно вставить задачи:
   a. add_task для каждой (root первой, остальные с root_id).
   b. add_dep для каждой зависимости.
9. Проверить: promote_ready() — сколько задач стали ready.
10. Вывести: root_id, количество задач, готовых к выполнению.
```

Транзакционность: шаг 8 в одной транзакции. Если вставка упала —
rollback, состояние БД не меняется.

---

5. CLI

```bash
# Из argv
swarm-planner "Сделать рефакторинг модуля auth: разделить на сервисы"

# Из stdin
echo "Задача: ..." | swarm-planner

# С контекстом
PLANNER_CONTEXT="Проект на Rust, использует PostgreSQL" \
PLANNER_EXPECTED="Pull Request с тестами" \
  swarm-planner "Сделать рефакторинг"
```

Вывод:

```
root_id: 5f846c77-e942-42ab-b361-2f7c0dc54e76
tasks created: 7
ready to execute: 1
graph:
  t1: pending  (root)
  t2: pending  depends_on=[t1]
  t3: pending  depends_on=[t1]
  ...
```

---

6. Тесты

6.1. Unit-тесты (crates/swarm-planner/src/lib.rs)

1. parse_valid_json — валидный ответ LLM парсится в граф.
2. parse_invalid_json — невалидный JSON отклоняется.
3. validate_no_cycles — граф с циклом отклоняется.
4. validate_depends_exist — depends_on на несуществующий id
   отклоняется.
5. validate_limits — граф > max_tasks отклоняется.
6. validate_root — несколько корней отклоняется.

6.2. Integration-тесты (tests/test_planner.py)

1. test_planner_creates_graph — Planner на реальном Postgres +
   mock LLM создаёт граф 3 задачи → 2 зависимости.
2. test_planner_idempotent — повторный запуск с тем же root_id не
   создаёт дублей.
3. test_planner_retries_on_invalid — невалидный ответ → retry →
   успех.
4. test_planner_respects_budget — превышение PLANNER_BUDGET_TOKENS
   → ошибка.
5. test_e2e_planner_worker — Planner создал → worker выполнил →
   задача в review.

Mock LLM: в тестах использовать простую программу на Python, которая
возвращает заранее заданный JSON. Не тратить реальные токены.

---

7. Критерий готовности

1. cargo build — 0 errors, 0 warnings.
2. cargo test -p swarm-planner --lib — 6 passed.
3. pytest tests/test_planner.py — 5 passed.
4. Демонстрация: swarm-planner "..." создаёт граф на реальном Postgres.
5. Demo: Planner → 2 воркера → все задачи в review за 5 минут.
6. Документация в docs/PLANNER.md — как пользоваться, какие env,
   какие лимиты.

---

8. Что НЕ делать

· Не использовать tokio::main для всего — только для HTTP-запросов.
  Основная логика синхронная (как в swarm-core).
· Не переходить к swarm-worker с LLM — это v0.3, отдельное ТЗ.
· Не менять schema_v0.sql — граф создаётся через существующие функции.
· Не добавлять новые таблицы — планировщик использует tasks и
  task_deps.

---

9. Формат отчёта

REPORT_v0.2_step2.md:

· Список файлов.
· cargo build лог.
· cargo test логи.
· Демонстрация: пример задачи → граф в БД → cli status.
· Скриншот/лог реального прогона с mock LLM.
· Замечания.
· Если что-то не так — стоп и вопрос.

---

10. Риски

Риск Как mitigate
LLM возвращает невалидный JSON Retry с сообщением об ошибке
LLM создаёт цикл в графе Валидация через topological sort
LLM превышает бюджет Считать токены; abort при превышении
LLM создаёт слишком много задач Лимит max_tasks, проверка после парсинга
Разные модели дают разное качество Промпт с примерами; фиксация seed (если API поддерживает)
Idempotency нарушается Проверка root_id перед вставкой
