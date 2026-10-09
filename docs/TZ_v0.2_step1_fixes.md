# ТЗ: правки v0.1 (шаг 1 фазы v0.2)

Версия 2. Учитывает находки Claude (`DispatcherTx<'a,'b>`, `Lease.contract`
как сырой JSON, отказ от `#[from]`) и замечания OpenClaw (`--ignored`
ненадёжен, дайджест через `buildx imagetools`).

**Начинать только после полного прочтения.** Если что-то противоречит
`docs/ARCHITECTURE.md` или `docs/METHODOLOGY.md` — стоп и вопрос автору.

---

## 0. Неприкосновенно

- `schema/schema_v0.sql` — не менять.
- `step2/roles.sql`, `step2/test_roles.py` — не менять (проверены).
- `tests/test_v0.py` — 26 тестов, не менять.
- `crates/swarm-core/tests/integration.rs` — существующие тесты, только
  добавлять (см. §8).
- Любой проходящий тест — не менять.

---

## 1. `Cargo.toml` (workspace root)

### 1.1. MSRV

```toml
[workspace.package]
rust-version = "1.85"
```

Причина: транзитивная block-buffer 0.12 требует edition2024.

1.2. Feature-флаги postgres

```toml
[workspace.dependencies]
postgres = { version = "0.19", features = [
    "with-uuid-1",
    "with-serde_json-1",
] }
```

Причина: без with-uuid-1 Uuid не реализует ToSql/FromSql; без
with-serde_json-1 — serde_json::Value.

1.3. Cargo.lock

· Коммитить Cargo.lock в репозиторий.
· Все сборки — cargo build --locked.
· Dockerfile — cargo build --release --locked --bins.

---

2. errors.rs

2.1. НЕ использовать #[from] для postgres::Error

```diff
-    Database(#[from] postgres::Error),
+    Database(#[source] postgres::Error),
```

Причина: #[from] создаёт автоматическую конверсию через ?, и любой
будущий ? молча превратит LS001 LeaseLost в Database. Маппинг
SQLSTATE будет обойдён.

2.2. Правило в коде

Любой ? на postgres::Error — только через явный map_err:

```rust
// ❌ НЕПРАВИЛЬНО
let client = Client::connect(dsn, NoTls)?;

// ✅ ПРАВИЛЬНО
let client = Client::connect(dsn, NoTls)
    .map_err(DispatcherError::from_pg)?;

self.client
    .query_one("SELECT ...", &[...])
    .map_err(DispatcherError::from_pg)?;
```

Добавить комментарий в начало errors.rs с этим правилом.

2.3. Тест sqlstate_known_codes_map

Переписать на проверку варианта, не текста:

```rust
use DispatcherError::*;
let cases: [(&str, fn(&DispatcherError) -> bool); 18] = [
    ("LS001", |e| matches!(e, LeaseLost)),
    ("AR001", |e| matches!(e, ArtifactMissing)),
    // ... все 18
];
for (code, is_expected) in cases {
    let e = DispatcherError::from_sqlstate(code)
        .unwrap_or_else(|| panic!("sqlstate {code} not mapped"));
    assert!(is_expected(&e), "sqlstate {code} mapped to wrong variant: {e:?}");
}
```

---

3. types.rs

3.1. Lease.contract — сырой JSON

Критичный баг: SQL принимает контракт без kind или с дробным
budget.attempts; Rust — нет. Если claim десериализует после выдачи
аренды, задача зависает в leased на весь срок аренды, потом reclaim
сжигает попытку. Через 3 попытки — blocked.

Правильно:

```rust
pub struct Lease {
    pub task_id: Uuid,
    pub contract: serde_json::Value,   // ✅ сырой JSON
    pub epoch: i64,
    pub budget_cap: i64,
    pub spent_tokens: i64,
    pub worker_id: String,
}

impl Lease {
    /// Типизированный разбор контракта. Не влияет на аренду.
    pub fn typed_contract(&self) -> Result<TaskContract, DispatcherError> {
        serde_json::from_value(self.contract.clone()).map_err(Into::into)
    }
}
```

В dispatcher.rs::claim — никакого from_value. Только r.get(1).

---

4. dispatcher.rs

4.1. Все ? на postgres — через map_err

См. §2.2.

4.2. DispatcherTx<'a, 'b> — рабочий код

Раздел 5.1 предыдущей версии ТЗ не компилируется
(Transaction<'a> { tx: &'a mut postgres::Transaction<'a> } даёт E0597 +
E0505). Рабочий вариант — из файла dispatcher.rs Claude:

```rust
pub struct DispatcherTx<'a, 'b> {
    tx: &'a mut postgres::Transaction<'b>,
}

impl SwarmDispatcher {
    pub fn with_transaction<F, T>(&mut self, f: F) -> Result<T, DispatcherError>
    where
        F: FnOnce(&mut DispatcherTx<'_, '_>) -> Result<T, DispatcherError>,
    {
        let mut tx = self.client.transaction().map_err(DispatcherError::from_pg)?;
        let out = {
            let mut w = DispatcherTx { tx: &mut tx };
            f(&mut w)?   // Err → tx дропается без commit → rollback
        };
        tx.commit().map_err(DispatcherError::from_pg)?;
        Ok(out)
    }
}

impl DispatcherTx<'_, '_> {
    pub fn add_task(&mut self, task_id: Uuid, contract: &TaskContract,
                    root_id: Option<Uuid>, budget_cap: i64, priority: i32)
        -> Result<(), DispatcherError> { /* ... */ }

    pub fn add_dep(&mut self, task_id: Uuid, dep_id: Uuid)
        -> Result<(), DispatcherError> { /* ... */ }

    pub fn promote_ready(&mut self) -> Result<i32, DispatcherError> { /* ... */ }
}
```

Точный список методов DispatcherTx: add_task, add_dep,
promote_ready. Больше ничего. Planner использует только эти.

4.3. swarm-cli status через task_status()

swarm-cli сейчас делает SELECT FROM tasks — под ролями это не
сработает. roles.sql предоставляет функцию task_status(n), доступную
только роли swarm_operator (и swarm_cli как её член).

Заменить:

```rust
// ❌ БЫЛО
"SELECT id, root_id, state, lease_owner, lease_epoch, attempts, rework, priority, block_reason \
 FROM tasks ORDER BY ..."

// ✅ СТАЛО
"SELECT id, root_id, state, lease_epoch, lease_owner, attempts, rework, block_reason, updated_at \
 FROM task_status($1)"
```

list_tasks() в swarm-core — соответствующим образом.

---

5. Dockerfile

5.1. Образ — дайджест

```dockerfile
# Требуется Rust 1.85+ (edition2024 в block-buffer 0.12).
FROM rust:1.98-slim@sha256:<digest> AS builder
```

Дайджест получить:

```bash
docker buildx imagetools inspect rust:1.98-slim | head -3
# Верхняя строка `Digest: sha256:...` — индекс, вписать в FROM.
```

НЕ использовать docker manifest inspect | grep digest — покажет
дайджесты платформенных манифестов, не индекса.

5.2. Убрать COPY schema

Если в текущем Dockerfile есть COPY --chmod=644 schema/... — убрать.
Схема монтируется в контейнер Postgres через volume (см. §6).

5.3. Сборка с --locked

```dockerfile
RUN cargo build --release --locked --bins
```

---

6. docker-compose.yml

6.1. Схема и роли — в Postgres

```yaml
postgres:
  image: postgres:16.4-alpine@sha256:<digest>
  volumes:
    - postgres_data:/var/lib/postgresql/data
    - ./schema/schema_v0.sql:/docker-entrypoint-initdb.d/01-schema.sql:ro
    - ./step2/roles.sql:/sql/roles.sql:ro
    - ./scripts/02-roles.sh:/docker-entrypoint-initdb.d/02-roles.sh:ro
```

02-roles.sh — вызывается после 01-schema.sql, применяет roles.sql
с паролями из env. Задача shell-скрипта — отдельная; если ещё нет,
в step 1 достаточно заготовки, полностью включим в step 2 docker-проверке.

6.2. Убрать version: "3.9"

Deprecated.

6.3. CLI без command

```yaml
cli:
  build: .
  # command НЕ указывать — иначе compose подменяет entrypoint.
  profiles: ["tools"]
```

Все вызовы CLI: docker compose run --rm --no-deps cli swarm-cli <cmd>.

---

7. Документация — консолидация

7.1. Оставить в docs/ пять файлов

· PROJECT_STATE.md
· ARCHITECTURE.md
· KNOWN_LIMITATIONS.md
· VERIFICATION_LOG.md
· TZ_v0.2_step1_fixes.md (этот файл)

7.2. Остальные — в docs/optional/

Ничего не удалять, но переместить: PLATFORM_COMPARISON.md,
THREAT_MODEL.md, BRIEF_FOR_AGENTS.md, PLAIN_EXPLANATION.md,
ROADMAP.md, NEXT_STEPS.md, ERRORS.md, DOC_STRATEGY.md,
NATIVE_SETUP.md, ARCHITECTURE_DECISIONS.md, AUDIT_HISTORY.md,
CLAUDE_DIALOGUE.md, REPORT_TEMPLATE.md, TZ_v0.2_step2_roles.md,
TZ_v0.2_step2_planner.md, RUST_FINDINGS.md.

7.3. Один источник чисел

«26 тестов», «19 функций», «6 таблиц» — только в README.md. Остальные
файлы ссылаются, не дублируют.

7.4. Формулировки

7.4.1. Везде non-repudiation → tamper-evidence. Non-repudiation
требует WORM / третьей стороны — v2.0.

7.4.2. В KNOWN_LIMITATIONS.md раздел 3.2: убрать «10 задач/с, запас
100×» — не измерено. Заменить: «не измерено, нагрузочный тест — v1.0».

7.5. README

Обновить:

1. Требования: Rust 1.85+, cargo build --locked.
2. Команды: docker compose run --rm --no-deps cli swarm-cli seed.
3. Ссылка на docs/NATIVE_SETUP.md.
4. Ссылка на docs/KNOWN_LIMITATIONS.md вместо дублирования.
5. Таблица ролей (короткая — 5 строк).

---

8. Тесты

8.1. В integration.rs — добавить 5 новых

Из integration_verified.rs Claude:

1. contract_accepted_by_sql_but_not_by_rust_must_not_strand_the_task
   — регрессия на баг из §3.1.
2. concurrent_claims_are_disjoint — 8 воркеров, 24 задачи.
3. with_transaction_rolls_back_on_error — локальная ошибка.
4. with_transaction_rolls_back_on_sql_error_midway — SQL-ошибка
   посреди графа.
5. with_transaction_commits_whole_graph_atomically — атомарный коммит.

Остальные 3 теста из integration_verified.rs — в
tests/alt/integration_alt.rs как альтернативные формулировки
(full_cycle, sqlstate_mapping, zombie_is_fenced).

8.2. Мутационное тестирование (в отчёт)

Проверить, что 5 новых тестов ловят намеренную поломку:

№ Мутация Какие тесты должны упасть
М1 В claim вернуть serde_json::from_value(...).unwrap() contract_accepted...
М2 В with_transaction заменить f(&mut w)? на let _ = f(&mut w); (коммит даже при ошибке) with_transaction_rolls_back_on_error
М3 В Lease::typed_contract — не использовать (заменить на Ok(Default::default())) full_cycle (в tests/alt/)

Результат мутаций — в отчёт. Если хоть одна не ловится — провал, стоп
и вопрос автору.

---

9. Критерии готовности step 1

Все пункты — с полным выводом, не «прошло».

1. cargo build --workspace --locked — 0 errors, 0 warnings.
2. cargo test --workspace --lib — итог с числом passed; список имён тестов.
3. cargo test -p swarm-core --test integration -- --test-threads=1 —
   список имён тестов + итог (например, 10 passed).
4. pytest tests/test_v0.py — 26 passed.
5. pytest step2/test_roles.py — 30 passed (роли не сломаны).
6. Мутации (§8.2) — 3 из 3 ловятся, с указанием какие тесты упали.
7. docker compose up -d --build — 4 контейнера healthy.
8. docker compose run --rm --no-deps cli swarm-cli seed — создаёт
   задачи, status выводит таблицу.
9. End-to-end: seed → падение воркера (CRASH_AFTER_CLAIM=1) →
   {"reclaimed":1} в логе watchdog → второй воркер подхватил →
   review → cli review ... approve → done.

Про --ignored: команда cargo test -- --ignored запускает только
тесты с меткой #[ignore]. Не использовать как критерий «всё
прошло». Требовать в отчёте список имён тестов и итог.

---

10. Формат отчёта

Один файл REPORT_v0.2_step1.md:

· Список изменённых файлов.
· git diff по каждому (или общий).
· Полный вывод критериев 1–9.
· Отдельно: результаты мутаций (§8.2).
· Отдельно: «что не сработало или не проверено».
· Если что-то не так — стоп, вопрос автору.

Не переходить к следующему шагу без приёмки.

---

11. Что НЕ делать

· Не менять schema_v0.sql, step2/roles.sql, step2/test_roles.py.
· Не использовать #[from] postgres::Error.
· Не добавлять tokio — Rust-инфраструктура синхронная.
· Не добавлять новые крейты.
· Не переносить начало Planner — сначала step 1, потом step 2b.

