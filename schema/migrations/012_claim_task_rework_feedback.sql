-- Migration 012: контур обратной связи при доработке (FIND-CODE-02).
--
-- Дефект (второй проход аудита, снимок 69f749b):
--   При `changes_requested` ревьюер пишет причину отказа в `reviews.checks`, но
--   воркер при повторном взятии задачи (`rework > 0`) её НЕ читает: `claim_task`
--   возвращал только `(id, contract, epoch, budget, spent)`. Воркер отправлял в
--   LLM ровно тот же промпт (та же температура), получал тот же ответ и снова
--   не проходил приёмку — пока не упирался в `max_rework` (`blocked`) или
--   `budget_cap` (`budget_exhausted`). Это корневая причина прод-статистики
--   `changes_requested = 39` против `approved = 16`.
--
-- Решение (вариант «отдельный RPC», а не новые колонки claim_task):
--   Сигнатуру `claim_task` НЕ меняем. Смена возвращаемого типа требует
--   `DROP FUNCTION` + перевыдачи гранта и ломает применение миграций
--   (`create or replace` не меняет row type; migration 011 пересоздаёт
--   claim_task со старой формой → «cannot change return type»). Вместо этого —
--   отдельная SECURITY DEFINER функция `task_rework_feedback(task, worker,
--   epoch)`, которую воркер вызывает сразу после выдачи аренды. Она проверяет
--   ЖИВУЮ аренду (`assert_lease`), поэтому чужую диагностику не прочитать.
--
-- Идемпотентно: create or replace + повторный grant безопасны.

create or replace function public.task_rework_feedback(p_task uuid, p_worker text,
                                                       p_epoch bigint)
returns table (out_rework int, out_last_review jsonb)
language plpgsql security definer set search_path = public, pg_temp as $$
declare v_rework int;
begin
  perform assert_lease(p_task, p_worker, p_epoch);
  select t.rework into v_rework from tasks t where t.id = p_task;
  if not found then
    raise exception 'task_not_found' using errcode = 'GR000';
  end if;
  return query
    select v_rework,
           (select jsonb_build_object(
                     'verdict', r.verdict,
                     'artifact_ref', r.artifact_ref,
                     'checks', r.checks,
                     'reviewer', r.reviewer)
              from reviews r
             where r.task_id = p_task
             order by r.id desc
             limit 1);
end $$;

-- Владелец и режим — канонический инвариант каталога
-- (step2/test_roles.py::test_catalog_invariants).
alter function public.task_rework_feedback(uuid, text, bigint) owner to swarm_owner;
alter function public.task_rework_feedback(uuid, text, bigint)
  security definer set search_path = public, pg_temp;

-- Права: закрыть по умолчанию, выдать воркеру.
revoke all on function public.task_rework_feedback(uuid, text, bigint) from public;
grant execute on function public.task_rework_feedback(uuid, text, bigint) to swarm_worker;

comment on function public.task_rework_feedback(uuid, text, bigint) is
  'FIND-CODE-02: диагностика последнего ревью (rework + verdict/checks) для возвращённой задачи';
