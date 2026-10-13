-- Migration 010: витрина качества моделей (Issue #7/#10).
--
-- Проблема: модель выбиралась по названию, а не по ИЗМЕРЕННОЙ цене успешного
-- результата. Дешёвая модель, которая валится в `max_rework`, дороже дорогой,
-- которая сдаёт с первого раза. Данные для измерения уже есть: `llm_calls`
-- (модель + стоимость вызова) и `tasks` (терминальное состояние + `rework`).
--
-- Решение: view `llm_model_quality`, агрегирующая по модели:
--   - `tasks`          — сколько задач с этой моделью дошло до терминала;
--   - `tasks_done`     — сколько из них `done`;
--   - `success_rate`   — доля успеха (done / tasks);
--   - `total_cost_usd` — суммарный расход по модели (терминальные задачи);
--   - `cost_per_success_usd` — **цена успешного результата** (расход / done);
--   - `avg_rework`     — среднее число доработок (косвенная мера качества).
--
-- Именно `cost_per_success_usd` — метрика выбора для селектора
-- (`crates/swarm-worker/src/model_selector.rs`); читается
-- `scripts/model_quality_report.py` и кладётся в `SWARM_MODEL_STATS_FILE`.
--
-- Права: view исполняется от владельца `swarm_owner` (как 003/008/009), поэтому
-- worker-ролям не нужен прямой SELECT на `tasks`/`llm_calls` сверх уже выданного.
--
-- Идемпотентно: create or replace view + повторный grant безопасны.

create or replace view public.llm_model_quality as
with per_task as (
  -- Расход по (модель, задача): одна задача могла уйти на несколько моделей,
  -- поэтому агрегируем именно пары, а не сумму по модели целиком.
  select c.model,
         c.task_id,
         sum(c.total_cost_usd) as task_cost_usd
    from public.llm_calls c
   group by c.model, c.task_id
),
terminal as (
  -- Только терминальные задачи: незавершённые искажали бы success_rate.
  select p.model,
         p.task_id,
         p.task_cost_usd,
         t.state,
         t.rework
    from per_task p
    join public.tasks t on t.id = p.task_id
   where t.state in ('done', 'blocked', 'failed', 'budget_exhausted')
)
select
  model,
  count(*)::bigint                                   as tasks,
  count(*) filter (where state = 'done')::bigint     as tasks_done,
  case when count(*) > 0
       then round((count(*) filter (where state = 'done'))::numeric / count(*), 4)
       else 0::numeric end                           as success_rate,
  coalesce(round(sum(task_cost_usd), 6), 0::numeric) as total_cost_usd,
  case when count(*) filter (where state = 'done') > 0
       then round(sum(task_cost_usd) / (count(*) filter (where state = 'done')), 6)
       else null end                                 as cost_per_success_usd,
  coalesce(round(avg(rework), 2), 0::numeric)        as avg_rework
from terminal
group by model;

comment on view public.llm_model_quality is
  'Issue #7/#10: качество модели по факту — tasks/tasks_done/success_rate и цена успешного результата (cost_per_success_usd)';

alter view public.llm_model_quality owner to swarm_owner;
grant select on public.llm_model_quality
  to swarm_operator, swarm_planner, swarm_reviewer, swarm_watchdog;
