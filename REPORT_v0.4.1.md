# Отчёт v0.4.1: строгий Agent Loop и безопасный batch

**Дата:** 2026-10-09  
**Проект:** `swarm-dispatcher`  
**Основание:** два замечания внутреннего ревью после закрытия v0.4.

## Итог

v0.4.1 завершён. Исправлены оба замечания:

1. batch tool calls прерывается на первом отклонённом allowlist-вызове;
2. malformed DSML/JSON больше не проглатывается как допустимый текст.

Дополнительно удалён из рабочего дерева неиспользуемый SQL-черновик.

## 1. Batch reject

В `crates/swarm-worker/src/agent_loop.rs`:

- `run_tool_and_record` возвращает признак `rejected`;
- `Step::ToolCalls` прекращает выполнение сразу после первого policy reject;
- последующие вызовы из того же batch не выполняются;
- причина отказа и номер шага добавляются в историю, которую получает LLM;
- следующий запрос модели может исправить действие отдельным вызовом.

Добавлен integration-тест:

```text
agent_loop_stops_batch_after_reject
```

Тест проверяет одновременно:

- первый допустимый вызов выполнен;
- запрещённый вызов отклонён;
- третий вызов после отказа не выполнен;
- LLM получил `REJECTED:` и сообщение о прекращении batch;
- следующий ответ модели обработан.

## 2. Strict parsing

В `crates/swarm-worker/src/parse_step.rs`:

- malformed quoted DSML string возвращает `InvalidToolCall`;
- truncated quoted string возвращает `InvalidToolCall`;
- `string` должен быть строго `true` или `false`;
- отсутствующие обязательные атрибуты отклоняются;
- повторяющиеся имена параметров отклоняются;
- вложенные `invoke`/`parameter` отклоняются;
- `args` обязан быть JSON-объектом;
- конфликтующие аргументы отклоняются;
- служебный параметр `tool`, который фактически добавляет relay.fast рядом
  с `args`, игнорируется;
- обычный JSON принимается только как полный JSON-объект или fenced JSON;
- произвольный текст с JSON-подобными фрагментами больше не извлекается
  эвристически.

Обратная совместимость сохранена для:

- старого JSON tool-call формата;
- fenced JSON;
- native DeepSeek DSML;
- DSML с отдельными именованными параметрами;
- DSML с JSON-параметром `args`.

Системный prompt Agent Loop дополнен требованием: финальный ответ должен
быть ровно JSON с `done=true`, без обычного текста и Markdown.

Добавлены unit-тесты на malformed и truncated quoted strings; всего unit-тестов
`swarm-worker`: **46**.

## 3. Уборка SQL-черновика

Файл:

```text
step2b/roles_2b_patch_section3.sql
```

перенесён в:

```text
docs/history/obsolete/roles_2b_patch_section3.sql
```

SHA-256 сохранён:

```text
6dff73f212efba97a30d36c7fec7ee0bf2e46be8bff3dd714201d10ac8d6c0b7
```

Файл не применялся Docker Compose и не входил в glob `roles_patch_*.sql`.
Причина переноса добавлена в `docs/history/obsolete/README.md`.

## 4. Проверки

| Проверка | Результат |
|---|---:|
| `cargo test -p swarm-worker --lib --locked` | **46 passed** |
| `cargo test -p swarm-worker --test worker_sandbox --locked -- --test-threads=1` | **11 passed** |
| `cargo build --workspace --locked` | успешно |
| DSML batch reject integration | passed |
| strict malformed DSML tests | passed |
| реальный DeepSeek через relay.fast | passed |

### Реальный Agent Loop после патча

- LLM: `deepseek-v4-flash` через relay.fast;
- 4 итерации;
- 3 tool calls;
- Rust-крейт создан и протестирован внутри Docker sandbox;
- artifact сохранён;
- 3 события `tool_call` записаны в PostgreSQL;
- `verify_chain=true`;
- после прогона sandbox-контейнеры удалены.

Проверка выполнялась в отдельной временной БД `v0sandbox`; после завершения
БД удалена. Рабочая БД `swarm` не удалялась.

## 5. Состояние после v0.4.1

Закрыто:

- v0.4 shell tools + Docker sandbox;
- DSML parser DeepSeek;
- criterion 9;
- batch reject safety;
- strict malformed-input handling;
- архивирование SQL-черновика.

Оставлено осознанно:

- `swarm-planner` в dev-dependencies `swarm-worker` — dev-only e2e-тест;
- нативный OpenAI `tools` API — следующий этап, v0.4.1 не требует;
- нагрузочный тест — следующий шаг;
- git worktree — v0.5.

## 6. Артефакты

- `crates/swarm-worker/src/parse_step.rs`
- `crates/swarm-worker/src/agent_loop.rs`
- `crates/swarm-worker/src/prompt.rs`
- `crates/swarm-worker/tests/worker_sandbox.rs`
- `docs/history/obsolete/roles_2b_patch_section3.sql`
- `docs/history/obsolete/README.md`
- `docs/VERIFICATION_LOG.md`
- `CHANGELOG.md`
