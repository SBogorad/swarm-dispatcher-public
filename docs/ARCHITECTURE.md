# Архитектура системы

Документ описывает архитектуру reference implementation. Содержит шесть
диаграмм: контекст, контейнеры, данные, состояния, потоки, развёртывание.

Стек: **Postgres 16+** (единственный источник истины) + **Rust 1.85+**
(диспетчер, сторож, воркеры, CLI). Без Redis, без внешних очередей,
без OPA в v0.x.

---

## 1. Контекст системы (C4 Context)

```mermaid
graph TB
    subgraph actors [Акторы]
        Human[Человек-оператор]
        Planner[Планировщик / декомпозитор]
    end

    subgraph system [Swarm Dispatcher]
        SD[Оркестратор задач<br/>Guarded FSM, аренды, хэш-цепочка]
    end

    subgraph external [Внешний мир]
        LLM[LLM API<br/>Claude / GPT / локальные]
        Git[Git repository]
        S3[Object storage]
    end

    Planner -->|создаёт граф задач| SD
    Human -->|наблюдает, ревьюит,<br/>reopen/unblock| SD
    SD -->|выдаёт задачи| Worker[Рабочие воркеры]
    Worker -->|вызывает| LLM
    Worker -->|сохраняет артефакты| S3
    Worker -->|коммитит финальный результат| Git
```

Ключевое: Swarm Dispatcher — Control Plane, который координирует
задачи, но не выполняет их. Воркеры берут задачи по pull-модели и
работают вне транзакций.

---

2. Контейнеры (C4 Container)

```mermaid
graph TB
    subgraph bins [Бинарники Rust]
        Watchdog[swarm-watchdog<br/>reclaim + promote<br/>каждые 30 сек]
        WorkerA[swarm-worker-1]
        WorkerB[swarm-worker-2]
        CLI[swarm-cli<br/>seed / status / review]
        Planner[swarm-planner<br/>v0.2 step 3]
    end

    subgraph lib [Библиотека]
        Core[swarm-core<br/>SwarmDispatcher<br/>DispatcherTx<br/>типы, ошибки]
    end

    subgraph pg [Postgres 16+]
        Schema[schema_v0.sql<br/>6 таблиц, 19 функций]
        Roles[roles.sql<br/>6 ролей, SECURITY DEFINER]
    end

    subgraph sandbox [Sandbox]
        SB[Worker sandbox<br/>cap_drop: ALL<br/>read_only: true]
    end

    Watchdog --> Core
    WorkerA --> Core
    WorkerB --> Core
    CLI --> Core
    Planner --> Core
    Core -->|postgres crate| Schema
    Schema -.-> Roles
    WorkerA -.->|изолированная среда| SB
    WorkerB -.->|изолированная среда| SB
```

Ключевое: все бинарники используют одну библиотеку swarm-core.
Никакого IPC между бинарниками — только через Postgres.

---

3. Модель данных (ER)

```mermaid
erDiagram
    tasks ||--o{ task_deps : "зависит от"
    tasks ||--o{ artifacts : "создаёт"
    tasks ||--o{ events : "журнал"
    tasks ||--o{ reviews : "ревью"
    workers ||--o{ tasks : "владеет"
    tasks ||--o{ tasks : "root_id"

    tasks {
        uuid id PK
        uuid root_id FK
        text state
        jsonb contract
        bigint lease_epoch
        text lease_owner
        timestamptz lease_until
        int attempts
        int rework
        bigint budget_cap
        bigint spent_tokens
        text result_ref FK
        int priority
        text block_reason
        timestamptz created_at
        timestamptz updated_at
    }
    artifacts {
        uuid task_id FK
        text ref PK
        text created_by
        bigint lease_epoch
        text content_type
        bigint size_bytes
        text storage_path
        timestamptz created_at
    }
    task_deps {
        uuid task_id FK
        uuid dep_id FK
    }
    workers {
        text id PK
        jsonb capabilities
        text_array affinity_tags
        timestamptz last_seen
    }
    events {
        bigserial seq PK
        uuid task_id FK
        text event_id
        text kind
        text actor
        jsonb payload
        timestamptz at
        bytea prev_hash
        bytea hash
    }
    reviews {
        bigserial id PK
        uuid task_id FK
        text artifact_ref FK
        text reviewer
        text verdict
        jsonb checks
        timestamptz created_at
    }
```

Ключевые инварианты схемы:

1. tasks.lease_epoch — монотонный fencing token.
2. tasks.state — CHECK (state IN (...)).
3. events (task_id, event_id) UNIQUE — дедупликация через БД.
4. events.prev_hash / hash — tamper-evidence.
5. artifacts PRIMARY KEY (task_id, ref) — привязка к задаче и хэшу.
6. task_deps PRIMARY KEY (task_id, dep_id) — без дублей рёбер.

---

4. Жизненный цикл задачи (Guarded FSM)

```mermaid
stateDiagram-v2
    [*] --> pending: add_task
    pending --> ready: promote_ready<br/>(все deps = done)
    ready --> leased: claim_task<br/>(affinity match)
    leased --> review: complete_task<br/>(result_ref создан)
    leased --> ready: reclaim_expired<br/>(lease истёк, attempts < max)
    leased --> blocked: reclaim_expired<br/>(lease истёк, attempts = max)
    review --> done: submit_review<br/>(approved)
    review --> ready: submit_review<br/>(changes_requested)
    review --> blocked: submit_review<br/>(rejected или max_rework)
    done --> ready: reopen_task<br/>(regression_found)
    blocked --> pending: unblock_task<br/>(operator_override)
    done --> [*]
    blocked --> [*]
```

Guards:

· pending → ready: все deps в состоянии done.
· ready → leased: spent_tokens < budget_cap, attempts < budget.attempts,
  capabilities воркера совпадают с needs задачи.
· leased → review: assert_lease (epoch и owner совпадают),
  result_ref существует в artifacts.
· review → done: ревьюер ≠ автор артефакта, все checks прошли,
  verdict = approved.
· leased → ready (reclaim): attempts + 1 < max_attempts.
· done → ready (reopen): роль оператора — ak-maintainer или
  Human_Operator.

Каскадный откат при reopen_task: все потомки в
ready/leased/review/done переходят в blocked с lease_epoch + 1,
что немедленно отсекает зомби-воркеров.

---

5. Потоки данных (Sequence)

5.1. Успешное выполнение

```mermaid
sequenceDiagram
    participant P as Planner
    participant D as swarm-core
    participant DB as Postgres
    participant W as Worker
    participant LLM as LLM API

    P->>D: add_task(id, contract, root)
    D->>DB: SELECT add_task(...)
    DB-->>D: OK, event 'created'
    P->>D: add_dep(task, dep)
    D->>DB: SELECT add_dep(...)
    DB-->>D: OK, event 'dep_added'

    W->>D: register_worker(id, caps, tags)
    D->>DB: SELECT register_worker(...)

    W->>D: claim(worker_id)
    D->>DB: SELECT claim_task(...)
    DB-->>D: Lease {task_id, epoch, contract}
    D-->>W: Lease

    loop каждые lease_sec/3
        W->>D: heartbeat(lease)
        D->>DB: SELECT heartbeat(...)
        DB-->>D: true
    end

    W->>LLM: inference(contract)
    LLM-->>W: result
    W->>D: save_artifact(lease, ref, path)
    D->>DB: SELECT save_artifact(...)

    W->>D: complete(lease, result_ref, tokens)
    D->>DB: SELECT complete_task(...)
    DB-->>D: "review"

    Note over DB: Reviewer проверяет result_ref
    DB->>DB: submit_review(task, ref, reviewer, approved)
    Note over DB: state = done, promote_ready для потомков
```

5.2. Падение воркера

```mermaid
sequenceDiagram
    participant W1 as Worker 1
    participant D as swarm-core
    participant DB as Postgres
    participant WD as Watchdog
    participant W2 as Worker 2

    W1->>DB: claim_task
    DB-->>W1: Lease {epoch=1}
    Note over W1: Падение (OOM, SIGKILL)
    Note over DB: lease_until истекает

    WD->>DB: reclaim_expired()
    Note over DB: attempt +1, epoch +1, state = ready
    DB-->>WD: 1 задача восстановлена

    W2->>DB: claim_task
    DB-->>W2: Lease {epoch=2}

    Note over W1: Если W1 "ожил" и пробует save_artifact(epoch=1)
    W1->>DB: save_artifact(epoch=1)
    DB-->>W1: LS001 lease_lost
```

5.3. Каскадный откат

```mermaid
sequenceDiagram
    participant Op as Operator
    participant D as swarm-core
    participant DB as Postgres

    Op->>D: reopen(root_task_id, actor)
    D->>DB: SELECT reopen_task(...)
    Note over DB: root: done -> ready, epoch +1
    Note over DB: recursive CTE: все потомки в ready/leased/review/done -> blocked, epoch +1
    DB-->>D: N потомков заблокировано
    D-->>Op: N
```

5.4. Верификация хэш-цепочки

```mermaid
sequenceDiagram
    participant Auditor
    participant D as swarm-core
    participant DB as Postgres

    Auditor->>D: verify_chain(task_id)
    D->>DB: SELECT verify_chain(...)
    Note over DB: Пройти по events ordered by seq
    Note over DB: prev_hash == prev.hash, hash == sha256(prev_hash || payload)
    DB-->>D: true / false
    D-->>Auditor: результат
```

---

6. Развёртывание (Deployment)

```mermaid
graph TB
    subgraph host [Docker Host]
        subgraph net [Network: swarm-internal]
            PG[postgres:16.4-alpine<br/>volume: postgres_data]
            WD[swarm-watchdog<br/>interval: 30s]
            W1[swarm-worker-1<br/>cap_drop: ALL]
            W2[swarm-worker-2<br/>cap_drop: ALL]
            CL[swarm-cli<br/>profile: tools]
        end

        subgraph vol [Volumes]
            V1[postgres_data]
            V2[artifacts<br/>/var/swarm/artifacts]
        end
    end

    WD -->|5432| PG
    W1 -->|5432| PG
    W2 -->|5432| PG
    CL -->|5432| PG
    PG -.-> V1
    W1 -.-> V2
    W2 -.-> V2

    Ext[External LLM API]
    W1 -->|HTTPS через gateway| Ext
    W2 -->|HTTPS через gateway| Ext
```

Принципы:

· Postgres — единственный stateful компонент. Backup через WAL.
· Все Rust-бинарники — stateless, N копий.
· Воркеры — изолированные контейнеры с cap_drop: ALL,
  read_only: true, no-new-privileges.
· Egress — только через gateway (для LLM API).
· Volumes — postgres_data (БД) и artifacts (v0 — локальные, v1 — S3).

---
7. Решения, зафиксированные в архитектуре

№ Решение Обоснование
1 Postgres — единственный источник истины Атомарность, дедупликация через UNIQUE
2 Без Redis Устраняет рассинхронизацию и «сгоревший ключ»
3 Fencing token (lease_epoch) Защита от зомби без хранения истории
4 Хэш-цепочка событий Tamper-evidence (не non-repudiation — v2.0)
5 Pull-модель с capabilities Воркер сам решает, брать ли задачу
6 Rust для инфраструктуры Типизация, один бинарник, безопасность
7 Короткие транзакции Долгая операция (inference) вне транзакции
8 Recursive cascade Единый запрос по всему поддереву
9 Без OPA/approvals в v0 Ядро работает без compliance-обвязки
10 Без :latest в Docker Воспроизводимость

---

8. Что не входит в v0.x

· OPA / Policy Engine — v2.0.
· Trust Registry + Approvals (Ed25519) — v2.0.
· CryptoVerifier — v2.0.
· Мультитенантность — v2.0.
· OTel — v1.0.
· Rate limiting — v1.0 (на реверс-прокси).
· S3 для артефактов — v1.0 (в v0.x локальная ФС).

