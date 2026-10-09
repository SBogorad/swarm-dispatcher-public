# PostgreSQL load tests

## C2 real worker + OpenAI-compatible LLM

`c2_test.py` defaults to 100 independent tasks and ten actual `swarm-worker` OS processes; `--tasks` and `--workers` accept smaller positive values for a controlled smoke test before the full run. It refuses to mutate or launch workers unless the admin DSN and worker DSN point to the same database whose name starts with `c2_`; use a freshly created disposable DB with the project schema and worker grants. It never drops/cleans data. Ten real processes share the existing `swarm_worker_w1` login and `WORKER_ID=w1`; database row locking allows them to compete safely while role identity validation remains satisfied. Build runs once (`cargo build --locked`) before any LLM worker starts. Every worker gets `WORKER_MAX_TOKENS=128`; the task-level token budget is 100,000 because the worker accounts for prompt plus completion tokens, not just the model output. The hard smoke/full-run timeout is controlled by `--max-runtime-sec`. Run only after reviewing the code and confirming the DSN. This makes paid provider requests and should be treated accordingly. Provider credentials/DSN are read from environment, not printed or written to results. Logs are in a private-permissions `/tmp/swarm-c2-<run-id>` directory; inspect them locally if needed.

```sh
export C2_DATABASE_URL='host=127.0.0.1 port=55440 dbname=c2_gemini user=... password=...' # setup/admin only
export C2_WORKER_DATABASE_URL='host=127.0.0.1 port=55440 dbname=c2_gemini user=swarm_worker_w1 password=...'
export LLM_PROVIDER=openai-compatible LLM_BASE_URL='https://example.invalid/v1'
export LLM_API_KEY='...' LLM_MODEL='gemini-3.8-flash'
python3 bench/c2_test.py --tasks 2 --workers 1 # controlled paid smoke test
python3 bench/c2_test.py                     # full 100-task / 10-worker run
```

Summary JSON in `bench/results/c2/` includes elapsed time, final task states, per-task claim-to-completion latency from event timestamps, completed-event count, worker token usage recorded in task rows (provider-reported tokens only where exposed by the worker), process exit codes and sanitized counts of log errors/warnings. It does not claim request-level latency or provider usage unavailable from current worker instrumentation. Setup/admin and worker DSNs are independently validated against the same `c2_*` disposable database; the worker DSN must authenticate as `swarm_worker_w1`. No production crate/schema/role changes are made. Successful worker logs stay in private `/tmp` directories and are not copied to result JSON.

C1 measures the dispatcher DB claim/complete cycle using 1,000 independent tasks and 50 concurrent mock workers by default. Tasks are inserted through schema `add_task`, promoted using `promote_ready`, claimed using `claim_task` (`FOR UPDATE SKIP LOCKED`), then completed with a tiny mock artifact. No production crates or schema are modified. Each invocation creates uniquely named `c1-<run id>-*` workers and unique task UUIDs. Use a disposable, isolated test database; records intentionally remain there for inspection/cleanup.

## Setup and run

Requires Python 3 and `psycopg2` (`python -m pip install psycopg2-binary`). Apply `schema/schema_v0.sql` to a dedicated empty database first. Never pass the production-like `swarm` database: the runner refuses common production DB names and requires an explicit DSN.

```sh
export LOAD_TEST_DATABASE_URL='host=127.0.0.1 port=55440 dbname=c1_loadtest user=... password=...'
python3 bench/load_test.py                         # 1000 tasks, 50 workers
python3 bench/load_test.py --tasks 20 --workers 4 # smoke test
```

Alternatively pass `--dsn`. Artifacts are emitted under `bench/results/c1/` (git-ignored) as JSON and one-row CSV. Metrics: tasks/sec, claim latency p50/p95/p99 (including the final empty-queue poll), wall-clock cycle time, peak DB connections sampled every 50 ms during the run (includes the monitor connection), remaining DB connections after workers exit, completion count, errors. Each mock worker uses one persistent PostgreSQL connection. `cycle_time_seconds` is setup-excluded elapsed time from concurrent claims through completion. The benchmark intentionally measures DB dispatch rather than LLM or simulated task execution.

Run against an isolated DB only; PostgreSQL activity/CPU and other concurrent clients affect results. The script does not drop tables or clean up, so destroy/recreate the disposable DB between trials. Keep DSNs/secrets out of command logs and artifacts; output contains only the database name.

## C3 fault-injection recovery

`fault_test.py` creates tasks in an isolated DB, starts ten concurrent mock-worker threads, deliberately abandons the first claim made by one worker (10% worker failure rate), and runs the actual `reclaim_expired()`/`promote_ready()` DB functions every five seconds. The remaining workers finish the tasks with mock artifacts. It records reclaimed leases, recovered task IDs, final states and `verify_chain` result to JSON/CSV. Defaults are 1,000 tasks, 10 workers, 1 failing worker, 5-second lease and 5-second watchdog interval.

```sh
export LOAD_TEST_DATABASE_URL='host=127.0.0.1 port=55440 dbname=c3_loadtest user=... password=...'
python3 bench/fault_test.py
```

This is deterministic DB-level fault injection with threads; it does not kill real OS/Compose worker processes or test container restart behavior. The C3 report must describe that boundary.
