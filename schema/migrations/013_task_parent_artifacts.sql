-- Migration 013: DAG Handoff — артефакты родительских задач для дочерней (Итерация 3).
--
-- Дефект (найден при разборе pstack-навыка `handoff`):
--   Задачи в графе связаны зависимостями `task_deps(task_id = t2, dep_id = t1)`,
--   но воркер, взявший `t2`, НЕ видит результата `t1`. `claim_task` возвращает
--   только контракт своей задачи, а прямого `SELECT` к `tasks`/`artifacts`
--   у сервисных ролей нет (least privilege). В итоге каждая задача графа
--   решает свою часть вслепую, хотя её вход — выход родителя.
--
-- Решение (тот же приём, что в 012): отдельный SECURITY DEFINER RPC, а не
--   новые колонки `claim_task`. Смена возвращаемого типа `claim_task` требует
--   `DROP FUNCTION` + перевыдачи гранта и ломает применение миграций
--   (`create or replace` не меняет row type). Функция проверяет ЖИВУЮ аренду
--   (`assert_lease`) и identity воркера, поэтому чужую задачу не прочитать.
--
-- Контракт `kind` живёт в JSONB (`tasks.contract->>'kind'`), отдельной колонки
--   нет — отсюда `coalesce(..., 'task')` для legacy-контрактов без kind.
--
-- Идемпотентно: create or replace + повторный grant безопасны.

create or replace function public.task_parent_artifacts(p_task uuid, p_worker text,
                                                        p_epoch bigint)
returns table (parent_task_id uuid, parent_kind text, artifact_ref text,
               artifact_sha256 text)
language plpgsql security definer set search_path = public, pg_temp as $$
begin
  -- F3: identity воркера (deny-by-default) и живая аренда именно этой задачи.
  perform assert_worker_identity(p_worker);
  perform assert_lease(p_task, p_worker, p_epoch);

  return query
    select d.dep_id,
           coalesce(pt.contract->>'kind', 'task'),
           a.ref,
           -- `ref` имеет форму 'sha256:<hex>'; отдаём hex отдельно, чтобы
           -- потребителю не парсить префикс. Не-sha256 ref вернётся как есть.
           regexp_replace(a.ref, '^sha256:', '')
      from public.task_deps d
      join public.tasks pt on pt.id = d.dep_id
      join public.artifacts a on a.task_id = d.dep_id
     where d.task_id = p_task
     order by d.dep_id, a.created_at desc, a.ref asc;
end $$;

-- Владелец и режим — канонический инвариант каталога
-- (step2/test_roles.py::test_catalog_invariants).
alter function public.task_parent_artifacts(uuid, text, bigint) owner to swarm_owner;
alter function public.task_parent_artifacts(uuid, text, bigint)
  security definer set search_path = public, pg_temp;

-- Права: закрыть по умолчанию, выдать воркеру. Общая роль `swarm_worker`
-- (nologin) — носитель прав; конкретные w1/w2/w_1/gpu_1 в неё входят
-- (step2b/roles_2b.sql), поэтому перечислять их здесь не нужно.
revoke all on function public.task_parent_artifacts(uuid, text, bigint) from public;
grant execute on function public.task_parent_artifacts(uuid, text, bigint) to swarm_worker;

comment on function public.task_parent_artifacts(uuid, text, bigint) is
  'Итерация 3 (DAG handoff): последние артефакты родительских задач для дочерней — только держателю живой аренды';
