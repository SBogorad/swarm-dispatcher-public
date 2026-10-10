# ТЗ волны 4 — закрытие F1–F5 (аудит `swarm-dispatcher` от 2026-10-11)

**Вход:** отчёт аудитора `docs/audit/AUDIT_REPORT_swarm_dispatcher_v0_4.md`
(Claude, песочница; head `1909116`).
**Эталон:** `reference_fix_wave4.sql` (от аудитора) — только доказательство
проходимости тестов, **не копировать поверх**.
**Правило приёмки:** каждый дефект закрыт отдельным коммитом + тест-арбитр,
который **механически** доказан: красный на коде до → зелёный после → красный
после отката правки.

---

## Шаг 0 (ОБЯЗАТЕЛЬНО до правок): свести дубли тел функций

Инвентаризация на `1909116` (проверено на живом PG 16.15 в compose-порядке):

| Функция | Источник A | Источник B | Кто побеждает |
|---|---|---|---|
| `claim_task` | `schema/schema_v0.sql:257` | `step2b/roles_2b.sql:69` | **2b** (с `assert_worker_identity`) |
| `complete_task` | `schema/schema_v0.sql:334` | `step2b/roles_2b.sql:144` | **2b** (с `assert_worker_identity`) |
| `heartbeat` | `schema/schema_v0.sql:298` | `step2b/roles_2b.sql:111` | **2b** (с `assert_worker_identity`) |
| `submit_review` | `schema/schema_v0.sql:358` | `step2/roles_patch_submit_review.sql:25` | schema (патч **не смонтирован** в compose) |
| `assert_worker_identity` | `step2b/roles_2b.sql:25` | — | единственный источник |
| `unblock_task`, `validate_contract`, `promote_ready`, `append_event`, `c_num` | `schema/schema_v0.sql` | — | единственный источник |

**Действие:** оставить ровно один источник на функцию.
Рекомендация: тела `claim_task`/`complete_task`/`heartbeat` живут в
`roles_2b.sql` (там identity-ограничение, которое должно быть рядом с
проверкой); в `schema_v0.sql` эти три тела **не дублировать** (либо оставить
базовую версию без identity и явно задокументировать, что 2b её перекрывает).
**НЕ удалять** `roles_2b.sql`: он держит per-worker роли.

**Тест-арбитр шага 0:** SQL-проверка «все определения функции совпадают» либо
checksum тела из `pg_proc` для каждой дублированной функции против эталона.
Плюс приёмка: порядок миграций без `roles_patch_submit_review.sql` не ломается.

---

## F2 [HIGH] — review-gate сверяет по `type`, а не по сути

**Файл:** `schema/schema_v0.sql` (тело `submit_review`, блок gate).
**Дефект (воспроизведён на PG 16.15):** контракт с двумя `cmd`-проверками
(`cargo test`, `cargo clippy`) закрывается **одним** `{"type":"cmd","status":"pass"}`.
**Правка:** ключ сравнения = `coalesce(check->>'id', check->>'type')`; для каждого
ключа в отчёте должно быть не меньше `pass`, чем требований контракта с этим ключом.
**Решение владельца (2026-10-11): `id` делать ОБЯЗАТЕЛЬНЫМ** в `validate_contract`
(вариант «а»), иначе через полгода F2 откроется снова. Эталон аудитора
оставлял `id` опциональным — здесь мы сознательно строже эталона.
Эталон: `reference_fix_wave4.sql` (блок F2).
**Тест-арбитр:** `test_review_gate.py::test_two_checks_same_type_need_two_passes`
(две `cmd` → один `pass` = RV003; два `pass` = done). **Плюс** кейс с `id`.
**НЕ трогать:** `self_review`/`review_stale` ветки (F20 подтвердил корректность).

## F15 [MEDIUM] — `validate_contract` принимает checks любой формы

**Файл:** `schema/schema_v0.sql:139`.
**Дефект (воспроизведён):** `checks:["pytest",42]` и `[{"run":"cargo test"}]`
принимаются; задача без `type` затем **не может** быть одобрена (RV003 вечно).
**Правка:** каждый элемент `checks` — объект с непустым строковым `type`;
опционально `id` — непустая строка. Эталон: блок F15.
**Тест-арбитр:** `test_review_gate.py::test_contract_rejects_non_object_check`
и `::test_contract_rejects_check_without_type`.
**Связь:** F2 и F15 меняют одни и те же функции — делать вместе.

## F3 [HIGH] — ПЕРЕПРОВЕРИТЬ: на `1909116` уже закрыт

**Файл:** `step2/roles.sql:21` (`alter role swarm_worker nologin`) +
`step2b/roles_2b.sql:29` (проверка `session_user like 'swarm_worker_%'`).
**Статус:** на живом стеке `swarm_worker.rolcanlogin = f`. Аудитор, вероятно,
тестировал более раннее состояние. **Действие:** воспроизвести прогон аудитора
(логин под `swarm_worker` → `claim_task('victim')`) и зафиксировать результат.
Если вход невозможен — F3 закрыт, добавить регресс-тест и закрыть.
Если воспроизводится — применить эталон: `session_user = 'swarm_worker_'||id`
безусловно + `alter role swarm_worker nologin`; перевести тесты step2 с общей
роли на per-worker.

## F4 [HIGH] — сбой LLM «хоронит» задачу навсегда

**Файл:** `crates/swarm-worker/src/worker.rs:181-195` (overspend-хак);
`step2b/roles_2b.sql` (`unblock_task` живёт в `schema_v0.sql:469`).
**Дефект (воспроизведён):** воркер пишет `spent = cap+1`, `block_reason=budget_exhausted`;
`unblock_task` → `ready`, но `claim_task` требует `spent < cap` → задача не выдаётся.
**Правка:**
- новая `fail_task(p_task, p_worker, p_epoch, p_result_ref, p_reason)` — честный
  отказ без искажения учёта; гранить **per-worker** ролям (`swarm_worker_w*`),
  НЕ общей `swarm_worker` (после F3 она `nologin`);
- `unblock_task(p_task, p_actor, p_new_budget_cap default null)` — умеет
  поднимать бюджет; запись в журнал событий (`append_event`);
- воркер при исчерпании ретраев LLM вызывает `fail_task`, а не overspend.
**ВНИМАНИЕ:** `unblock_task` меняет сигнатуру → проверить Rust-клиент и тесты.
**Тест-арбитр:** `tests/test_v0.py::test_llm_failure_block_then_unblock_then_claim`.
Эталон: блок F4.

## F5 [HIGH] — потеря аренды не доходит до воркера; heartbeat хрупок

**Файл:** `crates/swarm-worker/src/worker.rs:101-141`, `:179`.
**Дефект (статически):** (а) heartbeat при `Ok(false)` только `break`, общего флага
нет, основной поток работает дальше и платит; (б) если heartbeat не стартовал —
работа без аренды вслепую; (в) `Err` логируется и повторяется без реконнекта;
(г) ветка `Outcome::LeaseLost` недостижима (разбирает результат LLM).
**Правка:** `Arc<AtomicBool> lease_lost`, проверяемый между шагами и перед
дорогими операциями; реконнект heartbeat; если поток не стартовал — сдавать аренду.
**Тест-арбитр:** `worker_integration.rs::heartbeat_lease_lost_aborts_work`
(аренда **≥ 3 с**, т.к. интервал = `lease/3` с полом 1 с; `process_lease(..., Some(dsn))`).
Сейчас **все** вызовы `process_lease` идут с `None` (F14а) — это и надо закрыть.
**Дополнительно (владелец, 2026-10-11):** тест обязан **явно** создавать ситуацию
«heartbeat не стартовал» (`Some(dsn)` с искусственной ошибкой подключения) и
проверять, что задача **не уходит в работу вслепую**. Без этого F5 закрыт формально.

---

## F1 [CRITICAL] — артефакты в tmpfs недоступны потребителям

**Файл:** `docker-compose.yml:75-77`, `:102-104`; `crates/swarm-worker/src/worker.rs:157-169`.
**Правка (решение владельца):** общий **именованный volume** для артефактов
вместо per-container tmpfs; `storage_path` — URI, доступный потребителю.
S3 — **честный пробел**: в `KNOWN_LIMITATIONS` записать, что volume работает
только на одном хосте, а для distributed-ветки нужен S3.
**Тест-арбитр:** выполнить задачу до `done`, `docker restart` воркера, прочитать
артефакт по `storage_path` **из другого контейнера**, и **сверить содержимое с
хэшем/записью в `artifacts`** (не только «файл существует» — иначе volume
закроется, а runtime-невидимость останется). Падает на текущем compose, зеленеет с volume.

---

## Явные запреты

- **НЕ подключать shell tools** (`agent_loop`, `sandbox`) к бинарю воркера,
  пока не закрыты F6, F7, F11, F12.
- **Бан в КОДЕ, а не в переписке** (владелец, 2026-10-11): `agent_loop` не
  должен компилироваться в бинарь воркера — cfg-флаг/feature-gate, чтобы
  «случайно подключить» было нельзя. Иначе правило живёт только в чате.
- Отчёт аудитора — внешний документ (закоммичен как доказательная база).

## Сопутствующие (владелец, 2026-10-11)

- **F16(a)** — доки «Planner отсутствует»/«Worker — заглушка» (v0.1) при живом
  коде: забрать в F2-коммит, если успеваем; иначе сразу отдельным мелким коммитом.
  Это не косметика — источник будущего ложного отчёта аудитора.
- **`promote_ready`** при `changes_requested → ready`: проверить на живом стеке,
  нужен ли проход через `pending`.
- **F3 `nologin`** — координированное обновление, если старые worker-бинарники
  в обороте.
- **F4 SQL и Rust — в ОДНОМ релизе**, окно недопустимо: пока `worker.rs` шлёт
  `complete(overspend)`, SQL-часть «честного отказа» не работает. Если рассинхрон
  неизбежен — `unblock_task(..., p_reset_fake_spend bool default false)` с записью
  в events; по умолчанию `false`, обнуление — инструмент, не норма.
- **Evidence-гигиена**: перед коммитом прогонять логи через grep на
  `password|dsn|PGDSN|SWARM_PW_|secret|token=`.

## Порядок исполнения

```
шаг 0 (дубли) → F3 (перепроверить/закрыть) → F2+F15 (вместе) → F4 → F5 → F1
```
Каждый — отдельный коммит, gate зелёный, тест-арбитр механически доказан.
