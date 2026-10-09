-- Patch: grant task_status to swarm_planner.
--
-- Причина: Planner использует task_status() для идемпотентной проверки
-- существования графа по root_id. В step 2 грант был только у swarm_operator,
-- поэтому под своей ролью Planner получал "permission denied for function
-- task_status" (SQLSTATE 42501). Второй линией защиты остаётся перехват
-- unique_violation (23505) на вставке корня — он ловит гонку.
--
-- Применять ПОСЛЕ roles.sql и roles_2b.sql.
-- roles.sql не меняем — он принят в step 2.

grant execute on function task_status(int) to swarm_planner;
