-- 016_agent_memory_feedback.sql
--
-- Живой прогон 2026-10-17 показал: `public.agent_memory` осталась ПУСТОЙ,
-- хотя механизм из 015 был на месте и синтетические тесты были зелёными.
-- Причина не в триггере как таковом, а в том, ЧТО он мог записать:
--
--   1. Условие `state = 'done' AND rework > 0` не выполнилось ни на одной
--      задаче. Два корня (`4a5307d9`, `bfd8d5dd`) сгорели в
--      `blocked (max_rework)`, один (`9482c71a`) — в `blocked
--      (contract_error: missing_input)`. Провал всех попыток — самый важный
--      урок для следующего графа, и он терялся.
--   2. Даже когда урок писался, он собирался из `checks[].id`. Ревьюер писал
--      туда `cmd|true` (заглушка планировщика), а настоящая причина отказа
--      (`detail` рубрики) в БД не попадала вовсе — только в stdout-лог.
--
-- Эта миграция чинит обе половины:
--   * `outcome` различает урок преодоления (`corrected`) и урок провала
--     (`blocked`);
--   * текст урока строится из реального `detail`, который теперь кладёт
--     `checks_for` в `reviews.checks` (см. `crates/swarm-reviewer`).
--
-- Идемпотентность: уникальный индекс `(source_task_id, outcome)`. Одна задача
-- даёт не больше одного урока каждого вида — повторный `reopen → done` или
-- повторный переход в `blocked` дублей не плодит, но урок провала и урок
-- преодоления по одной задаче сосуществуют, потому что это разные события.
--
-- Идемпотентно на уровне DDL: `add column if not exists`, `drop ... if exists`
-- перед `add constraint`, `create index if not exists`, `create or replace`.

-- ---------------------------------------------------------------------------
-- 1. Разделить виды уроков.
-- ---------------------------------------------------------------------------
alter table public.agent_memory
  add column if not exists outcome text not null default 'corrected';

alter table public.agent_memory
  drop constraint if exists agent_memory_outcome_check;
alter table public.agent_memory
  add constraint agent_memory_outcome_check
  check (outcome in ('corrected', 'blocked'));

comment on column public.agent_memory.outcome is
  'Вид урока: corrected — ошибка преодолена после реворка; blocked — задача '
  'исчерпала попытки (max_rework) или контракт неисполним (contract_error)';

create unique index if not exists uq_agent_memory_task_outcome
  on public.agent_memory (source_task_id, outcome);

-- ---------------------------------------------------------------------------
-- 2. Пересобрать консолидатор: реальный `detail` + урок провала.
-- ---------------------------------------------------------------------------
create or replace function public.consolidate_task_memory() returns trigger
language plpgsql security definer set search_path = public, pg_temp as $$
declare
  v_kind    text;
  v_outcome text;
  v_detail  text;
  v_lesson  text;
begin
  -- Урок пишется на двух переходах: ошибка преодолена (`done` после реворка)
  -- и попытки исчерпаны (`blocked`). Остальные переходы памяти не касаются.
  if new.state = 'done' and new.rework > 0 then
    v_outcome := 'corrected';
  elsif new.state = 'blocked' then
    v_outcome := 'blocked';
  else
    return new;
  end if;

  v_kind := coalesce(nullif(btrim(new.contract->>'kind'), ''), '*');

  -- Причина отказа: `detail` рубрики приоритетнее id проверок. Заглушка
  -- `cmd|true` в контрактах старых графов даёт id без смысла — если рядом
  -- есть настоящий `detail`, id в урок не попадает вовсе.
  select coalesce(
           nullif((select string_agg(btrim(c->>'detail'), '; ' order by c->>'id')
                     from jsonb_array_elements(coalesce(r.checks, '[]'::jsonb)) c
                    where c->>'status' = 'fail'
                      and nullif(btrim(c->>'detail'), '') is not null), ''),
           (select string_agg(c->>'id', ', ' order by c->>'id')
              from jsonb_array_elements(coalesce(r.checks, '[]'::jsonb)) c
             where c->>'status' = 'fail'))
    into v_detail
    from public.reviews r
   where r.task_id = new.id
     and (v_outcome = 'blocked' or r.verdict = 'changes_requested')
   order by r.id desc
   limit 1;

  if v_outcome = 'corrected' then
    v_lesson := format(
      'Задача вида %s прошла ревью только с попытки %s. Причина отказа: %s. '
      'Следующая задача этого вида должна закрыть это требование до сдачи.',
      v_kind,
      new.rework + 1,
      coalesce(v_detail, 'не указана'));
  else
    v_lesson := format(
      'Задача вида %s заблокирована (%s): попытки исчерпаны без успеха. '
      'Причина: %s. Следующая задача этого вида должна учесть это до первой сдачи.',
      v_kind,
      coalesce(nullif(btrim(new.block_reason), ''), 'без причины'),
      coalesce(v_detail, 'не указана'));
  end if;

  -- `lesson` ограничен 2000 символами в 015; длинный `detail` обрезаем здесь,
  -- а не полагаемся на ошибку check-ограничения.
  insert into public.agent_memory (kind, source_task_id, rework_count, lesson, outcome)
  values (v_kind, new.id, new.rework, left(v_lesson, 2000), v_outcome)
  on conflict (source_task_id, outcome) do nothing;

  return new;
end $$;

alter function public.consolidate_task_memory() owner to swarm_owner;
alter function public.consolidate_task_memory()
  security definer set search_path = public, pg_temp;
revoke all on function public.consolidate_task_memory() from public;

-- Триггер уже создан в 015; пересоздаём, чтобы миграция была самодостаточной.
drop trigger if exists tasks_consolidate_memory on public.tasks;
create trigger tasks_consolidate_memory
  after update of state on public.tasks
  for each row execute function public.consolidate_task_memory();

comment on function public.consolidate_task_memory() is
  'Pstack correct/reflect: урок пишется и при преодолении отказа (done после '
  'rework>0), и при провале (blocked); текст строится из detail рубрики';
