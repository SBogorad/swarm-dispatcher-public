-- swarm roles 2b v0.2: идентификация воркера.
-- Запуск ПОСЛЕ schema_v0.sql и roles.sql (суперпользователем):
--   psql -v ON_ERROR_STOP=1 -d swarm \
--     -v w1_pw=... -v w2_pw=... \
--     -f step2b/roles_2b.sql
--
-- Задача: закрыть самопровозглашённый worker_id.
-- Проверка активна только для ролей swarm_worker_*; другие роли не затронуты.
--
-- Код ошибки: WK003 worker_identity_mismatch.
--
-- ВАЖНО: schema_v0.sql не меняется. Проверка добавляется через
-- CREATE OR REPLACE. Следующая итерация схемы (v0.3) должна включить
-- её в тела функций.

-- ============================================================
-- 1. Нормализация и проверка идентичности
-- ============================================================

create or replace function normalize_worker_id(p_worker text)
returns text language sql immutable as $$
  select lower(regexp_replace(p_worker, '[-. ]', '_', 'g'))
$$;

create or replace function assert_worker_identity(p_worker text)
returns void language plpgsql as $$
declare v_expected text;
begin
  -- F3: идентификация закрыта по умолчанию (deny-by-default).
  -- Пропуск ТОЛЬКО при строгом совпадении session_user с ожидаемой
  -- per-worker ролью либо для суперпользователя. Раньше проверка была
  -- завёрнута в `if session_user like 'swarm_worker_%'`, поэтому под
  -- общей ролью swarm_worker (единственной, кому выданы обёртки) она
  -- молча ничего не проверяла — bypass. См. reference_fix_wave4.sql (F3).
  v_expected := 'swarm_worker_' || normalize_worker_id(p_worker);
  if session_user::text = v_expected then return; end if;
  if (select rolsuper from pg_roles where rolname = session_user) then return; end if;
  raise exception 'worker_identity_mismatch: session_user=% expected=%', session_user, v_expected
    using errcode = 'WK003';
end $$;

alter function normalize_worker_id(text) owner to swarm_owner;
alter function assert_worker_identity(text) owner to swarm_owner;
alter function normalize_worker_id(text) security definer set search_path = public, pg_temp;
alter function assert_worker_identity(text) security definer set search_path = public, pg_temp;

revoke all on function normalize_worker_id(text) from public;
revoke all on function assert_worker_identity(text) from public;
-- assert_worker_identity — не выдаём никому; вызывается только из обёрток.

-- ============================================================
-- 2. Переопределение пяти функций воркера с проверкой
-- ============================================================

-- 2.1. register_worker
create or replace function register_worker(p_id text, p_caps jsonb, p_tags text[] default '{}')
returns void language plpgsql as $$
begin
  perform assert_worker_identity(p_id);
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

-- 2.2. claim_task
create or replace function claim_task(p_worker text, p_lease_sec int default 300)
returns table (out_task_id uuid, out_contract jsonb, out_epoch bigint,
               out_budget bigint, out_spent bigint)
language plpgsql as $$
declare v_id uuid; v_epoch bigint;
begin
  if p_lease_sec <= 0 then
    raise exception 'lease duration must be positive' using errcode = 'CT001';
  end if;
  perform assert_worker_identity(p_worker);
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

-- 2.3. heartbeat
create or replace function heartbeat(p_task uuid, p_worker text, p_epoch bigint,
                                     p_extend_sec int default 300)
returns boolean language plpgsql as $$
begin
  if p_extend_sec <= 0 then
    raise exception 'lease extension must be positive' using errcode = 'CT001';
  end if;
  perform assert_worker_identity(p_worker);
  update tasks set lease_until = now() + make_interval(secs => p_extend_sec),
                   updated_at = now()
   where id = p_task and state = 'leased' and lease_owner = p_worker
     and lease_epoch = p_epoch and lease_until > now();
  if found then update workers set last_seen = now() where id = p_worker; end if;
  return found;
end $$;

-- 2.4. save_artifact
create or replace function save_artifact(p_task uuid, p_worker text, p_epoch bigint,
                                         p_ref text, p_storage text,
                                         p_content_type text default null,
                                         p_size bigint default null)
returns void language plpgsql as $$
begin
  perform assert_worker_identity(p_worker);
  perform assert_lease(p_task, p_worker, p_epoch);
  insert into artifacts (task_id, ref, created_by, lease_epoch, content_type, size_bytes, storage_path)
  values (p_task, p_ref, p_worker, p_epoch, p_content_type, p_size, p_storage)
  on conflict (task_id, ref) do nothing;
  perform append_event(p_task, 'art:' || p_ref, 'artifact_saved', p_worker,
                       jsonb_build_object('ref', p_ref));
end $$;

-- 2.5. complete_task
create or replace function complete_task(p_task uuid, p_worker text, p_epoch bigint,
                                         p_result_ref text, p_tokens bigint default 0)
returns text language plpgsql as $$
declare v_new text; v_spent bigint; v_cap bigint;
begin
  perform assert_worker_identity(p_worker);
  perform assert_lease(p_task, p_worker, p_epoch);
  if p_tokens < 0 then
    raise exception 'tokens must be non-negative' using errcode = 'CT001';
  end if;
  if not exists (select 1 from artifacts where task_id = p_task and ref = p_result_ref) then
    raise exception 'artifact_missing' using errcode = 'AR001';
  end if;
  select spent_tokens + p_tokens, budget_cap into v_spent, v_cap from tasks where id = p_task;
  v_new := case when v_spent > v_cap then 'blocked' else 'review' end;
  update tasks
     set state = v_new, result_ref = p_result_ref, spent_tokens = v_spent,
         lease_owner = null, lease_until = null, updated_at = now(),
         block_reason = case when v_new = 'blocked' then 'budget_exhausted' end
   where id = p_task;
  perform append_event(p_task, 'complete:' || p_epoch, 'completed', p_worker,
                       jsonb_build_object('ref', p_result_ref, 'new_state', v_new, 'tokens', p_tokens));
  return v_new;
end $$;

-- Владелец и режим — для новых тел
alter function register_worker(text, jsonb, text[])          owner to swarm_owner;
alter function claim_task(text, int)                          owner to swarm_owner;
alter function heartbeat(uuid, text, bigint, int)             owner to swarm_owner;
alter function save_artifact(uuid, text, bigint, text, text, text, bigint) owner to swarm_owner;
alter function complete_task(uuid, text, bigint, text, bigint) owner to swarm_owner;

alter function register_worker(text, jsonb, text[])          security definer set search_path = public, pg_temp;
alter function claim_task(text, int)                          security definer set search_path = public, pg_temp;
alter function heartbeat(uuid, text, bigint, int)             security definer set search_path = public, pg_temp;
alter function save_artifact(uuid, text, bigint, text, text, text, bigint) security definer set search_path = public, pg_temp;
alter function complete_task(uuid, text, bigint, text, bigint) security definer set search_path = public, pg_temp;

-- ============================================================
-- 3. Роли для тестов
-- ============================================================
-- Соответствие worker_id → роль:
--   'w1'    → swarm_worker_w1      (без изменений)
--   'w2'    → swarm_worker_w2      (без изменений)
--   'w-1'   → swarm_worker_w_1     (нормализация: дефис → подчёркивание)
--   'GPU-1' → swarm_worker_gpu_1   (нормализация: регистр + дефис)
select 'create role swarm_worker_w1 login password ' || quote_literal(:'w1_pw')
 where not exists (select 1 from pg_roles where rolname = 'swarm_worker_w1') \gexec
select 'create role swarm_worker_w2 login password ' || quote_literal(:'w2_pw')
 where not exists (select 1 from pg_roles where rolname = 'swarm_worker_w2') \gexec
select 'create role swarm_worker_w_1 login password ' || quote_literal(:'w_1_pw')
 where not exists (select 1 from pg_roles where rolname = 'swarm_worker_w_1') \gexec
select 'create role swarm_worker_gpu_1 login password ' || quote_literal(:'gpu_1_pw')
 where not exists (select 1 from pg_roles where rolname = 'swarm_worker_gpu_1') \gexec

alter role swarm_worker_w1    password :'w1_pw';
alter role swarm_worker_w2    password :'w2_pw';
alter role swarm_worker_w_1   password :'w_1_pw';
alter role swarm_worker_gpu_1 password :'gpu_1_pw';

grant swarm_worker to swarm_worker_w1;
grant swarm_worker to swarm_worker_w2;
grant swarm_worker to swarm_worker_w_1;
grant swarm_worker to swarm_worker_gpu_1;

-- F4: fail_task — честный отказ воркera. Грант выдаётся ТОЛЬКО per-worker
-- login-ролям, НЕ общей swarm_worker: общая роль — не identity (см. F3),
-- и обёртки отказа должен вызывать только реальный воркер.
grant execute on function fail_task(uuid, text, bigint, text, text)
  to swarm_worker_w1, swarm_worker_w2, swarm_worker_w_1, swarm_worker_gpu_1;

-- ============================================================
-- 3b. Defense-in-depth: каждой per-worker роли запрещён SET на
--     чужую роль (включая саму общую swarm_worker).
--
--     Без этого PG16 по умолчанию выдаёт set_option = true при
--     grant, и компрометация одной роли позволяет SET ROLE на
--     другую. Защита identity и так держится на session_user
--     (см. assert_worker_identity), но мы убираем и сам механизм.
--
--     Синтаксис проверен на PostgreSQL 16.15:
--       revoke set option for <role> from <role>
--     (не "REVOKE SET ON ROLE" — такого варианта в PG16 нет).
-- ============================================================
revoke set option for swarm_worker from swarm_worker_w1;
revoke set option for swarm_worker from swarm_worker_w2;
revoke set option for swarm_worker from swarm_worker_w_1;
revoke set option for swarm_worker from swarm_worker_gpu_1;

-- Перекрёстный запрет: ни одна worker-роль не имеет SET на любую
-- другую worker-роль (матрица N x N, включая саму себя).
revoke set option for swarm_worker_w1    from swarm_worker_w2, swarm_worker_w_1, swarm_worker_gpu_1;
revoke set option for swarm_worker_w2    from swarm_worker_w1, swarm_worker_w_1, swarm_worker_gpu_1;
revoke set option for swarm_worker_w_1   from swarm_worker_w1, swarm_worker_w2, swarm_worker_gpu_1;
revoke set option for swarm_worker_gpu_1 from swarm_worker_w1, swarm_worker_w2, swarm_worker_w_1;
