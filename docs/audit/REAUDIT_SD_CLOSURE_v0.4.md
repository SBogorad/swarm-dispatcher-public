# Сводный re-audit: внешние замечания → статус закрытия

**Объект:** `swarm-dispatcher`
**Финальная ревизия (верифицированное состояние):**
HEAD `188e63604adf9782e1d451de57e9d2e24e4ea10e`
(ветка `feature/v0.6-distributed-worker-mvp`, +65 коммитов к `master db5ffe3`)
**Базовая ревизия (исходное состояние):** `HEAD 1909116` / `master db5ffe3`
**Дата:** 2026-10-11
**Статус:** все внешние замечания закрыты, триады RED→GREEN→RED сошлись.

---

## 0. Кросс-нумерация внешних замечаний

| Внешний источник | Замечание | Наш ID | Severity |
|---|---|---|---|
| ChatGPT SD-2026-10-11 | SD-01 обход через общую роль | F3 | HIGH |
| ChatGPT SD-2026-10-11 | SD-02 неполная проверка обязательных проверок | F2 | HIGH |
| ChatGPT SD-2026-10-11 | SD-03 отрицательный учёт токенов | F4 | HIGH |
| ChatGPT SD-2026-10-11 | SD-04 тип поля `checks` | F15 | MEDIUM |
| ChatGPT SD-2026-10-11 | SD-05 идентичность в аудите вызовов | FIND-04 | MEDIUM |

Оба внешних протокола относятся к **`master`/`1909116`**. Все пункты закрыты в
аудируемом HEAD; ниже — доказательства по каждому.

---

## 1. Прямое соответствие требований ChatGPT → артефакты

ChatGPT в §«Что нужно для окончательного протокола» перечислил, чего не хватало.
Вот где это лежит в пакете:

| Требование | Где в пакете |
|---|---|
| точный SHA проверенной ревизии | `git_metadata.txt`, `head=` в XML-томах, этот файл |
| версии PostgreSQL / Docker / окружения | `environment_versions.txt` (PG 16.15 подтверждён live `SELECT version()`) |
| результаты тестов с командами и логами | `evidence/*.txt` (триады + регрессии) |
| подтверждения исправлений по каждому пункту | `diffs/` + `evidence/` + этот файл |
| итоговый статус: открыто/исправлено/закрыто | таблица §2 |

---

## 2. Реестр закрытия

| Наш ID | Внешний | Severity | Коммит | Файл | Триада | Статус |
|---|---|---|---|---|---|---|
| F3 | SD-01 | HIGH | `418ae18` | `step2b/roles_2b.sql`, `step2/roles.sql` | RED→GREEN→RED | **закрыто** |
| F2 | SD-02 | HIGH | `33e8ef9` | `schema/schema_v0.sql` | RED→GREEN→RED | **закрыто** |
| F4 | SD-03 | HIGH | `3909166` | `schema_v0.sql`, `dispatcher.rs`, `worker.rs` | RED→GREEN→RED | **закрыто** |
| F15 | SD-04 | MEDIUM | `33e8ef9` | `schema/schema_v0.sql` | RED→GREEN→RED | **закрыто** |
| FIND-04 | SD-05 | MEDIUM | `566a4fb` | `step2/roles_patch_tool_call_audit.sql` | RED→GREEN→RED | **закрыто** |
| F16г | — | MEDIUM | `33e8ef9` | `step2/roles_patch_submit_review.sql` (удалён) | initdb | **закрыто** |
| F5 | — | HIGH | `21e11f7` | `crates/swarm-worker/src/worker.rs` | RED→GREEN→RED | **закрыто** |
| F1 | — | CRITICAL | `3ce2825` | `docker-compose.yml`, `Dockerfile`, `worker.rs` | RED→GREEN→RED | **закрыто** |
| FIND-01 | — | CRITICAL | `855daca` | `Cargo.toml`, `lib.rs`, `error.rs`, harness | символьный бан | **закрыто** |
| FIND-05 | — | HIGH | `80305d2` | `crates/swarm-worker/src/sandbox.rs` | bounded read | **закрыто** |
| FIND-08 | — | MEDIUM | `80305d2` | `tests/integration.rs`, `integration_alt.rs` | panic vs silent skip | **закрыто** |
| FIND-09 | — | LOW | `7eb8617` | `README.md`, `KNOWN_LIMITATIONS.md` | сверка MSRV | **закрыто** |
| FIND-10 | — | LOW | `7eb8617` | `docker-compose.yml`, harness | сверка наборов патчей | **закрыто** |

---

## 3. Определения закрытия по ChatGPT (условия §3 протокола)

**SD-01.** Условие: общая роль не может выполнять операции, требующие
персональной идентичности; исполнитель не может действовать от имени другого.
→ `swarm_worker nologin`; `assert_worker_identity` пропускает только точное
`session_user` или `rolsuper`, иначе `WK003`. Evidence: `wave4_F3_triad_*`,
`wave4_F3_roles_baseline.txt`.

**SD-02.** Условие: повтор одной успешной проверки не заменяет другие
обязательные; пропуски/дубликаты отклоняются.
→ Ключ `coalesce(id, type||'|'||run)`, требование `count(pass) >= req.n` по
каждому ключу. Evidence: `wave4_F2_triad_*`, `wave4_F2_legacy_step2_review_gate.txt`.

**SD-03.** Условие: отрицательные значения отклоняются; БД не хранит
отрицательный суммарный расход даже в обход приложения.
→ `p_tokens < 0` → отказ в `complete_task`; честный `fail_task`/`unblock_task`
(без overspend-хака). Evidence: `wave4_F4_triad_*`,
`wave4_F4_python_regression_GREEN.txt`.

**SD-04.** Условие: отсутствующее поле, `null`, объект, строка, пустой массив —
по документированным правилам; невалидные типы не вызывают непредусмотренную
ошибку PG.
→ `jsonb_typeof(...) is distinct from 'array'` **до** `jsonb_array_length`;
строгая проверка непустого строкового `type`. Evidence: `wave4_F15_triad_*`.

**SD-05.** Условие: `w2` не может зарегистрировать событие от имени `w1`, даже
зная `task_id` и `lease_epoch`.
→ `perform assert_worker_identity(p_worker)` до записи события; иначе `WK003`.
Evidence: `wave4_FIND04_triad_*`, `wave4_FIND04_preconditions*.txt`.

---

## 4. Замечания к самим протоколам (честно)

- **Gemini `AUDIT_PROTOCOL_v0.4.md`** указывает итоговую ревизию `7eb8617`.
  Финал после добавления re-audit-пакета — **`188e636`**; `188e636` не меняет
  `.rs/.sql` (только `docs/audit/*` + builder-скрипт), поэтому выводы в силе.
- **Версия PostgreSQL** в Gemini-протоколе — «16.15 (Alpine)». **Подтверждено
  live** (`SELECT version()` → `PostgreSQL 16.15 ... Alpine ...`), см.
  `environment_versions.txt`.
- **FIND-05:** формулировка «128 KiB на 10 MiB» корректна: cap =
  `max_output_bytes + 64 KiB`; тест даёт ~10.2 MiB вывода при cap 64 KiB и
  проверяет `≤ 128 KiB`.

---

## 5. Итоговый статус

Все замечания внешних протоколов (ChatGPT SD-01…SD-05) и внутренние находки
волны 4 (F1…F5, FIND-01/04/05/08/09/10) на ревизии `188e636` — **закрыто**,
с доказательствами. Эксплуатационная (runtime) безопасность сверх того, что
покрыто триадами, не заявляется — см. `docs/KNOWN_LIMITATIONS.md`.
