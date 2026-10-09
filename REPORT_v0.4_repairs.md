# Ремонт пакета и кода: итоги внешнего аудита v0.4

**Дата:** 2026-10-09
**Кто:** Арго (агент), по вердикту внешнего аудитора
**Репозиторий:** `/home/d88u525/Projects/rust/swarm-dispatcher` (не git — файловая копия)
**Источник замечаний:** вердикт аудитора по пакету
`PROJECT_CODE_BUNDLE_v0.4.xml` + `PROJECT_INFRA_BUNDLE_v0.4.xml` + `PROJECT_DOCS_BUNDLE_v0.4.xml`
(100 файлов, 914 КБ, sha256/md5 по каждому).

Аудитор дал общий вердикт «пакет корректен» и три конкретных дефекта.
Ниже — что сделано по каждому, что нашлось попутно, и что осталось открытым.

---

## 1. Три замечания аудитора

### 1.1. `schema/roles.sql` — заглушка

- **Было:** файл из одной строки-комментария, никем не применялся.
  `docker-compose.yml` монтирует как `/sql/roles.sql` именно `step2/roles.sql`.
- **Риск:** аудитор спотыкается о файл-обманку, принимая его за актуальные роли.
- **Сделано:** перенесён в `docs/history/obsolete/schema_roles.sql.stub`
  (ничего не удалено безвозвратно).
- **Проверка:** `schema/` теперь содержит только `schema_v0.sql`.

```
24d154d1c1c3ec534b69cb7736dc454e944918630d2ca049f4bf05dc2761b979  schema_roles.sql.stub
```

### 1.2. `step2b/test_roles_2b.DELIVERED_partial.py` — обрезанный остаток

- **Было:** 3477 Б против 8859 Б у полного `step2b/test_roles_2b.py`
  (остаток от прерванной доставки, тесты 5–16 отсутствовали).
- **Сделано:** перенесён в `docs/history/obsolete/test_roles_2b.DELIVERED_partial.py`.
  Исторический след сохранён; упоминания в `REPORT_v0.2_step2b*.md` остаются
  валидными — там зафиксирован именно этот md5.

```
156146fb1bc1df3392c7b3bec215510db62dd7e4c910e8653c364a36a00d7a94  test_roles_2b.DELIVERED_partial.py
```

### 1.3. `swarm-worker` dev-depends on `swarm-planner` — **открыто**

```toml
[dev-dependencies]
swarm-planner = { path = "../swarm-planner" }
```

- **Где используется:** ровно один e2e-тест
  (`crates/swarm-worker/tests/worker_integration.rs:381`,
  `worker_receives_description_from_planner_e2e`).
- **Почему не тронуто:** правка = перенос теста в отдельный крейт
  (`swarm-e2e`) либо в workspace-уровневый `tests/`. Это меняет структуру
  workspace, а решение по структуре — за владельцем. Формально дефект
  косметический (dev-only, в прод-сборку не попадает).
- **Решение:** оставлено как есть, вынесено в вопросы (см. §5).

---

## 2. Найдено попутно: файл-сирота в SQL-порядке

`step2b/roles_2b_patch_section3.sql` (1579 Б) — **никем не применяется**.

- `docker-compose.yml` его не монтирует (`grep roles_2b_patch_section3 → пусто`).
- В `scripts/03-role-patches.sh` под glob `roles_patch_*` он не попадает:
  glob ищет в `/sql/`, а файл лежит в `step2b/` и назван `roles_2b_patch_*`.
- **Содержимое:** секция «3. Роли для тестов» — построчная копия той же
  секции из `step2b/roles_2b.sql` (сверено `comm`: 25 строк / 25 строк,
  различий нет).

**Вывод:** это исторический черновик, уже влитый в `roles_2b.sql`.
Функционального вреда нет, но он попал в аудит-пакет и в «порядок применения»
из саммари аудитора — то есть **вводит в заблуждение** (создаёт видимость
пятого обязательного шага).

**Рекомендация:** перенести в `docs/history/obsolete/` либо оставить,
но убрать из аудит-индекса и из списка порядка применения.
Требует решения владельца (правка состава пакета).

```
6dff73f212efba97a30d36c7fec7ee0bf2e46be8bff3dd714201d10ac8d6c0b7  step2b/roles_2b_patch_section3.sql
```

---

## 3. Дефекты, найденные при реализации v0.4 (не аудитом)

Реализация shell-tools + Docker-песочницы вскрыла два реальных дефекта
изоляции, которых в ТЗ не было. Оба ломали `cargo test` внутри песочницы.

### 3.1. tmpfs `/workspace` монтировался `root:root 0755`

- **Симптом:** запись артефакта под uid 1001 → `Permission denied`.
- **Причина:** `--tmpfs /workspace:…` без `uid/gid/mode` → Docker монтирует
  как root.
- **Фикс:** `/workspace:rw,exec,nosuid,size=1g,uid=1001,gid=1001,mode=0700`.
- **Регресс-тест:** `run_args_have_security_flags` проверяет наличие
  `mode=0700` в аргументах.

### 3.2. Docker добавляет `noexec` к `--tmpfs`

- **Симптом:** `cargo test` собирал бинарник, но не мог его запустить.
- **Причина:** Docker по умолчанию монтирует tmpfs с `noexec`.
- **Фикс:** явный `exec` в опциях `/workspace`.
- **Обоснование допустимости:** песочница без capabilities, без сети,
  с read-only корнем; исполнить можно только то, что туда положил сам воркер.
  Зафиксировано в `docs/KNOWN_LIMITATIONS.md` §5.7.

---

## 4. Проверки после ремонта

| Проверка | Результат |
|---|---|
| `cargo build --workspace --locked` | 0 ошибок / 0 предупреждений |
| `cargo test --lib` (все крейты) | 8 + 2 + 16 + **35** |
| `cargo test --test worker_integration` | 10 passed |
| `cargo test --test planner_integration` | 9 passed |
| `cargo test --test worker_sandbox` (реальный Docker) | **9 passed** |
| `pytest tests/test_v0.py` | 26 passed |
| `pytest step2/test_roles.py` | 30 passed |
| `pytest step2b/test_roles_2b.py` | 19 passed |
| Мутации M1–M6 (`scripts/run_mutations_v04.sh`) | **6 из 6 поймано** |

Мутации: M1 allowlist, M2 timeout, M3 `--cap-drop ALL`, M4 `--network none`,
M5 `--read-only`, M6 audit-событие. После прогона исходники восстановлены,
md5 совпадают с эталоном:

```
4fe03dc01488508709120bc6c67b11b9  crates/swarm-worker/src/allowlist.rs
443e785d2c357d5326fd637466cb56e4  crates/swarm-worker/src/sandbox.rs
79c8ad46c0e46ecaa163c63233bf9540  crates/swarm-worker/src/agent_loop.rs
```

Ручной smoke подтвердил изоляцию: `CapEff: 0000000000000000` (нет
capabilities), сеть не резолвится, корень read-only, таймаут даёт exit 124.

Убраны 11 утёкших sandbox-контейнеров, оставшихся после прерванного прогона.

---

## 5. Открытые вопросы (ждут решения владельца)

1. **Tool calls и провайдер.** `deepseek-v4-flash` через relay.fast отвечает
   tool calls в DSML-разметке DeepSeek (`<｜｜DSML｜｜ invoke name=…>`), а не в
   JSON. Парсер ждёт `{"tool": …, "args": {…}}` → `InvalidToolCall`;
   в API-ответе `tool_calls: null`, `finish_reason: stop`.
   Варианты: (а) DSML-парсер в `parse_step` (~40 строк + тесты);
   (б) сменить модель; (в) нативный `tools` API (правка `swarm-llm`, ТЗ §0
   просит не трогать). Критерий 9 закрыт после добавления DSML-парсера; подробности — в `docs/VERIFICATION_LOG.md` §V-005.
2. **Замечание 1.3** — выносить ли e2e-тест из `swarm-worker` в `swarm-e2e`.
3. **Файл-сирота** `roles_2b_patch_section3.sql` — в `obsolete/` или вон из индекса.

---

### 5.4. DSML-парсер — закрыто

Добавлен `crates/swarm-worker/src/parse_step.rs`: поддержаны native DSML DeepSeek, JSON `args`, именованные параметры, несколько `invoke`, escaped quotes, ошибки malformed input и прежний JSON-формат. `agent_loop.rs` использует parser и выполняет batch-вызовы с аудитом.

Проверено: 42 unit-теста и 10 Docker integration-тестов; реальный `deepseek-v4-flash` выполнил 4 tool calls, создал и протестировал Rust-крейт, сохранил artifact, `verify_chain=true`.

## 6. Ссылки

- `docs/history/obsolete/README.md` — что и почему перенесено.
- `docs/audit/AUDITOR_BRIEF_v0.4.md` — задание аудитору.
- `docs/KNOWN_LIMITATIONS.md` §5.6, §5.7 — shell tools и tmpfs-монтирование.
- `CHANGELOG.md` [0.4.0] — что вошло в релиз.
- `docs/TZ_v0.4_shell_tools_sandbox.md` — ТЗ, по которому реализовано.
