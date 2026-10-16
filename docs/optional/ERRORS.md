# Коды ошибок

Каждая ошибка диспетчера имеет уникальный код формата `XYNNN`:
- `LS` — Lease (аренда).
- `AR` — Artifact (артефакт).
- `CT` — Contract (контракт).
- `GR` — Graph (граф).
- `RV` — Review (ревью).
- `WK` — Worker (воркер).

Коды задаются в `schema_v0.sql` через `errcode` в `RAISE EXCEPTION`.
Маппинг в Rust — в `crates/swarm-core/src/errors.rs`.

---

## Полная таблица

| Код | Rust-вариант | Когда возникает | Что делать |
|-----|--------------|-----------------|------------|
| **LS001** | `LeaseLost` | Запись с чужим, устаревшим или истёкшим `lease_epoch` | Воркер потерял аренду; взять следующую задачу |
| **AR001** | `ArtifactMissing` | `complete` с `result_ref`, которого нет в `artifacts` | Сначала вызвать `save_artifact` |
| **CT001** | `ContractInvalid` | Нет `checks` или нечисловое поле в `needs`/`budget` | Проверить контракт; `checks` обязателен, непустой |
| **GR000** | `TaskNotFound` | Задача не существует | Проверить `task_id` |
| **GR001** | `CycleDetected` | Ребро создаёт цикл (включая самозависимость) | Проверить граф |
| **GR002** | `DepthExceeded` | Цепочка длиннее `budget.max_depth` | Поднять лимит или разбить граф |
| **GR003** | `FanoutExceeded` | У задачи больше `budget.max_children` зависимых | Поднять лимит |
| **GR004** | `TaskLimitExceeded` | Больше `budget.max_tasks` в графе | Поднять лимит |
| **GR005** | `NotDone` | `reopen_task` не для `done` | Проверить состояние |
| **GR006** | `RootNotFound` | Корень графа не существует | Проверить `root_id` |
| **GR007** | `CrossRootDependency` | Зависимость между разными графами | Задачи должны быть в одном графе |
| **GR008** | `TaskNotPending` | `add_dep` для задачи не в `pending` | Добавлять рёбра только к `pending` |
| **GR009** | `NotBlocked` | `unblock_task` не для `blocked` | Проверить состояние |
| **RV001** | `ReviewStale` | Ревью не того артефакта или задача не в `review` | Проверить `artifact_ref` и состояние |
| **RV002** | `SelfReview` | Рецензент = автор артефакта | Привлечь другого рецензента |
| **RV003** | `ChecksFailed` | `approved` без полного набора пройденных проверок | Запустить все `checks` из контракта |
| **WK001** | `WorkerInvalid` | Неверные `capabilities` воркера | Проверить JSON: `ram_gb`, `models.coder_class` — числа |
| **WK002** | `WorkerUnknown` | Воркер не зарегистрирован | Вызвать `register_worker` перед `claim` |

---

## Как пользоваться

### В Rust

```rust
match d.claim("worker-1", 300) {
    Ok(Some(lease)) => { /* работаем */ }
    Ok(None) => { /* нет задач, ждём */ }
    Err(DispatcherError::WorkerUnknown) => {
        d.register_worker("worker-1", &caps, &[])?;
    }
    Err(DispatcherError::LeaseLost) => {
        // Аренда потеряна, перезапускаем
    }
    Err(e) => return Err(e),
}
```

В SQL

```sql
DO $$
BEGIN
    PERFORM claim_task('worker-1', 300);
EXCEPTION
    WHEN SQLSTATE 'WK002' THEN
        RAISE NOTICE 'Worker not registered, registering...';
        PERFORM register_worker('worker-1', '{"ram_gb":16}'::jsonb);
END $$;
```

В логах

При ошибке SQLSTATE попадает в лог через tracing:

```
{"level":"ERROR","error":"lease lost or expired","code":"LS001","task_id":"...","epoch":1}
```

Код в поле code — для алертов и мониторинга.

---

Что делать при неизвестном коде

Если вы получили SQLSTATE, которого нет в таблице:

1. Это баг. Код должен быть задокументирован.
2. Проверьте schema_v0.sql: какой RAISE EXCEPTION создаёт ошибку.
3. Проверьте errors.rs: попадает ли код в from_sqlstate.
4. Откройте issue с описанием.

Известные системные коды Postgres (не наши) — 23505 (unique violation),
40001 (serialization failure), 40P01 (deadlock detected). Они
попадают в DispatcherError::Database. В v0.2+ добавим ретраи для
40001 и 40P01.

```

---


## Файл 6: `docs/history/README.md`

```markdown
# Архив

Здесь лежат документы предыдущих итераций проекта. Они **не
используются как руководство**, но сохранены как исторический контекст.

Правило: **ничего не удаляется**. Если документ устарел — он
перемещается сюда с датой и кратким описанием.

---

## Что здесь и почему

### `AUDITED_DOC_v15.docx`

**Что:** исходный документ «Архитектура открытых экосистем оркестрации
автономных ИИ-агентов на базе канбан-систем и механизмов человеческого
контроля», версия 15.

**История:** 14 итераций архитектурной ревизии (v1 → v14) + 16 аудитов
(v1 → v16). Начинался как обзор платформ, постепенно превратился в
техническую спецификацию с кодом.

**Содержит:**
- Сравнительный анализ 7 открытых платформ (Plane, Planka, Hermes,
  Veritas, KaibanJS, Agent-Kanban, Dify).
- Модель угроз (9 векторов атак).
- Формальную модель состояний (Guarded FSM).
- Python reference implementation (устарел, заменён на Rust).
- Docker Compose с Redis, OPA, approvals (устарел, упрощён).

**Почему в архиве:**
- Redis убран (дедупликация через Postgres).
- Python заменён на Rust.
- OPA / approvals отложены до v2.
- 100+ страниц сжаты до 25.

**Что использовать вместо:**
- `docs/optional/PLATFORM_COMPARISON.md` — обзор платформ.
- `docs/optional/THREAT_MODEL.md` — модель угроз.
- `docs/ARCHITECTURE.md` — текущая архитектура.
- `docs/optional/ARCHITECTURE_DECISIONS.md` — ADR.

---

### `REPORT_v0_e2e.md`

**Что:** отчёт независимой верификации v0.1 от OpenClaw / агента на
ноутбуке.

**Дата:** 2026-10-08.

**Содержит:**
- `cargo build` логи (0 errors, 0 warnings).
- Unit-тесты (8 passed).
- Integration-тесты (5 passed).
- Native e2e на Postgres 18.6.
- Compose e2e на Postgres 16.4.
- Таймлайн fencing + reclaim + crash + recovery.
- Diff правок.

**Ключевое:** подтверждено, что система работает end-to-end на двух
платформах. Зафиксированы новые находки (E0521, Dockerfile rust 1.98,
chmod 644).

**Почему ценно:**
- Независимая верификация (не автор).
- Воспроизводимые команды.
- Честное описание находок, включая те, что не были предсказаны.

---

### `swarm_fixes.diff`

**Что:** unified diff правок, найденных при верификации v0.1.

**Правки:**
1. `#[from] postgres::Error` вместо `#[source]`.
2. Клон `Lease` перед `thread::spawn` (E0521).
3. Убран `use tracing::warn` (не используется).
4. Тест `sqlstate_known_codes_map` проверяет вариант, не текст.

**Статус:** все четыре правки внесены в канонический репозиторий.

---

## Как читать архив

**Не подряд.** Открывайте конкретный документ, если нужен контекст:

- «Почему решение было таким?» → `AUDITED_DOC_v15.docx`, раздел ADR.
- «Какие дефекты находили?» → `AUDITED_DOC_v15.docx`, раздел «Аудит».
- «Как проверяли v0.1?» → `REPORT_v0_e2e.md`.
- «Какие правки были?» → `swarm_fixes.diff`.

**Для текущей работы:** используйте документы в `docs/`, а не в
`docs/history/`.

---

## Как добавить в архив

1. Скопировать устаревший документ сюда с датой в имени:
   `<original_name>_<YYYY-MM-DD>.<ext>`.
2. Добавить запись в этот файл: что, когда, почему в архиве, что
   использовать вместо.
3. В исходном месте (`docs/<original>.md`) — не оставлять. Документ
   либо актуален, либо в архиве.

**Даты — ISO 8601** (`2026-10-08`), не «8 октября» — сортировка ломается.
