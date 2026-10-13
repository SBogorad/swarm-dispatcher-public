-- Migration 009: базовая точка сверки cost ledger (Issue #2).
--
-- Проблема: `total_usage` relay.fast (эндпоинт /v1/dashboard/billing/usage,
-- проверено: HTTP 200, {"object":"list","total_usage":0.2342}) — накопительный
-- за всё время жизни ключа, а локальный ledger считает расход только с момента
-- своего появления. Прямое сравнение абсолютных сумм даёт ложное расхождение
-- (вся история ключа против нескольких часов ledger) и вечный алерт.
--
-- Фикс: сверка идёт по ОКНУ между снимками. Первый подтверждённый снимок —
-- базовая точка (`is_baseline = true`, расхождение не считается); последующие
-- сравнивают приращение внешнего расхода с приращением локального.
--
-- Идемпотентно: add column if not exists + drop/create view.

alter table public.llm_cost_reconciliation
  add column if not exists window_external_usd numeric(18,10),
  add column if not exists window_local_usd    numeric(18,10),
  add column if not exists is_baseline         boolean not null default false;

comment on column public.llm_cost_reconciliation.window_external_usd is
  'Issue #2: приращение подтверждённого расхода с предыдущего подтверждённого снимка';
comment on column public.llm_cost_reconciliation.window_local_usd is
  'Issue #2: приращение локального расхода с предыдущего подтверждённого снимка';
comment on column public.llm_cost_reconciliation.is_baseline is
  'Issue #2: базовая точка (первый подтверждённый снимок), расхождение не считается';

drop view if exists public.llm_cost_divergence;

create view public.llm_cost_divergence as
  with latest_fresh as (
    select * from public.llm_cost_reconciliation
     where not stale
     order by observed_at desc, id desc
     limit 1
  )
  select
    (select max(observed_at) from public.llm_cost_reconciliation) as last_attempt_at,
    (select stale from public.llm_cost_reconciliation
      order by observed_at desc, id desc limit 1)             as last_attempt_stale,
    lf.observed_at         as confirmed_at,
    lf.external_total_usd  as external_total_usd,
    lf.local_total_usd     as local_total_usd,
    lf.window_external_usd as window_external_usd,
    lf.window_local_usd    as window_local_usd,
    coalesce(lf.is_baseline, false) as is_baseline,
    lf.divergence_pct      as divergence_pct,
    -- Issue #2: `stale` — признак УСТАРЕВШИХ данных, а не отсутствия подтверждённых
    -- снимков: он взводится, когда последняя попытка сверки не подтвердилась
    -- (relay.fast недоступен / отрицательное окно). Показывается последнее
    -- подтверждённое состояние + его timestamp + этот признак.
    ((select stale from public.llm_cost_reconciliation
       order by observed_at desc, id desc limit 1) is not false) as stale,
    -- Алерт только по окну: базовая точка и отсутствие подтверждённых снимков
    -- не алерят.
    (lf.id is not null
       and not coalesce(lf.is_baseline, false)
       and abs(lf.divergence_pct) > 5) as alert
  from (select 1) one
  left join latest_fresh lf on true;

comment on view public.llm_cost_divergence is
  'Issue #2: последнее подтверждённое состояние сверки по окну; stale = последняя попытка не подтвердилась; alert = расхождение >5% (базовая точка не алертит)';

-- Владелец и гранты (см. 003/008).
alter view public.llm_cost_divergence owner to swarm_owner;
grant select on public.llm_cost_divergence
  to swarm_operator, swarm_planner, swarm_reviewer, swarm_watchdog;
