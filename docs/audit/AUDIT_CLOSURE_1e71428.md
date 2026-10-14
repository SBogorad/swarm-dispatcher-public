# Закрытие внешнего аудита — снимок `1e71428`

**Проект:** `swarm-dispatcher`
**Ветка/снимок:** `master` @ `1e71428e7ffc0ee750b8739630fe3310d2a04e32`
**Дата вердикта:** 2026-10-14
**Пакет:** `swarm-dispatcher-audit-1e71428-uncompressed.zip` (zip -0 Store, 4 XML-тома, 202 файла)
**Аудитор:** внешний (Gemini), второй проход по CODE + INFRA/SQL/RBAC/CI

---

## 1. Итоговый вердикт

**PASS** — 0 CRITICAL / 0 HIGH / 0 MEDIUM.

Пакет принят как **финальный эталонный срез**. Все 4 PR, закрывающие находки первого и второго
проходов, проверены по фактическому коду снимка `1e71428`.

| PR | Находки | Вердикт |
|---|---|---|
| #49 | FIND-CODE-01, FIND-NEW-01/02/03 | PASS |
| #50 | FIND-CODE-04/03, остаток FIND-NEW-06 | PASS |
| #51 | FIND-CODE-02 (task_rework_feedback) | PASS |
| — | Упаковка/состав пакета (INFRA, 12 миграций, CODE 58 файлов Rust) | PASS |

## 2. Что проверено по факту кода

### 2.1. PR #49 — identity в `fail_task`/`fail_task_state`, эпоха в ключах событий, диапазоны `budget.*`

- `step2b/roles_2b.sql`: `fail_task` (стр. 209) и `fail_task_state` (стр. 230) переопределены,
  `perform assert_worker_identity(p_worker);` — первая содержательная строка (стр. 213/234).
- Обе функции добавлены в `ALLOWED_OVERRIDES` (`tests/test_no_duplicate_function_bodies.py:41-51`)
  и в матрицу `step2/test_roles.py`.
- `grant execute on function fail_task_state(uuid, text, bigint, text, text, text)` выдан
  `swarm_worker_w1`, `swarm_worker_w2`, `swarm_worker_w_1`, `swarm_worker_gpu_1` —
  боевой путь отказа (`worker.rs` → `dispatcher.rs::fail_with_state`) больше не даёт `42501`.
- `schema_v0.sql:525` — `'art:' || p_epoch || ':' || p_ref`; `schema_v0.sql:685` —
  `'review:' || t.lease_epoch || ':' || p_ref || ':' || p_reviewer`.
- `schema_v0.sql:516-521` — `insert into artifacts ... on conflict (task_id, ref) do update set
  created_by = excluded.created_by, lease_epoch = excluded.lease_epoch, content_type, size_bytes,
  storage_path, created_at = now()`.
- `validate_contract`: закрыты диапазоны `max_rework` 0..50 (стр. 225-228),
  `max_depth` 1..50 (234-237), `max_children` 1..100 (239-242), `max_tasks` 1..1000 (244-247).

### 2.2. PR #50 — расход по текущей эпохе, реальная модель в `llm_calls`, арбитр ролей

- `complete_task` фильтрует ledger по эпохе: `select count(*) into v_ledger from llm_calls
  where task_id = p_task and lease_epoch = p_epoch` — `schema_v0.sql:550`,
  `step2b/roles_2b.sql:179`, `schema/migrations/011_budget_sync_and_root_cap.sql:219`.
- `LlmClient::default_model(&self) -> &str` — `crates/swarm-llm/src/lib.rs:132` и `:422`.
  В `crates/swarm-worker/src/worker.rs:476-482` при ошибке в `llm_calls.model` пишется
  `attempted_model` (реально выбранная модель) вместо `"unknown"` → витрины
  `llm_costs_daily` / `llm_model_quality` больше не портятся.
- `scripts/compose_roles_arbiter.sh`: позитивные проверки для `swarm_reviewer`
  (`review_candidates(integer)`, `submit_review(...)`; стр. 179) и `swarm_planner`
  (`add_task(...)`, `add_dep(...)`; стр. 195), негативные — reviewer без `claim_task` (стр. 180),
  planner без `complete_task`/`submit_review` (стр. 196), плюс проверка `rolsuper`/`rolbypassrls`
  для всех сервисных ролей (стр. 228-242).

### 2.3. PR #51 — контур обратной связи при доработке (`task_rework_feedback`)

- Решение не менять OUT-сигнатуру `claim_task` (иначе `DROP FUNCTION` и поломка идемпотентности
  `011_budget_sync_and_root_cap.sql`) признано **правильным**.
- Отдельный `SECURITY DEFINER` RPC `task_rework_feedback(p_task, p_worker, p_epoch)`:
  `schema/schema_v0.sql:463`, `schema/migrations/012_claim_task_rework_feedback.sql:23`,
  проверка живой аренды через `assert_lease`; грант `EXECUTE` только `swarm_worker` (стр. 55),
  `revoke ... from public` (стр. 54).
- Воркер при `rework > 0` передаёт вердикт и checks предыдущего ревью в
  `prompt::user_prompt_with_rework` внутри `<untrusted>`.
- Тесты: `tests/test_v0.py:683-739` (доступность воркеру и закрытость для прочих).

## 3. Решение по FIND-CODE-03 — НЕ БРАТЬ (by-design, закрыто)

Запись промежуточных ретраев 5xx/429 внутрь `call_llm_with_retries` **не внедряем**.

Обоснование:
- Главный дефект FIND-CODE-03 (порча витрин `llm_costs_daily` и `llm_model_quality` из-за
  `"unknown"` при фейле) уже устранён в PR #50 (`attempted_model`).
- Транзиентные 5xx/429 до получения ответа **не списывают токены провайдера**.
- В лимите вызовов воркера (`MAX_LLM_CALLS`) каждая попытка уже учитывается через
  `cfg.llm_calls.fetch_add(1)`.
- Тащить `SwarmDispatcher` внутрь `call_llm_with_retries` не нужно.

## 4. Открытые долги

**Открытых долгов нет.** Финальный вердикт по `1e71428` — PASS без открытых замечаний.

Заметка про `task_rework_feedback` в `ALLOWED_OVERRIDES` **снята** (см. §6.4: not-applicable /
принято by-design). Пункт про рассинхрон `CHECKSUMS` — состояние упаковки, не код (см. §5).

## 5. Замечание по гигиене пакета (не влияет на вердикт)

`docs/audit/CHECKSUMS_v0.4.txt` в рабочем дереве перечисляет хэши **закоммиченных** бандлов
(`cf2d3d11…`, `411d46ae…`, `e406f882…`, `0e78021d…`), а сами бандлы в рабочем дереве —
пересобранные (mtime 2026-10-14 10:26). `sha256sum -c` в рабочем дереве поэтому даёт FAILED по всем
четырём томам.

В переданном аудитору пакете `swarm-dispatcher-audit-1e71428-uncompressed.zip` **всё сходится**:
внутренний `CHECKSUMS_v0.4.txt` (`0b857ee8…`, `0577d67b…`, `442f811b…`, `5c94d7c4…`) описывает
именно те бандлы, что лежат в архиве (совпадает с хэшами файлов в рабочем дереве).
Вывод: несоответствие — рассинхрон между закоммиченным `CHECKSUMS` и пересобранными бандлами в
рабочем дереве репозитория, а не дефект эталонного среза. Правило на будущее: пересобирать бандлы и
`CHECKSUMS_v0.4.txt` одним коммитом.

---

## 6. Уточнения аудитора по финальным вопросам (принято)

### 6.1. FIND-NEW-04 — ЗАКРЫТО (PASS), закрыто ещё в волне 1 (коммит `69f749b`), сохранено в `1e71428`

Метка стоит прямо в коде харнесса, а не в `docs/audit/` — отсюда пустой grep по документации:

```
scripts/acceptance_harness.sh:640  "tests/test_f8_orphan_llm_spend.py|F8 budget protects real spend, not only complete (Issue 5.10)"
scripts/acceptance_harness.sh:641  # FIND-NEW-04: три сьюта лежали мёртвым грузом мимо harness и CI.
scripts/acceptance_harness.sh:643  "tests/test_model_quality.py|model quality view -> selector stats (Issue #7/#10)"
scripts/acceptance_harness.sh:644  "tests/test_model_catalog.py|relay.fast pricing -> USD/1M recalculation (Issue #7/#10)"
```

Проверено: все три сьюта входят в массив `suites=(...)` внутри `review_gate_acceptance()`
(стр. 640, 643, 644), а STRICT-гейт CI (`release-gate.yml:85`) вызывает тот же
`scripts/acceptance_harness.sh`. Вердикт **PASS** подтверждён.

> **Поправка к цитате (INFO, вердикт не меняет).** Аудитор дважды указал «строки 453–457». В самом
> файле `scripts/acceptance_harness.sh` на 453–457 лежит тело `compose_security_acceptance()`
> (проверка границ worker-контейнера) — к FIND-NEW-04 отношения не имеет. Фактические координаты
> (проверено `grep -n`):
> - метка `FIND-NEW-04` — `scripts/acceptance_harness.sh:641`;
> - сьюты: `test_f8_orphan_llm_spend.py` — стр. **640**, `test_model_quality.py` — **643**,
>   `test_model_catalog.py` — **644**;
> - в томе `PROJECT_INFRA_BUNDLE_v0.4.xml` метка `FIND-NEW-04` — строка **3352**.
>
> Т. е. «453–457» — tome-локальная нумерация, а не строки скрипта. Содержательно вердикт PASS.

### 6.2. Метрика томов `58` / `202` — точное совпадение с `<manifest>`, расхождение с grep объяснено

Проверено по фактическим манифестам:

| Том | `<manifest><files count="…">` | `<file>` в `<excluded>` | всего тегов `<file>` |
|---|---|---|---|
| CODE | 58 | 2 | 60 |
| INFRA | 61 | 2 | 63 |
| DOCS_CORE | 29 | 2 | 31 |
| DOCS | 54 | 2 | 56 |
| **Итого** | **202** | **8** | **210** |

Наивный подсчёт тегов `<file` даёт +2 на каждый том (+8 на 4 тома) из-за блока `<excluded>`
(`.env*`, `target/`, `.venv/`, `*.docx|*.png`). Состав тома CODE подтверждён по атрибуту `lang`:
**44 `rust` + 9 `toml` + 5 `json` = 58**. Итоговая формулировка: «том CODE — 58 файлов
(44 `.rs`, 9 `Cargo.toml`/`Cargo.lock`, 5 `.json`), всего в 4 томах 202 файла (**176 уникальных
путей** при пересечении DOCS ∩ DOCS_CORE = **26**; пересечение CODE ∩ INFRA = 0) — прочитаны целиком».

> **Поправка к арифметике (INFO).** Аудитор привёл «173 уникальных пути, пересечение 29».
> Проверено по манифестам: уникальных путей **176**, пересечение DOCS ∩ DOCS_CORE = **26**.
> Число 29 — это размер тома DOCS_CORE, а не пересечение. 202 − 26 = **176** (а 202 − 29 = 173).
> Внутри тома CODE разбивка по `lang` подтверждена точно: 44 `rust` + 9 `toml` + 5 `json` = 58
> (9 `toml` = 8 `Cargo.toml` + 1 `Cargo.lock`).

### 6.3. FIND-CODE-03 — by-design (разделение понятий зафиксировано)

- `cfg.llm_calls` (`AtomicU32` в памяти воркера) — счётчик **HTTP-попыток** для рантайм-лимита
  `MAX_LLM_CALLS`; считает каждый ретрай.
- `public.llm_calls` — финансовый и качественный **ledger уровня логического вызова** из
  `process_lease` (1 строка = 1 завершённый `call_llm_with_retries` с суммарным `duration_ms`
  и финальным исходом).

Дефект FIND-CODE-03 закрыт в PR #50 (убран хардкод `"unknown"` при фейле). Отсутствие поштучных
строк на промежуточные 5xx/429 внутри одной попытки — осознанное архитектурное решение (by-design).

### 6.4. `task_rework_feedback` / `assert_lease` — заметка СНЯТА (not-applicable, принято by-design)

Механика подтверждена по коду (`schema_v0.sql:463-487`, `assert_lease` — `:500-508`):

- `p_worker` в `task_rework_feedback(p_task, p_worker, p_epoch)` — **текстовый аргумент от вызывающего**,
  а не `session_user`.
- `assert_lease` проверяет `WHERE id = p_task AND state = 'leased' AND lease_owner = p_worker
  AND lease_epoch = p_epoch AND lease_until > now()` — сверяет `tasks.lease_owner` с аргументом
  `p_worker`, но **не** связывает `session_user` с `p_worker` (эту связку делает только
  `assert_worker_identity(p_worker)`, как в FIND-04 / FIND-NEW-01).
- Следствие: сессия `swarm_worker_w1` может вызвать `task_rework_feedback(t_id, 'w2', epoch)` для
  аренды `w2`, и `LS001` не сработает.

Заметка снята, потому что функция — **чистый read-only RPC**: не меняет состояние задачи, не пишет
в `events`, требует знания активной пары `(task_id, epoch)` и возвращает только диагностику ревью.
Правок не требуется. В `step2b/roles_2b.sql` переопределения нет (`grep -c` = 0).

---

## 7. Ссылки

- Пакет: `swarm-dispatcher-audit-1e71428-uncompressed.zip` (Store, без потерь; 4 XML-тома, 202 файла).
- Предыдущий срез и находки: `docs/audit/AUDIT_REPORT_swarm_dispatcher_v0_4.md`,
  `docs/audit/REAUDIT_SD_CLOSURE_v0.4.md`, `docs/audit/WAVE5_CLOSURE_v0.5.md`.
- Внешние разборы (workspace): `reviews/swarm-dispatcher/2026-10-14-architecture-critique.md`,
  `-v2.md`, `2026-10-14-infra-audit-findings.md`.
