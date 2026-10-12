-- Migration 001: LLM cost ledger (Issue #2)
-- Tracks every LLM call for precise cost accounting and relay.fast balance reconciliation.

create table llm_calls (
  id              bigserial primary key,
  task_id         uuid not null references tasks(id) on delete cascade,
  worker_id       text not null,
  lease_epoch     bigint not null,
  
  -- Request metadata
  model           text not null,
  provider        text not null default 'relay.fast',
  
  -- Token usage (input/output/cached)
  prompt_tokens   bigint not null check (prompt_tokens >= 0),
  completion_tokens bigint not null check (completion_tokens >= 0),
  cached_tokens   bigint not null default 0 check (cached_tokens >= 0),
  
  -- Cost tracking (USD, 6 decimal places for micro-transactions)
  prompt_cost_usd    numeric(12,6) not null check (prompt_cost_usd >= 0),
  completion_cost_usd numeric(12,6) not null check (completion_cost_usd >= 0),
  total_cost_usd     numeric(12,6) not null check (total_cost_usd >= 0),
  
  -- Timing
  started_at      timestamptz not null,
  completed_at    timestamptz not null,
  duration_ms     bigint not null check (duration_ms >= 0),
  
  -- Response metadata
  success         boolean not null,
  error_code      text,
  error_message   text,
  
  -- Audit trail
  created_at      timestamptz not null default now(),
  
  check (completed_at >= started_at)
);

create index llm_calls_task_idx on llm_calls (task_id);
create index llm_calls_worker_idx on llm_calls (worker_id);
create index llm_calls_created_idx on llm_calls (created_at desc);
create index llm_calls_cost_idx on llm_calls (total_cost_usd) where total_cost_usd > 0;

-- View: Daily cost summary
create view llm_costs_daily as
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
  from llm_calls
  group by 1, 2
  order by 1 desc, 2;

-- View: Per-task cost summary
create view llm_costs_by_task as
  select
    t.id as task_id,
    t.root_id,
    t.state,
    count(l.*) as llm_calls,
    sum(l.prompt_tokens + l.completion_tokens) as total_tokens,
    sum(l.total_cost_usd) as total_cost_usd,
    t.spent_tokens,
    t.budget_cap
  from tasks t
  left join llm_calls l on l.task_id = t.id
  group by t.id
  order by total_cost_usd desc nulls last;

comment on table llm_calls is 'Issue #2: LLM cost ledger for precise accounting and relay.fast reconciliation';
comment on column llm_calls.cached_tokens is 'Prompt cache hits (relay.fast Claude models)';
comment on column llm_calls.total_cost_usd is 'prompt_cost_usd + completion_cost_usd';
comment on view llm_costs_daily is 'Daily cost breakdown by model';
comment on view llm_costs_by_task is 'Per-task cost summary with budget tracking';
