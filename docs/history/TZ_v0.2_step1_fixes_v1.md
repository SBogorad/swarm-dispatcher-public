# ТЗ: правки v0.1 (шаг 1 фазы v0.2)

Правки, выявленные в ходе верификации v0.1. Внести **до** начала работы
над Planner.

## 0. Неприкосновенно

- `schema/schema_v0.sql` — не менять.
- `tests/test_v0.py` — 26 тестов, не менять.
- `crates/swarm-core/tests/integration.rs` — 5 тестов, не менять.

## 1. `Cargo.toml` (workspace root)

Заменить:
```toml
rust-version = "1.75"
```

На:

```toml
rust-version = "1.85"
```

Причина: транзитивная зависимость block-buffer 0.12 требует
edition2024 (Rust ≥ 1.85). MSRV 1.75 был декларативно неверен.

2. Dockerfile

Заменить:

```dockerfile
FROM rust:1.80-slim AS builder
```

На:

```dockerfile
# ВНИМАНИЕ: rust 1.85+ обязателен из-за edition2024 в block-buffer 0.12
# (через postgres-protocol -> sha2 0.11 -> digest 0.11 -> block-buffer 0.12).
FROM rust:1.98-slim AS builder
```

Дополнительно: в runtime-стадии заменить копирование schema на:

```dockerfile
COPY --chmod=644 schema/schema_v0.sql /docker-entrypoint-initdb.d/01-schema.sql
```

Это устраняет требование chmod 644 на стороне пользователя.

Важно: эта правка не в текущем Dockerfile (там только бинарники).
Добавить schema/schema_v0.sql в build context (или в отдельный schema/
COPY-слой) и убедиться, что --chmod=644 применяется.

3. docker-compose.yml

· Удалить строку version: "3.9" — она deprecated.
· В сервисе cli удалить command: [...] — иначе compose подменяет
  entrypoint, и docker compose run --rm cli seed падает с
  exec: "seed" not found.
· Инструкция в README: всегда запускать CLI через
  docker compose run --rm --no-deps cli swarm-cli <command>.

4. README.md

Обновить:

1. Требования: добавить «Rust 1.85+».
2. Быстрый старт (compose): команды с --no-deps cli swarm-cli.
3. Добавить второй путь: native-развёртывание без Docker (см. docs/NATIVE_SETUP.md).
4. Известные ограничения: подтвердить, что save_artifact пишет только
   metadata в БД, файл на диск не пишется (by design в v0.1).
5. Таблица кодов ошибок — вынести из README в docs/ERRORS.md (если
   ещё не сделано), чтобы README оставался компактным.

5. Rust-правки (из swarm_fixes.diff)

Уже описаны в отчёте OpenClaw. Дополнительно:

5.1. crates/swarm-core/src/errors.rs

```diff
-    Database(#[source] postgres::Error),
+    Database(#[from] postgres::Error),
```

Почему: #[source] даёт доступ к source(), но не конвертацию через
?. Нужен impl From<postgres::Error> for DispatcherError.

5.2. Тест sqlstate_known_codes_map

Переписать на проверку варианта, не текста:

```rust
assert!(matches!(err, DispatcherError::LeaseLost));
```

Почему: тест проверял err.to_string().contains("lease_lost"), а
сообщение было "lease lost or expired". Тест падал по неверной причине.

5.3. crates/swarm-worker/src/main.rs

Клонировать Lease перед thread::spawn:

```rust
let lease_for_hb = lease.clone();
let hb = thread::spawn(move || {
    // использовать lease_for_hb вместо lease
});
```

Почему: E0521 — ссылка lease не может быть перемещена в поток
('static требуется). Клон — самый простой корректный вариант.

5.4. crates/swarm-watchdog/src/main.rs

Убрать warn из use tracing::{...} — не используется.

5.5. Чистка пустых строк

В types.rs, dispatcher.rs, cli/main.rs — убрать лишние пустые
строки в конце файлов (тривиально).

6. Критерий готовности шага 1

1. cargo build — 0 errors, 0 warnings.
2. cargo test --lib — 8 passed.
3. cargo test --test integration -- --ignored — 5 passed.
4. pytest tests/test_v0.py — 26 passed.
5. docker compose up -d --build — 4 контейнера поднимаются.
6. docker compose run --rm --no-deps cli swarm-cli seed — работает.
7. docker compose run --rm --no-deps cli swarm-cli status — вывод таблицы.

7. Формат отчёта

Один markdown-файл REPORT_v0.2_step1.md:

· Список файлов, которые изменены.
· git diff по каждому (или общий diff).
· Вывод всех тестов.
· Замечания, если что-то пошло не так.
· Если правка не проходит — стоп и вопрос автору.

```

---

# Файл 3: `docs/DOC_STRATEGY.md`

```markdown
# Стратегия документации проекта

Как сохранить проект так, чтобы:
- ничего не потерялось при перезапуске чата;
- новый участник мог присоединиться за час;
- решения не «зависали в воздухе»;
- перспективы были видны.

---

## 1. Принципы

**1. Markdown — единственный формат.****
Не `.docx`, не `.pdf`, не Notion. Markdown работает с git, читается
человеком, парсится машиной. Если нужен PDF — генерируется из markdown.

**2. Один документ — одна цель.**
Не «большой документ на всё». Отдельные файлы: архитектура, ADR,
методология, roadmap, ТЗ. Каждый можно читать независимо.

**3. История — в `docs/history/`.**
Текущие документы — в `docs/`. Устаревшие версии — в `docs/history/` с
датой в имени. Ничего не удаляется.

**4. Изменения — через CHANGELOG.**
Каждое изменение документируется в `CHANGELOG.md`: что, почему, кто, когда.

**5. Один manifest.**
`MANIFEST.md` в корне — карта всех документов. Что где лежит, для чего,
когда читать. Если добавляется новый документ — обновляется manifest.

---

## 2. Структура репозитория

```

swarm-dispatcher/
├── README.md                    # входная точка
├── MANIFEST.md                  # карта всех файлов
├── CHANGELOG.md                 # история изменений
├── LICENSE                      # MIT или Apache-2.0
├── Cargo.toml
├── Dockerfile
├── docker-compose.yml
├── .env.example
│
├── crates/
│   ├── swarm-core/
│   ├── swarm-watchdog/
│   ├── swarm-worker/
│   ├── swarm-cli/
│   └── swarm-planner/           # v0.2
│
├── schema/
│   ├── schema_v0.sql
│   └── roles.sql                # v1.0
│
├── tests/
│   ├── test_v0.py               # 26 SQL-тестов
│   ├── integration.rs           # 5 Rust integration
│   └── test_planner.py          # v0.2
│
├── bench/                       # v1.0
│   ├── load_test.py
│   └── RESULTS.md
│
└── docs/
├── PROJECT_STATE.md         # что есть сейчас
├── METHODOLOGY.md           # принципы работы
├── ARCHITECTURE.md          # 6 диаграмм
├── ARCHITECTURE_DECISIONS.md # ADR
├── PLATFORM_COMPARISON.md   # обзор платформ
├── THREAT_MODEL.md          # модель угроз
├── BRIEF_FOR_AGENTS.md      # бриф для агентов
├── PLAIN_EXPLANATION.md     # для человека простыми словами
├── ROADMAP.md               # план v0.1 → v1.0 (этот документ)
├── NEXT_STEPS.md            # что делать прямо сейчас
├── KNOWN_LIMITATIONS.md     # известные ограничения
├── VERIFICATION_LOG.md      # независимые прогоны
├── ERRORS.md                # таблица кодов ошибок
├── DOC_STRATEGY.md          # этот файл
├── TZ_v0.2_step1_fixes.md   # текущее ТЗ
├── TZ_v0.2_step2_planner.md # следующее ТЗ
│
└── history/
├── AUDITED_DOC_v15.docx # исходный документ (14 итераций)
├── REPORT_v0_e2e.md     # отчёт верификации v0.1
├── swarm_fixes.diff     # diff правок v0.1
└── README.md            # что здесь и почему

```

---

## 3. Что куда писать

| Когда… | Куда писать |
|--------|-------------|
| Принято архитектурное решение | `docs/ARCHITECTURE_DECISIONS.md` (новый ADR) |
| Изменилось поведение системы | `CHANGELOG.md` + обновление `docs/PROJECT_STATE.md` |
| Появился новый компонент | `docs/ARCHITECTURE.md` (диаграмма) + `crates/` (код) |
| Найден дефект | `docs/AUDIT_HISTORY.md` (если исторический) или новый issue |
| Независимая верификация | `docs/VERIFICATION_LOG.md` |
| Изменение плана | `docs/ROADMAP.md` + `CHANGELOG.md` |
| Новое ТЗ | `docs/TZ_vX.Y_<name>.md` |
| Устаревший документ | `docs/history/` + запись в `docs/history/README.md` |

---

## 4. Как сохранять в условиях ограниченного чата

**Проблема:** чат с ИИ имеет лимит сообщений/токенов. Документы,
созданные в чате, теряются при следующем чате.

**Решение:** каждое сообщение с документом заканчивается явной командой:

> «Скопируй этот файл в `docs/<name>.md`».

Пользователь (или агент на ноутбуке) сохраняет. Через несколько итераций
репозиторий содержит всё необходимое для продолжения в новом чате.

**Что делать при перезапуске чата:**

1. Открыть `MANIFEST.md` — список всех документов.
2. Открыть `docs/PROJECT_STATE.md` — где мы сейчас.
3. Открыть `docs/NEXT_STEPS.md` — что делать.
4. Открыть последнее ТЗ — что делать конкретно.
5. Продолжить с того же места.

**Что делать, если документа не хватает:**

- Прикрепить её к новому чату.
- Или скопировать в `docs/` и попросить ИИ прочитать.
- Или описать словами — если помните контекст.

---

## 5. Как сохранить перспективы

«Перспективы» — это то, что **не реализовано, но планируется**.
Хранить в трёх файлах:

| Файл | Что внутри |
|------|-----------|
| `docs/ROADMAP.md` | v0.1 → v1.0, фазы, оценки, критерии |
| `docs/NEXT_STEPS.md` | что делать в ближайшие дни |
| `docs/KNOWN_LIMITATIONS.md` | что НЕ работает сейчас |

**Правило:** каждая «идея на будущее» попадает **в ROADMAP или
KNOWN_LIMITATIONS**, а не остаётся в голове или в чате. Иначе через
месяц никто не вспомнит.

---

## 6. Внешняя память

Если чат с ИИ — единственный канал — использовать **markdown-файлы
как внешнюю память**:

- Перед завершением сессии — сохранить все созданные документы.
- При новой сессии — приложить `MANIFEST.md` + `PROJECT_STATE.md` +
  последнее ТЗ.
- ИИ читает их и продолжает с того же места.

Это работает. Проверено на этом проекте: 14 итераций audited-документа
сохранены в `docs/history/`, и из них можно извлечь любую деталь.

---

## 7. Долгосрочная стратегия

**Для проекта-портфолио:**
- Публичный GitHub-репозиторий.
- README с быстрым стартом.
- Документы в `docs/`, история в `docs/history/`.

**Для публикации:**
- Технический отчёт — отдельный файл `REPORT.md`.
- Написан **из** документации, не поверх неё.
- Ссылки на репозиторий, тесты, верификацию.

**Для расширения команды:**
- Новый участник читает `MANIFEST.md` → `PROJECT_STATE.md` →
  `METHODOLOGY.md` → `NEXT_STEPS.md`.
- Час — и он готов работать.
- Если что-то непонятно — открывает `ARCHITECTURE_DECISIONS.md` или
  `docs/history/`.

---

## 8. Правила именования

- **Текущие документы:** `UPPER_CASE.md` (`README.md`, `CHANGELOG.md`).
- **Тематические:** `UPPER_CASE.md` (`ARCHITECTURE.md`, `METHODOLOGY.md`).
- **ТЗ:** `TZ_vX.Y_<name>.md` (`TZ_v0.2_step1_fixes.md`).
- **Отчёты:** `REPORT_<date>_<context>.md` (`REPORT_v0_e2e.md`).
- **История:** `<original_name>_<date>.<ext>` (`AUDITED_DOC_v15.docx`).

Даты — ISO 8601 (`2026-10-08`). Не «8 октября» — сортировка ломается.

---

## 9. Что делать, если документ устарел

**Не удалять.** Переместить в `docs/history/` с датой. Добавить запись в
`docs/history/README.md`:

```markdown
## AUDITED_DOC_v15.docx (архив, 2026-10-08)

Исходный документ — 14 итераций архитектурной ревизии. Содержит
сравнительный анализ платформ, модель угроз, формальную модель состояний.
Многие решения пересмотрены: Redis → Postgres, Python → Rust, 100 страниц
→ 25. Актуальные версии — в `docs/PLATFORM_COMPARISON.md` и
`docs/THREAT_MODEL.md`.
```

Это сохраняет контекст: почему решение было таким, почему изменилось.

---

10. Итог

Три правила:

1. Один документ — одна цель. Не мешать архитектуру с ТЗ, план с
   историей.
2. История — в docs/history/. Ничего не удаляется.
3. Manifest — в корне. Карта всех файлов, всегда актуальна.

Одна проверка:

Если через год новый человек откроет репозиторий и за час не поймёт,
что это и как продолжить — значит, стратегия не сработала.
