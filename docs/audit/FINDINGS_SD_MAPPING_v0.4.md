# Соответствие внешних замечаний аудита (SD-*) и исправлений на HEAD

**HEAD пакета:** `7eb8617ec1a48434ebd76787913a29ec9c55adb8`
(ветка `feature/v0.6-distributed-worker-mvp`)
**Дата:** 2026-10-11

Этот документ связывает **внешние** замечания (нумерация `SD-01…SD-05`) с
фактическими исправлениями и доказательствами в репозитории, чтобы аудитор без
доступа к GitHub мог проверить статус каждого пункта по содержимому бандла.

> **Важно:** внешние замечания относятся к состоянию `master`
> (`db5ffe3`), который **на 64 коммита позади** аудируемой ветки. Все пункты
> ниже закрыты в аудируемом HEAD. Для каждого пункта указан коммит, файл и
> evidence-лог (в `docs/audit/evidence/`).

---

## SD-01 — общая роль исполнителя допускала вход

- **Замечание:** общая роль `swarm_worker` могла логиниться; проверка
  идентичности опиралась только на префикс `swarm_worker_`, что давало путь
  обхода.
- **Статус на HEAD:** **закрыто**.
- **Что сделано:**
  - `step2/roles.sql:21` — `alter role swarm_worker nologin;` (вторая линия
    защиты: вход общей роли запрещён).
  - `step2b/roles_2b.sql:25-38` — `assert_worker_identity`: допускается
    **только** строгое совпадение `session_user = 'swarm_worker_' ||
    normalize_worker_id(p_worker)` либо явный `rolsuper`; иначе — исключение
    `worker_identity_mismatch` (SQLSTATE `WK003`). Обёртки вызывают проверку до
    операции.
- **Коммит:** `418ae18` (F3 — worker identity closed by default).
- **Evidence:** `wave4_F3_triad_RED_prefix.txt`,
  `wave4_F3_triad_GREEN_fixed.txt`, `wave4_F3_triad_RED_after_revert.txt`,
  `wave4_F3_roles_baseline.txt`, `wave4_F3_step2b_regression_GREEN.txt`.

## SD-02 — неполная проверка обязательных проверок контракта

- **Замечание:** `submit_review` не подтверждал, что каждая обязательная
  проверка сопоставлена с отдельным результатом; повтор одной успешной
  проверки мог закрыть гейт.
- **Статус на HEAD:** **закрыто**.
- **Что сделано:** `schema/schema_v0.sql`, функция `submit_review` — строгий
  **per-key** подсчёт: ключ = `coalesce(id, type || '|' || run)`, требование
  `count(status='pass') >= req.n` **по каждому ключу**. Дубликаты не заменяют
  разные обязательные проверки.
- **Коммит:** `33e8ef9` (F2 strict review-gate + F15 checks shape; F16г —
  удалён мёртвый патч `submit_review`).
- **Evidence:** `wave4_F2_triad_RED_prefix.txt`,
  `wave4_F2_triad_GREEN_fixed.txt`, `wave4_F2_triad_RED_after_revert.txt`,
  `wave4_F2_legacy_step2_review_gate.txt`, `wave4_F15_triad_*.txt`.

## SD-03 — возможность отрицательного расхода токенов

- **Замечание:** не подтверждено ограничение, запрещающее отрицательный
  `p_tokens` в `complete_task`.
- **Статус на HEAD:** **закрыто**.
- **Что сделано:** `schema/schema_v0.sql` — `complete_task`:
  - `if p_tokens < 0 then raise ...` (строки ~352);
  - начисление `spent_tokens + p_tokens` только после проверки;
  - связанный фикс F4: `fail_task`/`unblock_task` — честный учёт вместо
    overspend-хака (`unblock_task` требует `p_new_budget_cap > spent_tokens`,
    иначе `CT001`).
- **Коммит:** `3909166` (F4 honest worker failure + budget-aware unblock).
- **Evidence:** `wave4_F4_triad_*.txt`,
  `wave4_F4_python_regression_GREEN.txt`.

## SD-04 — обработка типа поля `checks`

- **Замечание:** совмещённая проверка типа JSONB и длины массива могла вызвать
  `jsonb_array_length` на не-массиве → непредусмотренная ошибка PostgreSQL.
- **Статус на HEAD:** **закрыто**.
- **Что сделано:** `schema/schema_v0.sql:142` — сначала
  `jsonb_typeof(c->'checks') is distinct from 'array'`, затем
  `jsonb_array_length(...)`; `jsonb_typeof` используется и для остальных
  веток (`needs`, `budget`, `id`, `type`).
- **Коммит:** `33e8ef9` (+ `418ae18` в составе гейта).
- **Evidence:** `wave4_F15_triad_*.txt`.

## SD-05 — идентичность при аудите вызовов инструментов

- **Замечание:** `record_tool_call` проверял аренду, но в проверенном пути не
  вызывал проверку идентичности исполнителя.
- **Статус на HEAD:** **закрыто**.
- **Что сделано:** `step2/roles_patch_tool_call_audit.sql:30` —
  `perform assert_worker_identity(p_worker);` до записи события; при
  несоответствии — `WK003`.
- **Коммит:** `566a4fb` (FIND-04).
- **Evidence:** `wave4_FIND04_triad_*.txt`,
  `wave4_FIND04_preconditions*.txt`.

---

## Итоговая таблица

| Внешнее замечание | Критичность | Статус на HEAD | Коммит |
|---|---|---|---|
| SD-01 shared role login | высокая | закрыто | `418ae18` |
| SD-02 contract checks | высокая | закрыто | `33e8ef9` |
| SD-03 negative tokens | высокая | закрыто | `3909166` |
| SD-04 checks type | средняя | закрыто | `33e8ef9` |
| SD-05 tool-call identity | средняя | закрыто | `566a4fb` |

Все закрытия подтверждены триадой доказательств RED → GREEN → RED
(регрессия обязательна: доказательство, что фикс не «вакуумный»).
