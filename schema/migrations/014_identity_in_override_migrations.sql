-- Migration 014: identity-проверка в RPC, перекрытых миграциями (FIND-NEW-02).
--
-- Дефект (найден ночной сменой 2026-10-17, подтверждён на живом PG):
--   Миграции применяются ПОСЛЕ step2b/roles_2b.sql, поэтому их
--   `create or replace` перекрывает тела воркерских обёрток. Комментарий в
--   миграции 011 это и требует («обязаны воспроизвести identity, иначе
--   create-or-replace тихо снимает защиту»), но миграция 012
--   (`task_rework_feedback`) воспроизвела только `assert_lease`.
--
--   `assert_lease(p_task, p_worker, p_epoch)` сверяет `lease_owner` с
--   ПЕРЕДАННЫМ p_worker, а не с `session_user`. Поэтому swarm_worker_w1, зная
--   (task_id, epoch) живой аренды w2, получал диагностику ревью ЧУЖОЙ задачи:
--   rework, verdict, reviewer, artifact_ref и checks. Это тот же класс, что
--   FIND-NEW-01 (fail_task/fail_task_state), но в другом RPC.
--
--   Доказательство (до фикса, disposable PG, цепочка schema_v0 → roles →
--   roles_2b → патчи → миграции):
--     PGPASSWORD=c2_w1 psql -U swarm_worker_w1 -c \
--       "select * from task_rework_feedback('<task_w2>','w2',1)"
--     → out_rework=2, verdict=changes_requested, reviewer=rev1,
--       artifact_ref=sha256:deadbeef, checks=[{...}]
--   Для сравнения `complete_task` по чужой аренде отдаёт WK003 — там identity
--   есть, и она стоит ПЕРВОЙ строкой тела.
--
-- Решение: пересоздать функцию с `assert_worker_identity(p_worker)` ПЕРВОЙ
-- содержательной строкой (до assert_lease). Это ровно доктрина F3
-- (deny-by-default): чужой воркер получает WK003, а не «пустой результат».
--
-- Почему новая миграция, а не правка 012: 012 уже применена на существующих
-- томах (миграции накатываются в порядке номеров, см. ограничение §5.3
-- KNOWN_LIMITATIONS.md), поэтому правка тела 012 не догнала бы живые контуры.
-- Новая миграция идемпотентна и применяется и на пустом, и на существующем
-- томе — при условии, что её запускают (см. §5.3: на старом томе initdb не
-- переигрывается).
--
-- Арбитр: tests/test_worker_identity_f3.py::test_f3_task_rework_feedback_foreign_worker_rejected
-- (краснеет на 012-версии, зеленеет на 014) + позитивный контроль
-- test_f3_task_rework_feedback_own_identity_passes.
-- Мутация: scripts/run_mutations_sql.sh S14 (снять identity-строку из 014).

create or replace function public.task_rework_feedback(p_task uuid, p_worker text,
                                                       p_epoch bigint)
returns table (out_rework int, out_last_review jsonb)
language plpgsql security definer set search_path = public, pg_temp as $$
declare v_rework int;
begin
  -- FIND-NEW-02: identity ПЕРВОЙ строкой. assert_lease доверяет переданному
  -- p_worker, поэтому без этой проверки чужой воркер с (task_id, epoch)
  -- читает диагностику чужой задачи.
  perform assert_worker_identity(p_worker);
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

-- Права: закрыть по умолчанию, выдать воркеру (как в 012 — повторный grant
-- идемпотентен).
revoke all on function public.task_rework_feedback(uuid, text, bigint) from public;
grant execute on function public.task_rework_feedback(uuid, text, bigint) to swarm_worker;

comment on function public.task_rework_feedback(uuid, text, bigint) is
  'FIND-CODE-02 + FIND-NEW-02: диагностика последнего ревью (rework + verdict/checks) для возвращённой задачи; identity воркера проверяется до аренды';
