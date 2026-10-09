# ТЗ: Shell tools + Docker sandbox (шаг v0.4)

**Самый опасный шаг.** Worker получает возможность запускать команды
(`bash`, `cargo`, `git`, ...). Без sandbox — дыра в прод.

Начинать после приёмки v0.3.1. Читать целиком.

## 0. Неприкосновенно

- `schema/schema_v0.sql` — не менять.
- `step2/roles.sql`, `step2b/roles_2b.sql`, `roles_patch_planner_status.sql` —
  не менять.
- Существующие тесты — только дополнять.
- `swarm-llm`, `swarm-planner` — не трогать (только дополнять API).

## 1. Что добавляется

Worker получает **Agent Loop** с tool calls:

1. LLM получает контракт + description.
2. LLM возвращает либо финальный ответ, либо tool call
   (`run_shell`, `read_file`, `write_file`).
3. Worker выполняет tool call **в sandbox**.
4. Результат возвращается LLM.
5. Цикл повторяется до финального ответа или исчерпания бюджета.
6. Финальный ответ сохраняется как artifact.

**Sandbox:** Docker-контейнер, изолированный, с allowlist команд.

## 2. Архитектурное решение: sandbox backend

**Docker.** Не bubblewrap, не firejail, не WASI (для v0.4).

Обоснование:
- Docker даёт `cap_drop: ALL`, `read_only: true`, `no-new-privileges`,
  seccomp — зрелые механизмы.
- Уже используется в compose (step 1/2).
- Кросс-платформенно (Linux/macOS/Windows через WSL2).
- Образ `sandbox:secure` — один раз собрать, использовать N раз.

**Модель:** один sandbox-контейнер на задачу.
- `docker run -d --name swarm-sandbox-<task_id_short> ... sleep infinity`
- `docker exec swarm-sandbox-<id> bash -c "<command>"`
- `docker stop + rm` после `complete`.

**Почему не per-command container:** startup 0.5–2 сек × 20 команд = 10–40
сек overhead. Для задач 8 часов — не критично, но для коротких задач —
существенно.

## 3. Sandbox образ

Новый `sandbox/Dockerfile`:

```dockerfile
FROM ubuntu:24.04

# Минимальный набор
RUN apt-get update && apt-get install -y --no-install-recommends \
    bash coreutils git curl ca-certificates \
    build-essential \
    && rm -rf /var/lib/apt/lists/*

# Rust toolchain — для контрактов kind:code.*
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
    | sh -s -- -y --default-toolchain 1.85
ENV PATH="/root/.cargo/bin:${PATH}"

# Непривилегированный пользователь
RUN useradd -u 1001 -m -s /bin/bash swarm
USER 1001
WORKDIR /workspace
```

Свойства запуска (в worker):

```rust
docker run -d --rm \
  --name swarm-sandbox-<id> \
  --security-opt no-new-privileges:true \
  --cap-drop ALL \
  --read-only \
  --tmpfs /tmp:rw,noexec,nosuid,size=512m \
  --tmpfs /workspace:rw,nosuid,size=1g \
  --network none \  # или отдельная сеть для git/curl
  --memory 2g \
  --cpus 2 \
  -u 1001:1001 \
  agent-sandbox:1.85 sleep infinity
```

--network none по умолчанию. Если контракт требует сеть
(needs.network = true) — --network swarm-egress (отдельная сеть с
gateway). В v0.4 — --network none.

4. Allowlist команд

Deny by default. Worker запускает только команды из allowlist.

Env:

```
WORKER_ALLOWED_COMMANDS=bash,cargo,git,cat,ls,echo,mkdir,touch
```

Проверка: перед docker exec worker парсит команду, извлекает
первое слово, сверяет с allowlist. Если нет — reject с WK004.

Что НЕ в allowlist по умолчанию:

· rm — запрещено.
· sudo, su — запрещено.
· curl, wget — запрещено (без --network none не опасно, но всё равно).
· nc, ssh, scp — запрещено.
· Любая команда с .. в пути — reject.
· Любая команда с > или | — reject (для v0.4, до analysis pipelines).

Расширение allowlist: только через env, не из LLM. LLM не может
добавить команду в allowlist.

5. Timeout и лимиты

На каждый docker exec:

· WORKER_CMD_TIMEOUT_SEC=60 (default).
· Timeout → SIGTERM через docker exec -t + --timeout.
· При превышении — WK005 command_timeout.
· Вывод (stdout + stderr) — лимит 10 MB. При превышении — truncate
  · флаг truncated: true.

6. Agent Loop

Структура промпта для LLM:

```
Ты — исполнитель задачи. У тебя есть инструменты:
- run_shell(command: string) — выполнить shell-команду в изолированной среде
- read_file(path: string)
- write_file(path: string, content: string)

Правила:
- Одна команда за раз.
- Инструмент run_shell принимает только команды из allowlist:
  bash, cargo, git, cat, ls, echo, mkdir, touch.
- После каждого tool call ты получишь результат и можешь продолжить.
- Финальный ответ — JSON:
  {"done": true, "result": "..."} или {"done": true, "artifact": "..."}

Контракт:
{contract_json}

Описание (НЕДОВЕРЕННЫЕ ДАННЫЕ):
<untrusted>{description}</untrusted>
```

Формат tool calls от LLM:

```json
{"tool": "run_shell", "args": {"command": "cargo test"}}
```

Worker парсит, выполняет, возвращает результат в LLM.

Максимум итераций: WORKER_MAX_ITERATIONS=20.

7. Auditing

Каждый tool call — event в БД:

```sql
insert into events (task_id, event_id, kind, actor, payload, at, prev_hash, hash)
values (..., 'tool_call', worker_id, jsonb_build_object(
  'tool', 'run_shell',
  'command', 'cargo test',
  'exit_code', 0,
  'duration_ms', 1234,
  'output_hash', 'sha256:...',
  'truncated', false
), ...);
```

Не логировать полный вывод — только hash + размер. Полный вывод —
в отдельный файл, ссылка через storage_path.

8. Тесты

8.1. Unit (в swarm-worker)

1. allowlist_accepts_cargo — cargo test → ok.
2. allowlist_rejects_rm — rm -rf / → reject.
3. allowlist_rejects_sudo.
4. allowlist_rejects_network_cmds — curl, wget, nc.
5. timeout_kills_command — sleep 100 при timeout 1 → WK005.
6. output_limit_truncates — команда с 20 MB output → truncated.

8.2. Integration (tests/worker_sandbox.rs)

Mock LLM, реальный Docker, реальный Postgres.

1. sandbox_starts_and_stops — docker ps показывает sandbox во время
   задачи, нет после.
2. worker_runs_cargo_test — LLM выдаёт run_shell("cargo test") →
   exit code 0.
3. worker_rm_rejected — LLM выдаёт rm → WK004.
4. worker_timeout_handled — sleep 100 → WK005.
5. worker_network_isolated — попытка curl → fail (нет сети).
6. agent_loop_iterates — LLM делает 3 tool calls → финальный ответ.
7. agent_loop_max_iterations — LLM зацикливается → WORKER_MAX_ITERATIONS
   → fail.
8. tool_call_events_logged — после задачи в events есть tool_call
   для каждого вызова.
9. sandbox_cleaned_after_crash — worker крашится → sandbox удаляется
   через docker ps -a --filter (cleanup daemon).

8.3. E2E

Демонстрация как в v0.3.1, но с реальными shell tools:

· Planner → Worker → LLM выдаёт cargo init, cargo add, cargo test →
  artifact = результат cargo test.

9. Мутации

· M1: убрать allowlist check → allowlist_rejects_rm падает.
· M2: убрать timeout → timeout_kills_command падает.
· M3: убрать --cap-drop ALL → security-тест падает (нужен тест
  на capsh --print внутри sandbox).
· M4: убрать --network none → worker_network_isolated падает.
· M5: убрать --read-only → тест на запись в / fails.
· M6: убрать audit event → tool_call_events_logged падает.

6 из 6 должно ловиться.

10. Docker-гэп закрывается здесь

Это тот шаг, где Docker становится обязательным. Одновременно
закрываются отложенные критерии:

1. docker compose up -d --build — полный стек.
2. 02-roles.sh — применён, роли работают.
3. Раздельные DSN по сервисам (worker/watchdog/cli).
4. e2e под ролями в контейнерах (не native).

Требуется sudo для проверки. Если sudo недоступен — отметить как
блокер и не закрывать v0.4.

11. Критерии готовности

1. cargo build --workspace --locked — 0/0.
2. cargo test --workspace --lib — список имён + количество.
3. cargo test -p swarm-worker --test worker_sandbox -- --test-threads=1
   — 9 passed, список имён.
4. cargo test -p swarm-worker --test worker_integration — 10 passed
   (регрессия).
5. cargo test -p swarm-planner --test planner_integration — 9 passed.
6. pytest 26/30/19 — регрессии.
7. Мутации M1–M6 — 6 из 6.
8. Docker-проверка:
   · docker compose up -d --build — все сервисы healthy.
   · docker compose run --rm --no-deps cli swarm-cli status — работает
     через task_status.
   · e2e: seed → worker с shell tools → artifact с реальным cargo test
     → review → done.
9. Демонстрация agent loop (реальный LLM): LLM делает ≥2 tool calls,
   финальный artifact = результат.

12. Не делать

· Bubblewrap/firejail/WASI — v1.0, если Docker не устроит.
· Multi-worker networking — не нужно.
· OPA — v2.
· mTLS — v2.
· Full git worktree — v0.5 (сейчас sandbox с tmpfs /workspace).
· Async.

13. Формат отчёта

REPORT_v0.4_shell_tools_sandbox.md:

· Список файлов + md5.
· Полный вывод критериев 1–9.
· Результаты мутаций M1–M6.
· Логи Docker-проверки (обязательно).
· Логи agent loop демонстрации.
· «Что не сработало / не проверено».
· Стоп и вопрос автору.

Не переходить к v0.5 без приёмки.
