-- Migration 008: сверка cost ledger с relay.fast (Issue #2).
--
-- Проблема: локальный ledger (`llm_calls`) считает расход по оценкам цен
-- (см. `llm_cost.rs`), а подтверждённый расход живёт только в web-интерфейсе
-- relay.fast (публичного API баланса/usage нет). Расхождение локального и
-- подтверждённого расхода нигде не фиксировалось, поэтому «оценка» могла
-- незаметно разойтись с фактом.
--
-- Решение: таблица снимков сверки + view последнего состояния. Каждый снимок
-- хранит подтверждённый внешний расход, локальный расход на тот же момент,
-- расхождение в процентах и признак устаревания (если relay.fast недоступен —
-- переиспользуется последний подтверждённый снимок с `stale = true`).
--
-- Идемпотентно: create table if not exists + create or replace view.

create table if not exists public.llm_cost_reconciliation (
  id                bigserial primary key,
  observed_at       timestamptz not null default now(),
  source            text not null default 'relay.fast/usage-logs/common',
  external_total_usd numeric(18,10) not null check (external_total_usd >= 0),
  local_total_usd    numeric(18,10) not null check (local_total_usd >= 0),
  divergence_pct     numeric(10,4) not null,
  stale              boolean not null default false,
  note               text
);

create index if not exists llm_cost_reconciliation_observed_idx
  on public.llm_cost_reconciliation (observed_at desc);

comment on table public.llm_cost_reconciliation is
  'Issue #2: снимки сверки локального cost ledger с подтверждённым расходом relay.fast';
comment on column public.llm_cost_reconciliation.divergence_pct is
  'Issue #2: (local - external) / external * 100; >5% — требует разбора';
comment on column public.llm_cost_reconciliation.stale is
  'Issue #2: true — relay.fast недоступен, показан последний подтверждённый снимок';

-- Последнее состояние сверки: свежий подтверждённый снимок + признак устаревания.
create or replace view public.llm_cost_divergence as
  with latest_fresh as (
    select * from public.llm_cost_reconciliation
     where not stale
     order by observed_at desc
     limit 1
  )
  select
    (select max(observed_at) from public.llm_cost_reconciliation) as last_attempt_at,
    lf.observed_at       as confirmed_at,
    lf.external_total_usd as external_total_usd,
    lf.local_total_usd    as local_total_usd,
    lf.divergence_pct     as divergence_pct,
    (lf.id is null)       as stale,
    abs(lf.divergence_pct) > 5 as alert
  from (select 1) one
  left join latest_fresh lf on true;

comment on view public.llm_cost_divergence is
  'Issue #2: последнее подтверждённое состояние сверки; alert = расхождение >5%';

-- Владелец — swarm_owner (инвариант каталога, см. migration 003).
alter table public.llm_cost_reconciliation owner to swarm_owner;
alter sequence if exists public.llm_cost_reconciliation_id_seq owner to swarm_owner;
alter view public.llm_cost_divergence owner to swarm_owner;

-- Чтение сверки — оператору и планировщику (как llm_costs_*), запись — сервисные роли.
grant select on public.llm_cost_reconciliation to swarm_operator, swarm_planner;
grant select on public.llm_cost_divergence to swarm_operator, swarm_planner, swarm_reviewer, swarm_watchdog;
grant insert, select on public.llm_cost_reconciliation to swarm_operator;
grant usage, select on sequence public.llm_cost_reconciliation_id_seq to swarm_operator;
