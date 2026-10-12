-- Migration 002: GRANT для llm_calls worker-ролям (Issue #9, #11)
--
-- Проблема: migration 001 создала таблицу llm_calls и view, но выдала права
-- только роли-владельцу (swarm). Workers подключаются как swarm_worker_w1/w2,
-- поэтому INSERT в llm_calls падал с "permission denied for table llm_calls",
-- а ошибка проглатывалась в worker.rs (warn, не fail).
-- Итог: таблица всегда была пустой, расходы LLM не учитывались.
--
-- Фикс: выдать INSERT/SELECT на llm_calls и SELECT на view всем worker-ролям.
-- GRANT идемпотентен, применять безопасно повторно.

-- Таблица llm_calls: workers пишут метрики, читать им тоже нужно для отладки.
grant insert, select on llm_calls to swarm_worker_w1;
grant insert, select on llm_calls to swarm_worker_w2;
grant insert, select on llm_calls to swarm_worker_gpu_1;
grant insert, select on llm_calls to swarm_worker_w_1;

-- Sequence для bigserial id — без него INSERT падает на nextval().
grant usage, select on sequence llm_calls_id_seq to swarm_worker_w1;
grant usage, select on sequence llm_calls_id_seq to swarm_worker_w2;
grant usage, select on sequence llm_calls_id_seq to swarm_worker_gpu_1;
grant usage, select on sequence llm_calls_id_seq to swarm_worker_w_1;

-- View для сводок — читать нужно оператору, planner и reviewer.
grant select on llm_costs_daily to swarm_operator;
grant select on llm_costs_daily to swarm_planner;
grant select on llm_costs_daily to swarm_reviewer;
grant select on llm_costs_daily to swarm_watchdog;

grant select on llm_costs_by_task to swarm_operator;
grant select on llm_costs_by_task to swarm_planner;
grant select on llm_costs_by_task to swarm_reviewer;
grant select on llm_costs_by_task to swarm_watchdog;
