-- 017_block_reason_limit.sql
--
-- Живой урок agent_memory#4 (2026-10-18, задача 76c2142c, code.verify):
-- в базе лежал урок длиной ровно 655 символов, оборванный на полуслове
-- («...ac):»). Арифметика точная: 155 символов префикса
-- («Задача вида code.verify заблокирована (contract_error: missing_input: »)
-- плюс ровно 500 символов причины. Причина обрезалась ДВАЖДЫ и теряла хвост
-- дважды:
--
--   1. `fail_task_state`/`fail_task` писали `block_reason = left(p_reason, 500)`;
--   2. консолидатор памяти (016) подставлял этот уже обрезанный `block_reason`
--      в текст урока.
--
-- Для `missing_input` хвост — самая полезная часть: там перечислено, ЧЕГО
-- именно не хватило. Обрезка превращала урок в «задача заблокирована, причина:
-- обрезано» — то есть в знание, которое нельзя применить.
--
-- Почему 2000: это ровно `CHECK (length(lesson) between 10 and 2000)` из 015.
-- Причина длиннее 2000 всё равно не поместится в урок, а 500 резало реальные
-- живые случаи. 2000 = верхняя граница полезного текста, а не круглое число.
--
-- Что делает миграция:
--   * пересоздаёт `fail_task` и `fail_task_state` с лимитом 2000;
--   * ОБЯЗАТЕЛЬНО воспроизводит `assert_worker_identity` и его порядок ДО
--     `assert_lease` — `create or replace` тихо снимает защиту, если строку
--     забыть (это уже случалось: 011/012, см.
--     tests/test_migration_identity_preserved.py);
--   * НЕ трогает `step2b/roles_2b.sql`: тот файл применяется раньше миграций,
--     а миграция побеждает. Канон в `schema/schema_v0.sql` и `roles_2b.sql`
--     обновлён тем же лимитом, чтобы drift guard
--     (`tests/test_no_duplicate_function_bodies.py`) видел идентичные тела.
--
-- Идемпотентность: `create or replace` + повторный `revoke`. Повторный прогон
-- даёт тот же результат.

-- ---------------------------------------------------------------------------
-- 1. fail_task: лимит причины 500 → 2000.
-- ---------------------------------------------------------------------------
create or replace function fail_task(p_task uuid, p_worker text, p_epoch bigint,
                                     p_result_ref text, p_reason text) returns text
language plpgsql as $$
begin
  perform assert_worker_identity(p_worker);
  perform assert_lease(p_task, p_worker, p_epoch);
  if p_result_ref is not null
     and not exists (select 1 from artifacts where task_id = p_task and ref = p_result_ref) then
    raise exception 'artifact_missing' using errcode = 'AR001';
  end if;
  update tasks
     set state = 'blocked', result_ref = coalesce(p_result_ref, result_ref),
         lease_owner = null, lease_until = null,
         block_reason = left('llm_error: ' || coalesce(p_reason, ''), 2000), updated_at = now()
   where id = p_task;
  perform append_event(p_task, 'fail:' || p_epoch, 'completed', p_worker,
                       jsonb_build_object('failed', true, 'reason', left(coalesce(p_reason, ''), 2000)));
  return 'blocked';
end $$;

alter function public.fail_task(uuid, text, bigint, text, text) owner to swarm_owner;
-- SECURITY DEFINER выставляется отдельным `alter function` в
-- `step2b/roles_2b.sql:268`, а НЕ в заголовке `create`. `create or replace`
-- сбрасывает атрибуты функции к дефолту (invoker), поэтому без этих строк
-- гейт краснеет: `test_roles.py::test_function_matrix[swarm_worker_w1]` и
-- `test_catalog_invariants` («fail_task: не SECURITY DEFINER»), а воркер
-- получает `42501` на своей же аренде. Ошибка была живой: поймана гейтом.
alter function public.fail_task(uuid, text, bigint, text, text)
  security definer set search_path = public, pg_temp;

-- ---------------------------------------------------------------------------
-- 2. fail_task_state: тот же лимит, состояние выбирает вызывающий.
-- ---------------------------------------------------------------------------
create or replace function fail_task_state(p_task uuid, p_worker text, p_epoch bigint,
                                           p_result_ref text, p_reason text, p_state text)
returns text language plpgsql as $$
begin
  perform assert_worker_identity(p_worker);
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
         block_reason = left(coalesce(p_reason, ''), 2000), updated_at = now()
   where id = p_task;
  perform append_event(p_task, 'fail:' || p_epoch, 'completed', p_worker,
                       jsonb_build_object('failed', true, 'state', p_state,
                                          'reason', left(coalesce(p_reason, ''), 2000)));
  return p_state;
end $$;

alter function public.fail_task_state(uuid, text, bigint, text, text, text) owner to swarm_owner;
alter function public.fail_task_state(uuid, text, bigint, text, text, text)
  security definer set search_path = public, pg_temp;

-- ---------------------------------------------------------------------------
-- 3. Границы лимита — явные, чтобы следующий читатель не гадал.
-- ---------------------------------------------------------------------------
comment on function public.fail_task(uuid, text, bigint, text, text) is
  'Воркерский отказ: block_reason хранится целиком до 2000 символов (017). '
  'До 017 резалось на 500, и причина missing_input теряла перечень нехватки.';

comment on function public.fail_task_state(uuid, text, bigint, text, text, text) is
  'Отказ с явным целевым состоянием: block_reason до 2000 символов (017). '
  'Лимит совпадает с CHECK на agent_memory.lesson из 015.';

revoke all on function public.fail_task(uuid, text, bigint, text, text) from public;
revoke all on function public.fail_task_state(uuid, text, bigint, text, text, text) from public;
