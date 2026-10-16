# ТЗ: Planner (шаг 2 фазы v0.2)

Планировщик: LLM-агент, который получает большую задачу текстом и
создаёт граф контрактов через `add_task`/`add_dep` в одной транзакции.

Начинать после приёмки step 2b. Читать целиком.

## 0. Неприкосновенно

- `schema/schema_v0.sql`, `step2/roles.sql`, `step2b/roles_2b.sql` —
  не менять.
- `tests/test_v0.py` (26), `step2/test_roles.py` (30),
  `step2b/test_roles_2b.py` (19) — не менять.
- `crates/swarm-core/` — только расширение (см. §5), без изменения
  существующих методов.

## 1. Что делает Planner

**Вход:** текстовая задача (`"Сделать рефакторинг модуля auth"`).

**Выход:** граф задач в БД. Корень + подзадачи с зависимостями.
Каждая задача — контракт с `kind`, `checks` (непустой), `needs`, `budget`.

**Ограничения (жёсткие):**
- Не более `max_tasks` (по умолчанию 50) задач в графе.
- Глубина ≤ `max_depth` (по умолчанию 5).
- Ширина ≤ `max_children` (по умолчанию 10).
- Идемпотентность: повторный запуск с тем же текстом задачи не создаёт
  дублей.

## 2. Архитектура

### 2.1. Новый крейт `swarm-planner`

```

crates/swarm-planner/
├── Cargo.toml
└── src/
├── main.rs       # CLI: swarm-planner "задача"
├── lib.rs        # Planner struct
├── llm.rs        # LLM-клиент (HTTP)
├── prompt.rs     # системный промпт, few-shot
└── parse.rs      # парсинг ответа LLM → граф

```

### 2.2. Зависимости

```toml
[dependencies]
swarm-core = { path = "../swarm-core" }
reqwest = { version = "0.12", features = ["json", "blocking", "rustls-tls"] }
serde = { workspace = true }
serde_json = { workspace = true }
sha2 = "0.10"
hex = "0.4"
thiserror = { workspace = true }
tracing = { workspace = true }
tracing-subscriber = { workspace = true }
clap = { workspace = true }
```

Sync, не async. reqwest::blocking — потому что swarm-core
синхронный, tokio добавлять не надо.

2.3. Env

Переменная Обязательно Default
DATABASE_URL да —
PLANNER_LLM_PROVIDER да openai / anthropic / openai-compatible
PLANNER_LLM_API_KEY да (кроме openai-compatible без auth) —
PLANNER_LLM_MODEL да —
PLANNER_LLM_BASE_URL для openai-compatible —
PLANNER_MAX_TASKS нет 50
PLANNER_MAX_DEPTH нет 5
PLANNER_MAX_CHILDREN нет 10
PLANNER_MAX_RETRIES нет 2
PLANNER_BUDGET_TOKENS нет 50000

3. Детерминированный root_id

Ключевое отличие от v1 ТЗ.

```
root_id = UUIDv5(
    namespace = "6ba7b810-9dad-11d1-80b4-00c04fd430c8",  # DNS namespace, RFC 4122
    name      = sha256_hex(user_task_text)
)
```

Или проще: root_id = sha256_hex(user_task_text)[0..32] в формате UUID.

Причина: повторный запуск на том же тексте даёт тот же root_id →
add_task падает на PK → Planner пропускает с сообщением
"graph already exists (root_id=..., tasks=N)".

Идемпотентность реализуется через PK, не через IF NOT EXISTS.
Это гарантия БД, не логики приложения.

Полный текст задачи сохраняется в событиях: первый created event
для корня получает payload = {"description": user_task_text}.

4. Контракт корня с явными лимитами

При создании корня Planner формирует контракт:

```json
{
  "kind": "root",
  "description": "<полный текст задачи>",
  "checks": [{"type": "cmd", "run": "true"}],
  "needs": {},
  "budget": {
    "attempts": 3,
    "max_rework": 2,
    "max_depth": 5,
    "max_children": 10,
    "max_tasks": 50
  }
}
```

Лимиты (max_depth, max_children, max_tasks) — из env, попадают в
budget корня явно. SQL читает их оттуда (c_num(contract, ...),
умолчания 10/20/200). Без явных значений Planner-овские лимиты не
применятся.

5. Расширение swarm-core

Единственное расширение: SwarmDispatcher уже имеет
with_transaction (step 1). Planner использует его.

Если окажется, что в DispatcherTx не хватает метода — не расширять
напрямую, а описать в отчёте. Возможные кандидаты:

· add_dep (уже есть по ТЗ step 1).
· add_task (уже есть).
· promote_ready (уже есть).

Больше ничего не нужно. Если Planner использует что-то ещё — это ошибка
дизайна, стоп и вопрос автору.

6. Контракт LLM

6.1. Системный промпт (в prompt.rs)

```
Ты — планировщик задач. Получаешь большую задачу и разбиваешь её на граф
подзадач (контрактов). Ответ должен быть валидным JSON.

Схема:
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
    }
  ]
}

Правила:
- Не более {max_tasks} задач.
- Глубина ≤ {max_depth}.
- Ширина ≤ {max_children}.
- У каждой задачи ≥ 1 check.
- depends_on ссылается на id из этого же ответа.
- Первая задача (root) имеет depends_on: [].
```

Параметры из env подставляются в промпт. Это фиксирует лимиты на
уровне LLM, не только на уровне пост-валидации.

6.2. Провайдеры

· openai: POST {base}/v1/chat/completions, модель по env.
· anthropic: POST {base}/v1/messages, x-api-key, anthropic-version.
· openai-compatible: то же что openai, но base_url задаётся.
· mock (для тестов): читает заранее заданный JSON из
  PLANNER_LLM_MOCK_RESPONSE_FILE. Без HTTP.

7. Алгоритм

```
1. Прочитать задачу из argv или stdin.
2. root_id = deterministic_uuid(text).
3. Проверить существование root_id: SELECT 1 FROM tasks WHERE id=root_id.
   Если существует — вывести "graph exists (root_id=..., N tasks)" и выход 0.
4. Сформировать промпт с параметрами из env.
5. Вызвать LLM.
6. Спарсить JSON. При ошибке — retry (до PLANNER_MAX_RETRIES) с
   указанием на ошибку в промпте.
7. Валидировать локально:
   a. JSON schema.
   b. tasks count ≤ max_tasks.
   c. depth ≤ max_depth (топсортировка графа).
   d. children per node ≤ max_children.
   e. нет циклов.
   f. все depends_on ссылаются на существующие id.
   g. ровно один корень (depends_on: []).
   h. у каждой задачи непустой checks.
8. В одной транзакции (dispatcher.with_transaction):
   - add_task для корня (контракт с лимитами).
   - add_task для каждой подзадачи.
   - add_dep для каждой зависимости.
   - promote_ready().
9. Вывести: root_id, tasks created, ready to execute.
```

Валидация до транзакции. Не открывать транзакцию до того, как
LLM-ответ проверен полностью.

8. CLI

```bash
# Из argv
swarm-planner "Сделать рефакторинг модуля auth"

# Из stdin
echo "..." | swarm-planner

# С контекстом
PLANNER_CONTEXT="Rust, PostgreSQL" \
PLANNER_EXPECTED="Pull Request с тестами" \
  swarm-planner "Рефакторинг"
```

Вывод:

```
root_id: a4f89d012e873b2c15f9e8a9d10e527
tasks created: 7
ready to execute: 1
```

9. Тесты

9.1. Unit (crates/swarm-planner/src/lib.rs)

Используют mock provider (без HTTP).

1. parse_valid_json — валидный ответ парсится.
2. parse_invalid_json — невалидный → ошибка.
3. validate_no_cycles — граф с циклом отвергается.
4. validate_depends_exist — ссылка на несуществующий id отвергается.
5. validate_limits — граф > max_tasks отвергается.
6. validate_single_root — два корня отвергаются.
7. deterministic_root_id — одинаковый текст → одинаковый root_id.
8. deterministic_root_id_differs — разные тексты → разные root_id.

9.2. Integration (crates/swarm-planner/tests/planner_integration.rs)

Реальный Postgres, mock LLM.

1. planner_creates_graph — Planner создаёт 3 задачи, 2 зависимости.
2. planner_idempotent_same_text — второй запуск с тем же текстом
   не создаёт дублей.
3. planner_idempotent_different_text — другой текст → другой граф.
4. planner_atomic_on_sql_error — если 3-я задача падает на add_dep,
   первые 2 не остаются в БД.
5. planner_respects_max_tasks — LLM вернул 100 задач при лимите 50 →
   локальная валидация отвергает.
6. planner_retries_on_invalid_json — невалидный ответ → retry → успех.
7. e2e_planner_to_review — Planner → worker выполняет → review.

9.3. Mock LLM

В тестах — PLANNER_LLM_PROVIDER=mock, файл с ответом через env.
Файл-заготовка в crates/swarm-planner/tests/fixtures/planner_ok.json.

10. Мутации (в отчёт)

· M1: root_id = Uuid::new_v4() (случайный) → падает
  deterministic_root_id, planner_idempotent_same_text.
· M2: убрать лимиты из контракта корня → падает
  planner_respects_max_tasks (SQL подставит 200 вместо 50).
· M3: with_transaction → последовательные add_task без транзакции →
  падает planner_atomic_on_sql_error.
· M4: убрать проверку single root → падает validate_single_root.

Результат — в отчёт. Если хоть одна не ловится — провал.

11. Критерии готовности

1. cargo build --workspace --locked — 0 errors, 0 warnings.
2. cargo test --workspace --lib — все unit, в отчёте список имён.
3. cargo test -p swarm-planner --test planner_integration
      -- --test-threads=1 — 7 passed, список имён.
4. pytest tests/test_v0.py — 26 passed (регрессия).
5. pytest step2/test_roles.py — 30 passed.
6. pytest step2b/test_roles_2b.py — 19 passed.
7. Мутации M1–M4 — 4 из 4 ловятся, с указанием упавших тестов.
8. Демонстрация на реальном Postgres с mock: 
   swarm-planner "test task" создаёт граф, swarm-cli status его
   показывает.
9. Docker/compose — если доступен sudo; иначе отметить как отложенный
   (батч с step 1/2/2b).

12. Не делать

· Async/tokio в Planner. Sync.
· Менять swarm-core public API. Использовать with_transaction.
· Менять схему. root_id, description, лимиты — через contract.
· Расширять roles.sql. Planner использует роль swarm_planner, она
  уже имеет add_task, add_dep, promote_ready.
· Запускать реальный LLM в тестах. Только mock.

13. Формат отчёта

REPORT_v0.2_step2_planner.md:

· Список файлов.
· git diff.
· Полный вывод критериев 1–9.
· Результаты мутаций M1–M4.
· «Что не сработало / не проверено».
· Стоп и вопрос автору, если что-то не так.

Не переходить к v0.3 (LLM-worker) без приёмки.

