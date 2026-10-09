-- swarm roles v0.2: горизонтальное разделение прав.
-- Запуск (суперпользователем, ПОСЛЕ schema_v0.sql):
--   psql -v ON_ERROR_STOP=1 -d swarm \
--        -v worker_pw=... -v watchdog_pw=... -v planner_pw=... \
--        -v reviewer_pw=... -v operator_pw=... -v cli_pw=... -f roles.sql
-- Скрипт идемпотентен. Ничего из того, что делает schema_v0.sql, он не меняет по смыслу:
-- только владельца объектов, режим исполнения функций и права на них.

-- 1. Владелец объектов: не суперпользователь и без входа. SECURITY DEFINER-функции
--    исполняются от его имени, поэтому даже ошибка в функции не даёт прав суперпользователя.
select 'create role swarm_owner nologin'
 where not exists (select 1 from pg_roles where rolname = 'swarm_owner') \gexec

-- 2. Сервисные роли: только вход, никаких привилегий.
select format('create role %I login nosuperuser nocreatedb nocreaterole', r)
  from unnest(array['swarm_worker','swarm_watchdog','swarm_planner',
                    'swarm_reviewer','swarm_operator','swarm_cli']) r
 where not exists (select 1 from pg_roles where rolname = r) \gexec

alter role swarm_worker   password :'worker_pw';
alter role swarm_watchdog password :'watchdog_pw';
alter role swarm_planner  password :'planner_pw';
alter role swarm_reviewer password :'reviewer_pw';
alter role swarm_operator password :'operator_pw';
alter role swarm_cli      password :'cli_pw';

-- swarm_cli: учебная роль для человека-оператора с CLI (seed, review, reopen).
-- Это членство, а не отдельные права. К функциям воркера доступа у неё нет.
grant swarm_planner, swarm_reviewer, swarm_operator to swarm_cli;

-- 2b. Чтение для оператора: прямого доступа к таблицам нет ни у кого, поэтому статус отдаётся
--     функцией. Поля ограничены (нет contract, нет артефактов), чтобы не светить лишнее.
create or replace function task_status(p_limit int default 200)
returns table (id uuid, root_id uuid, state text, lease_epoch bigint, lease_owner text,
               attempts int, rework int, block_reason text, updated_at timestamptz)
language sql stable as $$
  select t.id, t.root_id, t.state, t.lease_epoch, t.lease_owner, t.attempts, t.rework,
         t.block_reason, t.updated_at
    from tasks t order by t.updated_at desc, t.id limit least(greatest(p_limit, 1), 1000)
$$;

-- 3. Передать владение таблицам (индексы и привязанные sequence переходят вместе с таблицей)
--    и всем функциям схемы.
do $$
declare r record;
begin
  for r in select c.relname from pg_class c join pg_namespace n on n.oid = c.relnamespace
            where n.nspname = 'public' and c.relkind = 'r' loop
    execute format('alter table public.%I owner to swarm_owner', r.relname);
  end loop;
  for r in select p.oid::regprocedure as sig from pg_proc p
            join pg_namespace n on n.oid = p.pronamespace
           where n.nspname = 'public'
             and not exists (select 1 from pg_depend d where d.objid = p.oid and d.deptype = 'e') loop
    execute format('alter function %s owner to swarm_owner', r.sig);
    -- 4. Исполнение от имени владельца + защита от подмены объектов через search_path
    --    (pg_temp последним: временная таблица с именем tasks ничего не перекроет).
    execute format('alter function %s security definer set search_path = public, pg_temp', r.sig);
  end loop;
end $$;

-- 5. Закрыть всё по умолчанию.
revoke all on schema public from public;
grant usage on schema public to swarm_owner, swarm_worker, swarm_watchdog,
      swarm_planner, swarm_reviewer, swarm_operator;
revoke all on all tables    in schema public from public;
revoke all on all sequences in schema public from public;
revoke all on all functions in schema public from public;

-- 6. Выдать нужное. Внутренние функции (append_event, assert_lease, lock_root,
--    validate_contract, event_hash, c_num) не выдаются никому: журнал нельзя подделать снаружи.
grant execute on function claim_task, heartbeat, save_artifact, complete_task, register_worker
  to swarm_worker;
grant execute on function reclaim_expired, promote_ready to swarm_watchdog;
grant execute on function add_task, add_dep, promote_ready to swarm_planner;
grant execute on function submit_review, verify_chain to swarm_reviewer;
grant execute on function reopen_task, unblock_task, verify_chain, task_status to swarm_operator;
