-- Дополнения к схеме для демо-контура. Идемпотентно; монтируется после schema_v0.sql.
--
-- task_status(): витрина чтения для сервисных ролей. В боевом контуре её
-- определяет step2/roles.sql (вместе с ролями). Здесь она нужна, чтобы к
-- демо-БД можно было подключить и настоящие Rust-бинарники (swarm-planner,
-- swarm-cli) — они используют task_status() для чтения статусов.
create or replace function task_status(p_limit int default 200)
returns table (id uuid, root_id uuid, state text, lease_epoch bigint, lease_owner text,
               attempts int, rework int, block_reason text, updated_at timestamptz)
language sql stable as $$
  select t.id, t.root_id, t.state, t.lease_epoch, t.lease_owner, t.attempts, t.rework,
         t.block_reason, t.updated_at
    from tasks t order by t.updated_at desc, t.id limit least(greatest(p_limit, 1), 1000)
$$;
