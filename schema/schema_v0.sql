-- Swarm dispatcher v0: Postgres 16. Единый источник истины.
-- Правила: любая запись воркера несёт lease_epoch (fencing); переход состояния и
-- событие в журнале пишутся в одной транзакции; время только через now() БД.
-- Коды ошибок: LS001 lease_lost, AR001 artifact_missing, CT001 contract_invalid,
-- GR000 task_not_found, GR001 cycle, GR002 depth, GR003 fanout, GR004 task_limit,
-- GR005 not_done, GR006 root_not_found, GR007 cross_root, GR008 not_pending,
-- GR009 not_blocked, RV001 review_stale, RV002 self_review, RV003 checks_failed,
-- WK001 worker_invalid, WK002 worker_unknown.

create table tasks (
  id           uuid primary key,
  root_id      uuid not null references tasks(id) deferrable initially deferred,
  state        text not null default 'pending'
               check (state in ('pending','ready','leased','review','done','blocked',
                                'failed','budget_exhausted')),
  contract     jsonb not null,
  lease_epoch  bigint not null default 0,
  lease_owner  text,
  lease_until  timestamptz,
  attempts     int not null default 0,          -- сбои воркера (истекшие аренды)
  rework       int not null default 0,          -- циклы после ревью
  budget_cap   bigint not null check (budget_cap > 0),
  spent_tokens bigint not null default 0,
  result_ref   text,
  priority     int not null default 0,
  block_reason text,
  created_at   timestamptz not null default now(),
  updated_at   timestamptz not null default now(),
  -- инвариант аренды: leased <=> есть владелец и срок
  check ((state = 'leased') = (lease_owner is not null and lease_until is not null))
);
create index tasks_claim_idx  on tasks (priority desc, created_at) where state = 'ready';
create index tasks_root_idx   on tasks (root_id);
create index tasks_lease_idx  on tasks (lease_until) where state = 'leased';
create index tasks_pending_idx on tasks (id) where state = 'pending';

create table artifacts (
  task_id      uuid not null references tasks(id),
  ref          text not null,                   -- 'sha256:...'
  created_by   text not null,
  lease_epoch  bigint not null,                 -- fencing-токен создателя
  content_type text,
  size_bytes   bigint,
  storage_path text not null,
  created_at   timestamptz not null default now(),
  primary key (task_id, ref)
);
alter table tasks add foreign key (id, result_ref) references artifacts (task_id, ref);

create table task_deps (
  task_id uuid not null references tasks(id) on delete cascade,
  dep_id  uuid not null references tasks(id) on delete cascade,
  primary key (task_id, dep_id),
  check (task_id <> dep_id)
);
create index task_deps_dep_idx on task_deps (dep_id);

create table workers (
  id            text primary key,
  capabilities  jsonb not null,                 -- {"ram_gb":16,"models":{"coder_class":2}}
  affinity_tags text[] not null default '{}',
  last_seen     timestamptz not null default now()
);
create index workers_seen_idx on workers (last_seen);

create table events (
  seq       bigserial primary key,
  task_id   uuid not null references tasks(id) on delete cascade,
  event_id  text not null,
  kind      text not null check (kind in ('created','dep_added','promoted','claimed',
            'artifact_saved','completed','review_submitted','reclaimed','reopened',
            'cascade_blocked','unblocked')),
  actor     text,
  payload   jsonb not null default '{}',
  at        timestamptz not null,
  prev_hash bytea,
  hash      bytea not null,
  unique (task_id, event_id)
);
create index events_task_idx on events (task_id, seq);

create table reviews (
  id           bigserial primary key,
  task_id      uuid not null,
  artifact_ref text not null,
  reviewer     text not null,
  verdict      text not null check (verdict in ('approved','changes_requested','rejected')),
  checks       jsonb not null,                  -- [{type,status,exit_code,log_ref,duration_ms}]
  created_at   timestamptz not null default now(),
  foreign key (task_id, artifact_ref) references artifacts (task_id, ref)
);

-- ---------- вспомогательные ----------
create function c_num(c jsonb, path text[], dflt numeric) returns numeric
language sql immutable as $$ select coalesce((c #>> path)::numeric, dflt) $$;

create function lock_root(p_root uuid) returns void
language sql as $$ select pg_advisory_xact_lock(hashtextextended(p_root::text, 0)) $$;

create function event_hash(p_prev bytea, p_task uuid, p_eid text, p_kind text,
                           p_actor text, p_payload jsonb, p_at timestamptz)
returns bytea language sql immutable as $$
  select sha256(coalesce(p_prev, '\x'::bytea) || convert_to(
    p_task::text || '|' || p_eid || '|' || p_kind || '|' || coalesce(p_actor, '') || '|' ||
    p_payload::text || '|' || to_char(p_at at time zone 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US'),
    'UTF8')) $$;

-- Хэш-цепочка по задаче. Блокировка строки tasks сериализует запись, в том числе
-- первого события (у которого ещё нет строки events для блокировки).
create function append_event(p_task uuid, p_event_id text, p_kind text,
                             p_actor text, p_payload jsonb default '{}')
returns boolean language plpgsql as $$
declare v_prev bytea; v_at timestamptz := clock_timestamp();
begin
  perform 1 from tasks where id = p_task for update;
  if not found then raise exception 'task_not_found' using errcode = 'GR000'; end if;
  if exists (select 1 from events where task_id = p_task and event_id = p_event_id) then
    return false;                                   -- идемпотентность
  end if;
  select hash into v_prev from events where task_id = p_task order by seq desc limit 1;
  insert into events (task_id, event_id, kind, actor, payload, at, prev_hash, hash)
  values (p_task, p_event_id, p_kind, p_actor, p_payload, v_at, v_prev,
          event_hash(v_prev, p_task, p_event_id, p_kind, p_actor, p_payload, v_at));
  return true;
end $$;

create function verify_chain(p_task uuid) returns boolean language plpgsql stable as $$
declare e record; v_prev bytea := null;
begin
  for e in select * from events where task_id = p_task order by seq loop
    if e.prev_hash is distinct from v_prev then return false; end if;
    if e.hash <> event_hash(v_prev, e.task_id, e.event_id, e.kind, e.actor, e.payload, e.at) then
      return false;
    end if;
    v_prev := e.hash;
  end loop;
  return true;
end $$;

create function validate_contract(c jsonb) returns void language plpgsql immutable as $$
declare k text; e jsonb;
begin
  if jsonb_typeof(c->'checks') is distinct from 'array' or jsonb_array_length(c->'checks') = 0 then
    raise exception 'contract_invalid: checks required' using errcode = 'CT001';
  end if;
  -- F15: каждый элемент checks — объект с непустым строковым type; id опционален,
  -- но если задан — непустая строка. Без этого задача без type не одобряется вечно.
  for e in select * from jsonb_array_elements(c->'checks') loop
    if jsonb_typeof(e) <> 'object'
       or jsonb_typeof(e->'type') is distinct from 'string' or btrim(e->>'type') = '' then
      raise exception 'contract_invalid: every check must be an object with non-empty string type'
        using errcode = 'CT001';
    end if;
    if jsonb_exists(e, 'id') and (jsonb_typeof(e->'id') <> 'string' or btrim(e->>'id') = '') then
      raise exception 'contract_invalid: check id must be a non-empty string' using errcode = 'CT001';
    end if;
  end loop;
  foreach k in array array['min_ram_gb','coder_class'] loop
    if jsonb_exists(c->'needs', k) and jsonb_typeof(c->'needs'->k) <> 'number' then
      raise exception 'contract_invalid: needs.% must be number', k using errcode = 'CT001';
    end if;
  end loop;
  if jsonb_exists(c->'needs', 'affinity_tags') and jsonb_typeof(c->'needs'->'affinity_tags') <> 'array' then
    raise exception 'contract_invalid: needs.affinity_tags must be array' using errcode = 'CT001';
  end if;
  foreach k in array array['attempts','max_rework','max_depth','max_children','max_tasks'] loop
    if jsonb_exists(c->'budget', k) and jsonb_typeof(c->'budget'->k) <> 'number' then
      raise exception 'contract_invalid: budget.% must be number', k using errcode = 'CT001';
    end if;
  end loop;
  -- F7: диапазоны. Тип-проверки выше пропускают абсурдные значения
  -- (attempts: 1000000, min_ram_gb: 1000000). Ограничиваем жёстко.
  if jsonb_exists(c->'budget', 'attempts')
     and ((c->'budget'->>'attempts')::numeric < 1
          or (c->'budget'->>'attempts')::numeric > 100) then
    raise exception 'contract_invalid: budget.attempts out of range 1..100' using errcode = 'CT001';
  end if;
  if jsonb_exists(c->'budget', 'max_rework')
     and (c->'budget'->>'max_rework')::numeric > 50 then
    raise exception 'contract_invalid: budget.max_rework out of range 0..50' using errcode = 'CT001';
  end if;
  if jsonb_exists(c->'needs', 'min_ram_gb')
     and ((c->'needs'->>'min_ram_gb')::numeric < 0
          or (c->'needs'->>'min_ram_gb')::numeric > 1024) then
    raise exception 'contract_invalid: needs.min_ram_gb out of range 0..1024' using errcode = 'CT001';
  end if;
  if jsonb_exists(c->'needs', 'coder_class')
     and ((c->'needs'->>'coder_class')::numeric < 0
          or (c->'needs'->>'coder_class')::numeric > 100) then
    raise exception 'contract_invalid: needs.coder_class out of range 0..100' using errcode = 'CT001';
  end if;
  -- F7: реестр допустимых типов проверок и предел их числа.
  if jsonb_array_length(c->'checks') > 32 then
    raise exception 'contract_invalid: too many checks (max 32)' using errcode = 'CT001';
  end if;
  for e in select * from jsonb_array_elements(c->'checks') loop
    -- Волна 5.1: добавлен `unit`. F7 сузил реестр до cmd/test/lint/manual и тем
    -- сломал обратную совместимость: step2/test_review_gate.py использует
    -- {"type":"unit"}, и add_task начал падать с CT001. Набор типов теперь
    -- синхронизирован с Rust ALLOWED_CHECK_TYPES (N5).
    if lower(btrim(e->>'type')) not in ('cmd','test','unit','lint','manual') then
      raise exception 'contract_invalid: disallowed check type %', e->>'type' using errcode = 'CT001';
    end if;
  end loop;
end $$;

-- ---------- воркеры ----------
create function register_worker(p_id text, p_caps jsonb, p_tags text[] default '{}')
returns void language plpgsql as $$
begin
  if (p_caps ? 'ram_gb' and jsonb_typeof(p_caps->'ram_gb') <> 'number')
     or (p_caps #> '{models,coder_class}' is not null
         and jsonb_typeof(p_caps #> '{models,coder_class}') <> 'number') then
    raise exception 'worker_invalid' using errcode = 'WK001';
  end if;
  insert into workers (id, capabilities, affinity_tags, last_seen)
  values (p_id, p_caps, coalesce(p_tags, '{}'), now())
  on conflict (id) do update
    set capabilities = excluded.capabilities, affinity_tags = excluded.affinity_tags,
        last_seen = now();
end $$;

-- ---------- граф ----------
create function add_task(p_id uuid, p_root uuid, p_contract jsonb,
                         p_budget_cap bigint, p_priority int default 0)
returns void language plpgsql as $$
declare v_root uuid := coalesce(p_root, p_id); v_max numeric; v_cnt int;
begin
  perform validate_contract(p_contract);
  perform lock_root(v_root);
  if v_root <> p_id then
    select c_num(contract, '{budget,max_tasks}', 200) into v_max from tasks where id = v_root;
    if not found then raise exception 'root_not_found' using errcode = 'GR006'; end if;
    select count(*) into v_cnt from tasks where root_id = v_root;
    if v_cnt >= v_max then raise exception 'task_limit_exceeded' using errcode = 'GR004'; end if;
  end if;
  insert into tasks (id, root_id, contract, budget_cap, priority)
  values (p_id, v_root, p_contract, p_budget_cap, p_priority);
  perform append_event(p_id, 'created', 'created', 'planner', jsonb_build_object('root', v_root));
end $$;

create function add_dep(p_task uuid, p_dep uuid) returns void language plpgsql as $$
declare v_root uuid; v_root2 uuid; v_state text; v_maxd numeric; v_maxc numeric; v_len int;
begin
  if p_task = p_dep then raise exception 'cycle_detected' using errcode = 'GR001'; end if;
  select root_id, state into v_root, v_state from tasks where id = p_task;
  select root_id into v_root2 from tasks where id = p_dep;
  if v_root is null or v_root2 is null then raise exception 'task_not_found' using errcode = 'GR000'; end if;
  if v_root <> v_root2 then raise exception 'cross_root_dependency' using errcode = 'GR007'; end if;
  perform lock_root(v_root);                          -- сериализует конкурентные add_dep
  select state into v_state from tasks where id = p_task;
  if v_state <> 'pending' then raise exception 'task_not_pending' using errcode = 'GR008'; end if;

  if exists (
    with recursive d as (
      select task_id from task_deps where dep_id = p_task
      union
      select x.task_id from task_deps x join d on x.dep_id = d.task_id)
    select 1 from d where task_id = p_dep) then
    raise exception 'cycle_detected' using errcode = 'GR001';
  end if;

  select c_num(contract, '{budget,max_depth}', 10), c_num(contract, '{budget,max_children}', 20)
    into v_maxd, v_maxc from tasks where id = v_root;

  if (select count(*) from task_deps where dep_id = p_dep) >= v_maxc then
    raise exception 'fanout_exceeded' using errcode = 'GR003';
  end if;

  with recursive up(id, d) as (
    select p_dep, 0
    union
    select x.dep_id, up.d + 1 from task_deps x join up on x.task_id = up.id where up.d <= v_maxd),
  down(id, d) as (
    select p_task, 0
    union
    select x.task_id, down.d + 1 from task_deps x join down on x.dep_id = down.id where down.d <= v_maxd)
  select (select max(d) from up) + 1 + (select max(d) from down) into v_len;
  if v_len > v_maxd then raise exception 'depth_exceeded' using errcode = 'GR002'; end if;

  insert into task_deps (task_id, dep_id) values (p_task, p_dep);
  perform append_event(p_task, 'dep:' || p_dep, 'dep_added', 'planner', jsonb_build_object('dep', p_dep));
end $$;

create function promote_ready() returns int language plpgsql as $$
declare r record; v_n int := 0; v_epoch bigint;
begin
  for r in
    select t.id from tasks t
    where t.state = 'pending'
      and not exists (select 1 from task_deps d join tasks p on p.id = d.dep_id
                      where d.task_id = t.id and p.state <> 'done')
    order by t.id for update skip locked
  loop
    update tasks set state = 'ready', updated_at = now() where id = r.id
      returning lease_epoch into v_epoch;
    perform append_event(r.id, 'promote:' || r.id || ':' || v_epoch, 'promoted', 'scheduler');
    v_n := v_n + 1;
  end loop;
  return v_n;
end $$;

-- ---------- жизненный цикл воркера ----------
create function claim_task(p_worker text, p_lease_sec int default 300)
returns table (out_task_id uuid, out_contract jsonb, out_epoch bigint,
               out_budget bigint, out_spent bigint)
language plpgsql as $$
declare v_id uuid; v_epoch bigint;
begin
  if p_lease_sec <= 0 then
    raise exception 'lease duration must be positive' using errcode = 'CT001';
  end if;
  update workers set last_seen = now() where id = p_worker;
  if not found then raise exception 'worker_unknown' using errcode = 'WK002'; end if;

  select t.id into v_id
  from tasks t join workers w on w.id = p_worker
  where t.state = 'ready'
    and t.spent_tokens < t.budget_cap
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

-- Продлевает только действующую аренду; истёкшую «оживить» нельзя.
create function heartbeat(p_task uuid, p_worker text, p_epoch bigint, p_extend_sec int default 300)
returns boolean language plpgsql as $$
begin
  if p_extend_sec <= 0 then
    raise exception 'lease extension must be positive' using errcode = 'CT001';
  end if;
  update tasks set lease_until = now() + make_interval(secs => p_extend_sec), updated_at = now()
   where id = p_task and state = 'leased' and lease_owner = p_worker
     and lease_epoch = p_epoch and lease_until > now();
  if found then update workers set last_seen = now() where id = p_worker; end if;
  return found;
end $$;

create function assert_lease(p_task uuid, p_worker text, p_epoch bigint) returns void
language plpgsql as $$
begin
  perform 1 from tasks
   where id = p_task and state = 'leased' and lease_owner = p_worker
     and lease_epoch = p_epoch and lease_until > now()
   for update;
  if not found then raise exception 'lease_lost' using errcode = 'LS001'; end if;
end $$;

create function save_artifact(p_task uuid, p_worker text, p_epoch bigint, p_ref text,
                              p_storage text, p_content_type text default null,
                              p_size bigint default null)
returns void language plpgsql as $$
begin
  perform assert_lease(p_task, p_worker, p_epoch);
  insert into artifacts (task_id, ref, created_by, lease_epoch, content_type, size_bytes, storage_path)
  values (p_task, p_ref, p_worker, p_epoch, p_content_type, p_size, p_storage)
  on conflict (task_id, ref) do nothing;
  perform append_event(p_task, 'art:' || p_ref, 'artifact_saved', p_worker,
                       jsonb_build_object('ref', p_ref));
end $$;

create function complete_task(p_task uuid, p_worker text, p_epoch bigint,
                              p_result_ref text, p_tokens bigint default 0)
returns text language plpgsql as $$
declare v_new text; v_spent bigint; v_cap bigint;
begin
  perform assert_lease(p_task, p_worker, p_epoch);
  if p_tokens < 0 then
    raise exception 'tokens must be non-negative' using errcode = 'CT001';
  end if;
  if not exists (select 1 from artifacts where task_id = p_task and ref = p_result_ref) then
    raise exception 'artifact_missing' using errcode = 'AR001';
  end if;
  select spent_tokens + p_tokens, budget_cap into v_spent, v_cap from tasks where id = p_task;
  v_new := case when v_spent > v_cap then 'budget_exhausted' else 'review' end;
  update tasks
     set state = v_new, result_ref = p_result_ref, spent_tokens = v_spent,
         lease_owner = null, lease_until = null, updated_at = now(),
         block_reason = case when v_new = 'budget_exhausted' then 'budget_exhausted' end
   where id = p_task;
  perform append_event(p_task, 'complete:' || p_epoch, 'completed', p_worker,
                       jsonb_build_object('ref', p_result_ref, 'new_state', v_new, 'tokens', p_tokens));
  return v_new;
end $$;

-- F4: честный отказ воркера. Раньше не-транзиентная ошибка LLM выставлялась
-- через complete_task() с подделанным overspend (worker.rs), чтобы SQL сам
-- перевёл задачу в blocked. Это ложь в журнале (spent_tokens врёт) и хрупко:
-- зависит от '>' в complete_task. Теперь отказ — явная операция с reason.
create function fail_task(p_task uuid, p_worker text, p_epoch bigint,
                          p_result_ref text, p_reason text) returns text
language plpgsql as $$
begin
  perform assert_lease(p_task, p_worker, p_epoch);
  if p_result_ref is not null
     and not exists (select 1 from artifacts where task_id = p_task and ref = p_result_ref) then
    raise exception 'artifact_missing' using errcode = 'AR001';
  end if;
  update tasks
     set state = 'blocked', result_ref = coalesce(p_result_ref, result_ref),
         lease_owner = null, lease_until = null,
         block_reason = left('llm_error: ' || coalesce(p_reason, ''), 500), updated_at = now()
   where id = p_task;
  perform append_event(p_task, 'fail:' || p_epoch, 'completed', p_worker,
                       jsonb_build_object('failed', true, 'reason', left(coalesce(p_reason, ''), 500)));
  return 'blocked';
end $$;

-- Issue #3: отказ воркера с явным целевым состоянием.
-- `fail_task` всегда даёт 'blocked'; здесь состояние выбирает вызывающий:
--   * 'budget_exhausted' — исчерпан бюджет (токен-лимит или глобальный),
--   * 'failed'          — сбой авторизации (401/403), нужна ротация ключа.
-- Семантика аренды та же (assert_lease), причина — в block_reason и в журнале.
create function fail_task_state(p_task uuid, p_worker text, p_epoch bigint,
                                p_result_ref text, p_reason text, p_state text)
returns text language plpgsql as $$
begin
  if p_state not in ('failed', 'budget_exhausted', 'blocked') then
    raise exception 'invalid target state %', p_state using errcode = 'CT001';
  end if;
  perform assert_lease(p_task, p_worker, p_epoch);
  if p_result_ref is not null
     and not exists (select 1 from artifacts where task_id = p_task and ref = p_result_ref) then
    raise exception 'artifact_missing' using errcode = 'AR001';
  end if;
  update tasks
     set state = p_state, result_ref = coalesce(p_result_ref, result_ref),
         lease_owner = null, lease_until = null,
         block_reason = left(coalesce(p_reason, ''), 500), updated_at = now()
   where id = p_task;
  perform append_event(p_task, 'fail:' || p_epoch, 'completed', p_worker,
                       jsonb_build_object('failed', true, 'state', p_state,
                                          'reason', left(coalesce(p_reason, ''), 500)));
  return p_state;
end $$;

create function submit_review(p_task uuid, p_ref text, p_reviewer text,
                              p_verdict text, p_checks jsonb)
returns text language plpgsql security definer set search_path = public, pg_temp as $$
declare t tasks%rowtype; v_creator text; v_new text;
begin
  select * into t from tasks where id = p_task for update;
  if not found then raise exception 'task_not_found' using errcode = 'GR000'; end if;
  if t.state <> 'review' or t.result_ref is distinct from p_ref then
    raise exception 'review_stale' using errcode = 'RV001';  -- ревью строго привязано к хэшу
  end if;
  select created_by into v_creator from artifacts where task_id = p_task and ref = p_ref;
  if v_creator = p_reviewer then raise exception 'self_review' using errcode = 'RV002'; end if;
  if p_verdict = 'approved' then
    if jsonb_typeof(p_checks) <> 'array' then
      raise exception 'checks_failed' using errcode = 'RV003';
    end if;
    -- Строгий review-gate (F2, вариант A): ключ проверки =
    --   coalesce(id, type || '|' || run).
    -- Для каждого ключа число pass в отчёте >= числа требований контракта с этим
    -- ключом. Сопоставление только по type обходилось: контракт с двумя cmd
    -- (cargo test, cargo clippy) закрывался одним generic {"type":"cmd","status":"pass"}.
    -- Учёт run разводит такие проверки на разные ключи.
    if exists (
      select 1 from (
        select coalesce(c->>'id', (c->>'type') || '|' || coalesce(c->>'run', '')) as k,
               count(*) as n
          from jsonb_array_elements(coalesce(t.contract->'checks', '[]'::jsonb)) c
         group by 1) req
       where (select count(*) from jsonb_array_elements(p_checks) p
               where coalesce(p->>'id', (p->>'type') || '|' || coalesce(p->>'run', '')) = req.k
                 and p->>'status' = 'pass') < req.n
    ) then
      raise exception 'checks_failed' using errcode = 'RV003';
    end if;
  end if;
  insert into reviews (task_id, artifact_ref, reviewer, verdict, checks)
  values (p_task, p_ref, p_reviewer, p_verdict, p_checks);

  if p_verdict = 'approved' then v_new := 'done';
  elsif p_verdict = 'changes_requested' then
    v_new := case when t.rework + 1 > c_num(t.contract, '{budget,max_rework}', 3)
                  then 'blocked' else 'ready' end;
  else v_new := 'blocked';
  end if;

  update tasks
     set state = v_new,
         rework = rework + case when p_verdict = 'changes_requested' then 1 else 0 end,
         lease_epoch = lease_epoch + case when v_new = 'ready' then 1 else 0 end,
         block_reason = case when v_new = 'blocked' then
                        case when p_verdict = 'rejected' then 'rejected' else 'max_rework' end end,
         updated_at = now()
   where id = p_task;
  perform append_event(p_task, 'review:' || p_ref || ':' || p_reviewer, 'review_submitted',
                       p_reviewer, jsonb_build_object('verdict', p_verdict, 'new_state', v_new));
  if v_new = 'done' then perform promote_ready(); end if;
  return v_new;
end $$;

-- ---------- сторож и операторские действия ----------
-- attempts растёт только здесь; epoch+1 отсекает зомби-воркера немедленно.
create function reclaim_expired() returns int language plpgsql as $$
declare r record; v_n int := 0; v_next text; v_epoch bigint;
begin
  for r in
    select id, attempts, contract from tasks
    where state = 'leased' and lease_until < now()
    order by id for update skip locked
  loop
    v_next := case when r.attempts + 1 >= c_num(r.contract, '{budget,attempts}', 3)
                   then 'blocked' else 'ready' end;
    update tasks
       set state = v_next, attempts = attempts + 1, lease_epoch = lease_epoch + 1,
           lease_owner = null, lease_until = null, updated_at = now(),
           block_reason = case when v_next = 'blocked' then 'max_attempts' end
     where id = r.id returning lease_epoch into v_epoch;
    perform append_event(r.id, 'reclaim:' || r.id || ':' || v_epoch, 'reclaimed', 'watchdog',
                         jsonb_build_object('new_state', v_next));
    v_n := v_n + 1;
  end loop;
  return v_n;
end $$;

create function reopen_task(p_task uuid, p_actor text) returns int
language plpgsql as $$
declare r record; v_n int := 0; v_epoch bigint; v_root uuid;
begin
  select root_id into v_root from tasks where id = p_task;
  if v_root is null then raise exception 'task_not_found' using errcode = 'GR000'; end if;
  perform lock_root(v_root);
  update tasks set state = 'ready', lease_epoch = lease_epoch + 1, lease_owner = null,
         lease_until = null, block_reason = null, updated_at = now()
   where id = p_task and state = 'done' returning lease_epoch into v_epoch;
  if not found then raise exception 'not_done' using errcode = 'GR005'; end if;
  perform append_event(p_task, 'reopen:' || p_task || ':' || v_epoch, 'reopened', p_actor);
  for r in
    with recursive sub as (
      select task_id from task_deps where dep_id = p_task
      union
      select d.task_id from task_deps d join sub s on d.dep_id = s.task_id)
    select t.id from tasks t
    where t.id in (select task_id from sub) and t.state in ('ready','leased','review','done')
    order by t.id for update
  loop
    update tasks set state = 'blocked', lease_owner = null, lease_until = null,
           lease_epoch = lease_epoch + 1, block_reason = 'parent_reopened', updated_at = now()
     where id = r.id returning lease_epoch into v_epoch;
    perform append_event(r.id, 'cascade:' || r.id || ':' || v_epoch, 'cascade_blocked', p_actor,
                         jsonb_build_object('parent', p_task));
    v_n := v_n + 1;
  end loop;
  return v_n;
end $$;

-- F4: восстановимый unblock. Раньше задача из blocked возвращалась без учёта
-- бюджета: если он уже исчерпан, unblock не помогал — задача снова уходила в
-- blocked на первом же complete. Теперь оператор может явно поднять
-- budget_cap, а при исчерпанном бюджете без явного cap функция отказывает
-- (CT001) — вместо тихого нерабочего разблокирования.
create function unblock_task(p_task uuid, p_actor text, p_new_budget_cap bigint default null)
returns text language plpgsql as $$
declare v_epoch bigint; t tasks%rowtype;
begin
  select * into t from tasks where id = p_task
    and state in ('blocked', 'failed', 'budget_exhausted') for update;
  if not found then raise exception 'not_blocked' using errcode = 'GR009'; end if;
  if p_new_budget_cap is not null then
    if p_new_budget_cap <= t.spent_tokens then
      raise exception 'budget cap must exceed spent_tokens' using errcode = 'CT001';
    end if;
  elsif t.spent_tokens >= t.budget_cap then
    raise exception 'budget_exhausted: pass p_new_budget_cap' using errcode = 'CT001';
  end if;
  update tasks set state = 'pending', attempts = 0, rework = 0, block_reason = null,
         lease_epoch = lease_epoch + 1, budget_cap = coalesce(p_new_budget_cap, budget_cap),
         updated_at = now()
   where id = p_task returning lease_epoch into v_epoch;
  perform append_event(p_task, 'unblock:' || p_task || ':' || v_epoch, 'unblocked', p_actor,
                       jsonb_build_object('new_budget_cap', p_new_budget_cap));
  perform promote_ready();
  return (select state from tasks where id = p_task);
end $$;
