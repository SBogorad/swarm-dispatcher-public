-- Migration 003: владелец объектов llm_* — swarm_owner, а не суперпользователь.
--
-- Проблема (найдена аудитом проверки, 2026-10-13): migration 001 создавала
-- llm_calls/view/sequence от имени POSTGRES_USER (в проде — суперпользователь
-- `swarm`). Базовые таблицы переводит на `swarm_owner` файл roles.sql, но он
-- выполняется ДО миграций, поэтому созданные позже объекты llm_* остались
-- принадлежать суперпользователю.
--
-- Инвариант каталога (step2/test_roles.py::test_catalog_invariants): ни одна
-- таблица в public не должна принадлежать суперпользователю — иначе владелец
-- может отключить защитные триггеры и в обход SECURITY DEFINER-функций менять
-- состояние. Дефект был невидим, пока acceptance-harness не применял миграции
-- к тест-БД; после исправления harness тест честно покраснел.
--
-- Идемпотентно: повторный запуск безопасен (ALTER OWNER с текущим владельцем).

alter table if exists public.llm_calls owner to swarm_owner;
alter sequence if exists public.llm_calls_id_seq owner to swarm_owner;
alter view if exists public.llm_costs_daily owner to swarm_owner;
alter view if exists public.llm_costs_by_task owner to swarm_owner;

-- Индексы принадлежат таблице автоматически, но зафиксируем явно на случай,
-- если таблица пересоздавалась отдельно от индексов.
do $$
declare idx record;
begin
  for idx in
    select c.relname
    from pg_class c
    join pg_namespace n on n.oid = c.relnamespace
    where n.nspname = 'public'
      and c.relkind = 'i'
      and c.relname like 'llm_calls%'
  loop
    execute format('alter index if exists public.%I owner to swarm_owner', idx.relname);
  end loop;
end $$;

-- Права worker-ролям выданы в 002 и от смены владельца не зависят.
