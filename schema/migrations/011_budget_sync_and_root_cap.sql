-- Migration 011: бюджет ограничивает РЕАЛЬНЫЕ расходы (F8).
--
-- Дефект (аудит v0.4, docs/KNOWN_LIMITATIONS.md §5.10, F8):
--   `spent_tokens` рос ТОЛЬКО в `complete_task`. Воркер, получивший ответ LLM
--   (строка в `llm_calls` записана — деньги потрачены), но упавший/потерявший
--   аренду ДО `complete`, оставлял расход видимым в ledger и НЕвидимым в
--   `tasks.spent_tokens`. Цикл «взяли задачу → оплатили вызов → потеряли
--   аренду → reclaim_expired вернул в ready» крутился, пока
--   `attempts < budget.attempts`, и каждый оборот тратил деньги мимо бюджета.
--   Плюс `budget_cap` был только у задачи — суммарного лимита на дерево
--   (`root_id`) не существовало.
--
-- Фикс (директива владельца):
--   1. СИНХРОННЫЙ учёт: триггер AFTER INSERT ON llm_calls атомарно увеличивает
--      tasks.spent_tokens. Расход виден СРАЗУ, не дожидаясь complete.
--   2. ЕДИНЫЙ ИСТОЧНИК ИСТИНЫ: complete_task больше не прибавляет p_tokens
--      (иначе двойной счёт). Совместимость: если по задаче нет ни одной
--      строки ledger, а p_tokens > 0 (старый контракт вызова), учитываем
--      p_tokens ровно один раз.
--   3. ДВОЙНОЙ БЮДЖЕТНЫЙ ГЕЙТ: колонка tasks.root_budget_cap + проверка
--      SUM(spent_tokens) WHERE root_id = X < root_budget_cap в claim_task и
--      complete_task.
--   4. РЕКЛЕЙМ: reclaim_expired при исчерпанном бюджете (локальном или по
--      дереву) переводит задачу сразу в budget_exhausted, а не в ready.
--
-- Идемпотентно: add column if not exists, create or replace function,
-- drop trigger if exists + create. Безопасно применять повторно.
--
-- Замечание по триггеру и владельцу: триггерная функция — SECURITY DEFINER с
-- закреплённым search_path; обновление `tasks` идёт от имени swarm_owner, а не
-- от воркер-роли, поэтому INSERT в llm_calls под swarm_worker_w* не требует
-- прав на UPDATE tasks.

-- 1. Колонка root_budget_cap (см. schema_v0.sql).
alter table public.tasks
  add column if not exists root_budget_cap bigint
    check (root_budget_cap is null or root_budget_cap > 0);

comment on column public.tasks.root_budget_cap is
  'F8: суммарный бюджет дерева (root_id); у потомков денормализован триггером sync_root_budget';

-- 2. Синхронный учёт расхода: AFTER INSERT ON llm_calls -> tasks.spent_tokens.
create or replace function public.llm_calls_spend_sync() returns trigger
language plpgsql security definer set search_path = public, pg_temp as $$
begin
  -- success=false (ошибка до ответа) несёт нулевые токены — UPDATE на 0
  -- безвреден. Учитываем и prompt, и completion: это реальные потраченные
  -- токены (модель могла сжечь промпт даже при обрыве ответа).
  update tasks
     set spent_tokens = spent_tokens + coalesce(new.prompt_tokens, 0)
                                     + coalesce(new.completion_tokens, 0),
         updated_at = now()
   where id = new.task_id;
  return new;
end $$;

alter function public.llm_calls_spend_sync() owner to swarm_owner;
revoke all on function public.llm_calls_spend_sync() from public;

drop trigger if exists llm_calls_spend_sync on public.llm_calls;
create trigger llm_calls_spend_sync
  after insert on public.llm_calls
  for each row execute function public.llm_calls_spend_sync();

-- 3. Денормализация root_budget_cap на дерево (см. schema_v0.sql).
create or replace function public.sync_root_budget() returns trigger
language plpgsql security definer set search_path = public, pg_temp as $$
declare v_root bigint;
begin
  if new.root_id = new.id then
    return new;
  end if;
  select t.root_budget_cap into v_root from tasks t where t.id = new.root_id;
  new.root_budget_cap := v_root;
  return new;
end $$;

create or replace function public.propagate_root_budget() returns trigger
language plpgsql security definer set search_path = public, pg_temp as $$
begin
  if new.root_id = new.id and new.root_budget_cap is distinct from old.root_budget_cap then
    update tasks set root_budget_cap = new.root_budget_cap
     where root_id = new.id and id <> new.id;
  end if;
  return null;
end $$;

alter function public.sync_root_budget() owner to swarm_owner;
alter function public.propagate_root_budget() owner to swarm_owner;
revoke all on function public.sync_root_budget() from public;
revoke all on function public.propagate_root_budget() from public;

drop trigger if exists tasks_sync_root_budget on public.tasks;
create trigger tasks_sync_root_budget
  before insert or update of root_id, root_budget_cap on public.tasks
  for each row execute function public.sync_root_budget();

drop trigger if exists tasks_propagate_root_budget on public.tasks;
create trigger tasks_propagate_root_budget
  after update of root_budget_cap on public.tasks
  for each row execute function public.propagate_root_budget();

-- 4. add_task с явным root-бюджетом (см. schema_v0.sql).
create or replace function public.add_task(p_id uuid, p_root uuid, p_contract jsonb,
                                           p_budget_cap bigint, p_priority int default 0,
                                           p_root_budget_cap bigint default null)
returns void language plpgsql as $$
declare v_root uuid := coalesce(p_root, p_id); v_max numeric; v_cnt int; v_rbc bigint;
begin
  perform validate_contract(p_contract);
  perform lock_root(v_root);
  if p_root_budget_cap is not null and p_root_budget_cap <= 0 then
    raise exception 'root_budget_cap must be positive' using errcode = 'CT001';
  end if;
  if v_root <> p_id then
    select c_num(contract, '{budget,max_tasks}', 200) into v_max from tasks where id = v_root;
    if not found then raise exception 'root_not_found' using errcode = 'GR006'; end if;
    select count(*) into v_cnt from tasks where root_id = v_root;
    if v_cnt >= v_max then raise exception 'task_limit_exceeded' using errcode = 'GR004'; end if;
  end if;
  if v_root = p_id then
    v_rbc := p_root_budget_cap;
  else
    select root_budget_cap into v_rbc from tasks where id = v_root;
  end if;
  insert into tasks (id, root_id, contract, budget_cap, priority, root_budget_cap)
  values (p_id, v_root, p_contract, p_budget_cap, p_priority, v_rbc);
  perform append_event(p_id, 'created', 'created', 'planner',
                       jsonb_build_object('root', v_root, 'root_budget_cap', v_rbc));
end $$;

-- 5. claim_task: двойной бюджетный гейт (см. schema_v0.sql).
create or replace function public.claim_task(p_worker text, p_lease_sec int default 300)
returns table (out_task_id uuid, out_contract jsonb, out_epoch bigint,
               out_budget bigint, out_spent bigint)
language plpgsql as $$
declare v_id uuid; v_epoch bigint;
begin
  if p_lease_sec <= 0 then
    raise exception 'lease duration must be positive' using errcode = 'CT001';
  end if;
  -- F3: identity-проверка сохраняется (миграции применяются ПОСЛЕ
  -- roles_2b.sql, поэтому обязаны её воспроизвести, иначе create-or-replace
  -- тихо снимает защиту).
  perform assert_worker_identity(p_worker);
  update workers set last_seen = now() where id = p_worker;
  if not found then raise exception 'worker_unknown' using errcode = 'WK002'; end if;

  select t.id into v_id
  from tasks t join workers w on w.id = p_worker
  where t.state = 'ready'
    and t.spent_tokens < t.budget_cap
    and (t.root_budget_cap is null
         or (select coalesce(sum(s.spent_tokens), 0) from tasks s where s.root_id = t.root_id)
            < t.root_budget_cap)
    and t.attempts < c_num(t.contract, '{budget,attempts}', 3)
    and c_num(w.capabilities, '{ram_gb}', 0) >= c_num(t.contract, '{needs,min_ram_gb}', 0)
    and c_num(w.capabilities, '{models,coder_class}', 0) >= c_num(t.contract, '{needs,coder_class}', 0)
    and not exists (select 1 from task_deps d join tasks p on p.id = d.dep_id
                    where d.task_id = t.id and p.state <> 'done')
  order by (select count(*) from jsonb_array_elements_text(
              coalesce(t.contract #> '{needs,affinity_tags}', '[]'::jsonb)) x
            where x = any (w.affinity_tags)) desc,
           t.priority desc, t.created_at, t.id
  for update of t skip locked
  limit 1;

  if v_id is null then return; end if;

  update tasks
     set state = 'leased', lease_owner = p_worker, lease_epoch = lease_epoch + 1,
         lease_until = now() + make_interval(secs => p_lease_sec), updated_at = now()
   where id = v_id returning lease_epoch into v_epoch;
  perform append_event(v_id, 'claim:' || v_id || ':' || v_epoch, 'claimed', p_worker,
                       jsonb_build_object('epoch', v_epoch));
  return query select t.id, t.contract, t.lease_epoch, t.budget_cap, t.spent_tokens
               from tasks t where t.id = v_id;
end $$;

-- 5b. task_budget_state: свежее состояние бюджета для воркера (см. schema_v0.sql).
create or replace function public.task_budget_state(p_task uuid)
returns table (out_spent bigint, out_budget bigint,
               out_root_spent bigint, out_root_budget bigint)
language plpgsql stable security definer set search_path = public, pg_temp as $$
declare v_root uuid;
begin
  select root_id into v_root from tasks where id = p_task;
  if v_root is null then
    raise exception 'task_not_found' using errcode = 'GR000';
  end if;
  return query
    select t.spent_tokens, t.budget_cap,
           (select coalesce(sum(s.spent_tokens), 0)::bigint from tasks s where s.root_id = v_root),
           t.root_budget_cap
      from tasks t where t.id = p_task;
end $$;

alter function public.task_budget_state(uuid) owner to swarm_owner;
revoke all on function public.task_budget_state(uuid) from public;
grant execute on function public.task_budget_state(uuid) to swarm_worker;

-- 6. complete_task: без двойного счёта + гейт по дереву (см. schema_v0.sql).
create or replace function public.complete_task(p_task uuid, p_worker text,
                                                p_epoch bigint, p_result_ref text,
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
  -- FIND-CODE-04: фильтр по текущей эпохе (см. schema_v0.sql).
  select count(*) into v_ledger from llm_calls where task_id = p_task and lease_epoch = p_epoch;
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

-- 7. reclaim_expired: бюджет-осознанный реклейм (см. schema_v0.sql).
create or replace function public.reclaim_expired() returns int language plpgsql as $$
declare r record; v_n int := 0; v_next text; v_epoch bigint; v_root_spent bigint;
begin
  for r in
    select id, attempts, contract, spent_tokens, budget_cap, root_id, root_budget_cap
      from tasks
    where state = 'leased' and lease_until < now()
    order by id for update skip locked
  loop
    if r.spent_tokens >= r.budget_cap then
      v_next := 'budget_exhausted';
    elsif r.root_budget_cap is not null then
      select coalesce(sum(s.spent_tokens), 0) into v_root_spent
        from tasks s where s.root_id = r.root_id;
      if v_root_spent >= r.root_budget_cap then
        v_next := 'budget_exhausted';
      else
        v_next := case when r.attempts + 1 >= c_num(r.contract, '{budget,attempts}', 3)
                       then 'blocked' else 'ready' end;
      end if;
    else
      v_next := case when r.attempts + 1 >= c_num(r.contract, '{budget,attempts}', 3)
                     then 'blocked' else 'ready' end;
    end if;
    update tasks
       set state = v_next, attempts = attempts + 1, lease_epoch = lease_epoch + 1,
           lease_owner = null, lease_until = null, updated_at = now(),
           block_reason = case when v_next = 'blocked' then 'max_attempts'
                               when v_next = 'budget_exhausted' then 'budget_exhausted' end
     where id = r.id returning lease_epoch into v_epoch;
    perform append_event(r.id, 'reclaim:' || r.id || ':' || v_epoch, 'reclaimed', 'watchdog',
                         jsonb_build_object('new_state', v_next));
    v_n := v_n + 1;
  end loop;
  return v_n;
end $$;

-- 8. Владелец и режим (инвариант каталога: не суперпользователь, search_path
--    закреплён; см. migration 003 и step2/test_roles.py::test_catalog_invariants).
alter function public.add_task(uuid, uuid, jsonb, bigint, int, bigint) owner to swarm_owner;
alter function public.claim_task(text, int) owner to swarm_owner;
alter function public.complete_task(uuid, text, bigint, text, bigint) owner to swarm_owner;
alter function public.reclaim_expired() owner to swarm_owner;

alter function public.add_task(uuid, uuid, jsonb, bigint, int, bigint)
  security definer set search_path = public, pg_temp;
alter function public.claim_task(text, int)
  security definer set search_path = public, pg_temp;
alter function public.complete_task(uuid, text, bigint, text, bigint)
  security definer set search_path = public, pg_temp;
alter function public.reclaim_expired()
  security definer set search_path = public, pg_temp;

-- Гранты не меняются: подписи add_task/claim_task/complete_task/reclaim_expired
-- сохранены (add_task получил лишь новый параметр со значением по умолчанию,
-- поэтому прежний вызов с 5 аргументами продолжает работать).
grant execute on function public.add_task(uuid, uuid, jsonb, bigint, int, bigint)
  to swarm_planner;
grant execute on function public.claim_task(text, int) to swarm_worker;
grant execute on function public.complete_task(uuid, text, bigint, text, bigint) to swarm_worker;
grant execute on function public.reclaim_expired() to swarm_watchdog;
