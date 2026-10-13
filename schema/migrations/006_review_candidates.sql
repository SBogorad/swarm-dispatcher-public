-- Migration 006: review_candidates для роли swarm_reviewer (Issue #26).
--
-- Проблема: в прод-контуре нет компонента, закрывающего `review -> done`.
-- Ревьюеру нужны кандидаты (задачи в состоянии `review`) и путь к артефакту
-- исполнителя, но `swarm_reviewer` имеет только EXECUTE на submit_review/
-- verify_chain и НЕ имеет SELECT на `tasks`/`artifacts` (least privilege).
--
-- Решение: SECURITY DEFINER функция `review_candidates(limit)`, отдающая
-- ровно то, что нужно ревьюеру: id, result_ref, contract, storage_path,
-- created_by. Грант — только `swarm_reviewer`.
--
-- Идемпотентно: create or replace + повторный grant безопасны.

create or replace function public.review_candidates(p_limit int default 10)
returns table (id uuid, result_ref text, contract jsonb,
               storage_path text, created_by text)
language sql stable as $$
  select t.id, t.result_ref, t.contract, a.storage_path, a.created_by
    from tasks t
    left join artifacts a on a.task_id = t.id and a.ref = t.result_ref
   where t.state = 'review'
   order by t.updated_at asc, t.id
   limit least(greatest(p_limit, 1), 100)
$$;

-- Владелец и режим исполнения — как у остальных витрин (step2/roles.sql §3-4).
alter function public.review_candidates(int) owner to swarm_owner;
alter function public.review_candidates(int)
  security definer set search_path = public, pg_temp;

-- Права: закрыть по умолчанию, выдать только ревьюеру.
revoke all on function public.review_candidates(int) from public;
grant execute on function public.review_candidates(int) to swarm_reviewer;

comment on function public.review_candidates(int) is
  'Issue #26: кандидаты на ревью (id, result_ref, contract, storage_path, created_by) для swarm_reviewer';
