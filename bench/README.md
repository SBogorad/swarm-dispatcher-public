# C1 PostgreSQL load test

C1 measures the dispatcher DB claim/complete cycle using 1,000 independent tasks and 50 concurrent mock workers by default. Tasks are inserted through schema `add_task`, promoted using `promote_ready`, claimed using `claim_task` (`FOR UPDATE SKIP LOCKED`), then completed with a tiny mock artifact. No production crates or schema are modified. Each invocation creates uniquely named `c1-<run id>-*` workers and unique task UUIDs. Use a disposable, isolated test database; records intentionally remain there for inspection/cleanup.

## Setup and run

Requires Python 3 and `psycopg2` (`python -m pip install psycopg2-binary`). Apply `schema/schema_v0.sql` to a dedicated empty database first. Never pass the production-like `swarm` database: the runner refuses common production DB names and requires an explicit DSN.

```sh
export LOAD_TEST_DATABASE_URL='host=127.0.0.1 port=55440 dbname=c1_loadtest user=... password=...'
python3 bench/load_test.py                         # 1000 tasks, 50 workers
python3 bench/load_test.py --tasks 20 --workers 4 # smoke test
```

Alternatively pass `--dsn`. Artifacts are emitted under `bench/results/c1/` (git-ignored) as JSON and one-row CSV. Metrics: tasks/sec, claim latency p50/p95/p99 (including the final empty-queue poll), wall-clock cycle time, number of DB connections visible immediately after workers exit, completion count, errors. Each mock worker uses one persistent PostgreSQL connection. `cycle_time_seconds` is setup-excluded elapsed time from concurrent claims through completion. The benchmark intentionally measures DB dispatch rather than LLM or simulated task execution.

Run against an isolated DB only; PostgreSQL activity/CPU and other concurrent clients affect results. The script does not drop tables or clean up, so destroy/recreate the disposable DB between trials. Keep DSNs/secrets out of command logs and artifacts; output contains only the database name.
