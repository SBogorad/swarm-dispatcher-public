-- Migration 005: состояние budget_exhausted доезжает до существующих БД
-- (Issue #20).
--
-- Проблема: Issue #3 добавил состояния `budget_exhausted`/`failed` в
-- `schema/schema_v0.sql` (inline CHECK на tasks.state) и в тела функций.
-- Но `schema_v0.sql` применяется ТОЛЬКО при первой инициализации данных
-- (docker-entrypoint-initdb.d); на уже существующей БД (named volume) он не
-- перезапускается. Поэтому в проде:
--   * constraint остался старым — без `budget_exhausted`/`failed`;
--   * `complete_task` после #20 начал возвращать `budget_exhausted` → UPDATE
--     нарушал CHECK → задача падала с «database error» и зависала в `leased`.
--
-- Симптом (лог воркера): `executing complete_task(...)` → `task failed:
-- database error`, событие `completed` не пишется.
--
-- Фикс (идемпотентно):
--   1. Расширить CHECK на tasks.state до канонического набора состояний;
--   2. Переопределить `complete_task` с семантикой #3/#20 (`budget_exhausted`);
--   3. Сохранить владельца (swarm_owner) и security definer.
--
-- На свежей БД (schema_v0.sql + roles_2b.sql уже актуальны) миграция — no-op.

-- 1. Constraint на state.
--    Inline CHECK из schema_v0.sql Postgres именует `<table>_<column>_check`
--    (tasks_state_check). Пересоздаём его с полным набором состояний.
alter table public.tasks drop constraint if exists tasks_state_check;
alter table public.tasks
  add constraint tasks_state_check
  check (state in ('pending','ready','leased','review','done','blocked',
                   'failed','budget_exhausted'));

-- 2. complete_task: каноническая семантика #3 (см. Issue #20).
--    F8: тело синхронизировано с schema/schema_v0.sql — расход учитывается
--    триггером ledger (llm_calls_spend_sync), а не прибавлением p_tokens здесь
--    (иначе двойной счёт); добавлен гейт по root_budget_cap.
create or replace function public.complete_task(p_task uuid, p_worker text,
                                                p_epoch bigint,
                                                p_result_ref text,
                                                p_tokens bigint default 0)
returns text language plpgsql as $$
declare v_new text; v_spent bigint; v_cap bigint; v_root_cap bigint;
        v_root_spent bigint; v_ledger bigint;
begin
  perform assert_worker_identity(p_worker);
  perform assert_lease(p_task, p_worker, p_epoch);
  if p_tokens < 0 then
    raise exception 'tokens must be non-negative' using errcode = 'CT001';
  end if;
  if not exists (select 1 from artifacts where task_id = p_task and ref = p_result_ref) then
    raise exception 'artifact_missing' using errcode = 'AR001';
  end if;
  -- F8: единый источник истины — ledger `llm_calls` (триггер llm_calls_spend_sync).
  select count(*) into v_ledger from llm_calls where task_id = p_task;
  if v_ledger = 0 and p_tokens > 0 then
    update tasks set spent_tokens = spent_tokens + p_tokens where id = p_task;
  end if;
  select spent_tokens, budget_cap, root_budget_cap
    into v_spent, v_cap, v_root_cap from tasks where id = p_task;
  v_new := case when v_spent > v_cap then 'budget_exhausted' else 'review' end;
  if v_new <> 'budget_exhausted' and v_root_cap is not null then
    select coalesce(sum(s.spent_tokens), 0) into v_root_spent
      from tasks s where s.root_id = (select root_id from tasks where id = p_task);
    if v_root_spent > v_root_cap then
      v_new := 'budget_exhausted';
    end if;
  end if;
  update tasks
     set state = v_new, result_ref = p_result_ref,
         lease_owner = null, lease_until = null, updated_at = now(),
         block_reason = case when v_new = 'budget_exhausted' then 'budget_exhausted' end
   where id = p_task;
  perform append_event(p_task, 'complete:' || p_epoch, 'completed', p_worker,
                       jsonb_build_object('ref', p_result_ref, 'new_state', v_new, 'tokens', p_tokens));
  return v_new;
end $$;

-- 3. Владелец и security definer (как в step2b/roles_2b.sql).
--    `create or replace function` сохраняет владельца и атрибуты, но задаём
--    явно — миграция должна быть самодостаточной и на БД без roles_2b.
alter function public.complete_task(uuid, text, bigint, text, bigint)
  owner to swarm_owner;
alter function public.complete_task(uuid, text, bigint, text, bigint)
  security definer set search_path = public, pg_temp;
