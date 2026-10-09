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
