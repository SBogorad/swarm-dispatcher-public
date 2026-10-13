-- Migration 004: стоимость в numeric(18,10) вместо numeric(12,6) (Issue #14).
--
-- Проблема: numeric(12,6) хранит 6 знаков после запятой. Расход на дешёвых
-- моделях меньше $0.000001 за вызов, поэтому округлялся в 0.000000 — cost ledger
-- (Issue #2) и hard budget guard (Issue #3) теряли такие вызовы без следа.
-- Накопленно: 10k вызовов × $0.0000004 = $0.004, которые не видно в отчётах.
--
-- Фикс: 10 знаков после запятой (точность до 1e-10). Максимум numeric(18,10) =
-- 99 999 999.9999999999 USD — с запасом на любой суммарный бюджет.
--
-- ВАЖНО: ALTER COLUMN ... TYPE нельзя выполнить, пока от колонки зависит view
-- (`rule _RETURN on view ... depends on column`). Поэтому сначала снимаем оба
-- view, меняем типы, затем пересоздаём view с теми же определениями.
--
-- Идемпотентно: `drop view if exists` + пересоздание; повторный ALTER TYPE на
-- уже расширенную колонку — no-op (Postgres перепишет тип без потери данных).

drop view if exists public.llm_costs_daily;
drop view if exists public.llm_costs_by_task;

alter table if exists public.llm_calls
  alter column prompt_cost_usd     type numeric(18,10),
  alter column completion_cost_usd type numeric(18,10),
  alter column total_cost_usd      type numeric(18,10);

-- View: Daily cost summary (определение из 001, без изменений).
create view public.llm_costs_daily as
  select
    date_trunc('day', created_at at time zone 'UTC') as day,
    model,
    count(*) as calls,
    sum(prompt_tokens + completion_tokens) as total_tokens,
    sum(prompt_tokens) as prompt_tokens,
    sum(completion_tokens) as completion_tokens,
    sum(cached_tokens) as cached_tokens,
    sum(total_cost_usd) as total_cost_usd,
    count(*) filter (where not success) as errors
  from public.llm_calls
  group by 1, 2
  order by 1 desc, 2;

-- View: Per-task cost summary (определение из 001, без изменений).
create view public.llm_costs_by_task as
  select
    t.id as task_id,
    t.root_id,
    t.state,
    count(l.*) as llm_calls,
    sum(l.prompt_tokens + l.completion_tokens) as total_tokens,
    sum(l.total_cost_usd) as total_cost_usd,
    t.spent_tokens,
    t.budget_cap
  from public.tasks t
  left join public.llm_calls l on l.task_id = t.id
  group by t.id
  order by total_cost_usd desc nulls last;

-- Пересозданные view снова принадлежат swarm_owner (см. 003) и требуют грантов
-- из 002: `create or replace view` их не теряет, но `drop`+`create` — теряет.
alter view public.llm_costs_daily  owner to swarm_owner;
alter view public.llm_costs_by_task owner to swarm_owner;

grant select on public.llm_costs_daily  to swarm_operator, swarm_planner, swarm_reviewer, swarm_watchdog;
grant select on public.llm_costs_by_task to swarm_operator, swarm_planner, swarm_reviewer, swarm_watchdog;
