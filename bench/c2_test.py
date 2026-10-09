#!/usr/bin/env python3
"""C2: run real swarm-worker OS processes against an isolated PostgreSQL DB + LLM."""
import argparse
import json
import os
import signal
import subprocess
import sys
import time
import uuid
from datetime import datetime, timezone

import psycopg2
from psycopg2.extras import Json


def percentile(values, p):
    if not values:
        return None
    values = sorted(values)
    return float(values[min(len(values) - 1, int((len(values) - 1) * p))])


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument('--dsn', default=os.getenv('C2_DATABASE_URL'), help='isolated c2_loadtest DB DSN')
    ap.add_argument('--tasks', type=int, default=100)
    ap.add_argument('--workers', type=int, default=10)
    ap.add_argument('--timeout-sec', type=int, default=1800)
    ap.add_argument('--max-runtime-sec', type=int, default=7200,
                    help='hard wall-clock limit including provider stalls')
    ap.add_argument('--output', default='bench/results/c2')
    a = ap.parse_args()
    worker_dsn = os.getenv('C2_WORKER_DATABASE_URL')
    if (not a.dsn or not worker_dsn or not 1 <= a.tasks <= 100 or not 1 <= a.workers <= 10
            or a.timeout_sec < 1 or a.max_runtime_sec < a.timeout_sec):
        ap.error('requires valid DSNs, 1..100 tasks, 1..10 workers, positive timeout and max-runtime >= timeout')
    required = ('LLM_PROVIDER', 'LLM_BASE_URL', 'LLM_API_KEY', 'LLM_MODEL')
    if any(not os.environ.get(k) for k in required) or os.environ['LLM_PROVIDER'] != 'openai-compatible':
        ap.error('set LLM_PROVIDER=openai-compatible, LLM_BASE_URL, LLM_API_KEY, and LLM_MODEL via environment')

    # Validate target identity before any mutation or worker launch; never print DSN.
    try:
        check = psycopg2.connect(a.dsn, connect_timeout=5)
        check.autocommit = True
        dbname = check.get_dsn_parameters().get('dbname', '')
        with check.cursor() as cur:
            cur.execute('SELECT current_database()')
            actual = cur.fetchone()[0]
        check.close()
    except Exception as e:
        ap.error(f'DSN validation failed ({type(e).__name__}); no tasks/workers started')
    if actual != dbname or not actual.startswith('c2_'):
        ap.error('refusing DB: database name must match DSN and start with c2_')
    try:
        worker_check = psycopg2.connect(worker_dsn, connect_timeout=5)
        with worker_check.cursor() as cur:
            cur.execute('SELECT current_database(), session_user')
            worker_db, worker_role = cur.fetchone()
        worker_check.close()
    except Exception as e:
        ap.error(f'worker DSN validation failed ({type(e).__name__}); no tasks/workers started')
    if worker_db != actual or worker_role != 'swarm_worker_w1':
        ap.error('worker DSN must connect to the same c2_ database as swarm_worker_w1')

    # Build exactly once, before seeding tasks, so parallel cargo invocations do
    # not contend and a build failure cannot leave a partially prepared run.
    repo = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    binary = os.path.join(repo, 'target', 'debug', 'swarm-worker')
    build = subprocess.run(['cargo', 'build', '--locked', '-p', 'swarm-worker'], cwd=repo,
                           stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    if build.returncode:
        sys.stderr.write('swarm-worker build failed; compiler output suppressed\n')
        raise SystemExit(build.returncode)
    if not os.path.isfile(binary):
        ap.error('cargo build succeeded but expected worker binary is missing')

    run = uuid.uuid4().hex[:12]
    ids = [uuid.uuid4() for _ in range(a.tasks)]
    contract = {'version': 1, 'description': 'C2 isolated LLM load test: return a short plain-text result.',
                'needs': {'min_ram_gb': 1, 'coder_class': 1, 'affinity_tags': []},
                'budget': {'tokens': 100000, 'attempts': 1, 'max_tasks': 2000, 'max_depth': 10, 'max_children': 20},
                'checks': [{'type': 'cmd', 'run': 'true'}]}
    conn = psycopg2.connect(a.dsn)
    conn.autocommit = True
    try:
        with conn.cursor() as cur:
            # Ten processes concurrently compete under the one existing w1
            # login/identity. Claiming is serialized safely by task row locks.
            cur.execute('SELECT register_worker(%s,%s::jsonb,%s)',
                        ('w1', Json({'ram_gb': 16, 'models': {'coder_class': 2}}), []))
            for task in ids:
                cur.execute('SELECT add_task(%s,%s,%s::jsonb,%s,%s)',
                            (str(task), str(task), Json(contract), 100000, 0))
            cur.execute('SELECT promote_ready()')
    finally:
        conn.close()

    workers = []
    artifact_root = os.path.abspath(f'/tmp/swarm-c2-{run}')
    os.makedirs(artifact_root, mode=0o700)
    envbase = os.environ.copy()
    envbase.update({'DATABASE_URL': worker_dsn, 'WORKER_MAX_TOKENS': '128', 'LEASE_SEC': '30',
                    'WORKER_LLM_TIMEOUT_SEC': str(max(1, a.max_runtime_sec)),
                    'HTTP_PROXY': '', 'HTTPS_PROXY': '', 'ALL_PROXY': '', 'http_proxy': '', 'https_proxy': '', 'all_proxy': '', 'RUST_LOG': 'swarm_worker=info',
                    'ARTIFACTS_DIR': artifact_root})
    start = time.monotonic()
    for i in range(a.workers):
        env = envbase.copy()
        env['WORKER_ID'] = 'w1'
        logpath = os.path.join(artifact_root, f'worker-{i}.log')
        log = open(logpath, 'w')
        try:
            proc = subprocess.Popen([binary], cwd=repo, env=env, stdout=log,
                                    stderr=subprocess.STDOUT, start_new_session=True)
        except Exception:
            log.close()
            for p, f, _ in workers:
                os.killpg(p.pid, signal.SIGTERM); f.close()
            raise
        workers.append((proc, log, logpath))

    timed_out = False
    try:
        deadline = start + a.max_runtime_sec
        while time.monotonic() < deadline:
            c = psycopg2.connect(a.dsn)
            try:
                with c.cursor() as cur:
                    cur.execute("SELECT count(*) FILTER (WHERE state='review'), count(*) FILTER (WHERE state='blocked'), count(*) FILTER (WHERE state='ready' OR state='leased') FROM tasks WHERE id::text = ANY(%s)", ([str(x) for x in ids],))
                    review, blocked, unfinished = cur.fetchone()
            finally:
                c.close()
            if review + blocked == a.tasks:
                break
            time.sleep(1)
        else:
            timed_out = True
    finally:
        for p, _, _ in workers:
            if p.poll() is None:
                os.killpg(p.pid, signal.SIGTERM)
        for p, f, _ in workers:
            try: p.wait(timeout=10)
            except subprocess.TimeoutExpired:
                os.killpg(p.pid, signal.SIGKILL); p.wait()
            f.close()

    elapsed = time.monotonic() - start
    conn = psycopg2.connect(a.dsn)
    try:
        with conn.cursor() as cur:
            cur.execute("SELECT state, count(*) FROM tasks WHERE id::text = ANY(%s) GROUP BY state", ([str(x) for x in ids],))
            states = dict(cur.fetchall())
            cur.execute("SELECT COALESCE(sum(spent_tokens),0) FROM tasks WHERE id::text = ANY(%s)", ([str(x) for x in ids],))
            tokens = cur.fetchone()[0]
            cur.execute("SELECT count(*) FROM events WHERE task_id::text = ANY(%s) AND kind='completed'", ([str(x) for x in ids],))
            completed_events = cur.fetchone()[0]
            cur.execute("""SELECT e.task_id,
                       extract(epoch FROM (max(e.at) FILTER (WHERE e.kind='completed') -
                                           min(e.at) FILTER (WHERE e.kind='claimed'))) AS latency_seconds
                       FROM events e WHERE e.task_id::text = ANY(%s)
                       GROUP BY e.task_id
                       HAVING count(*) FILTER (WHERE e.kind='claimed') > 0
                          AND count(*) FILTER (WHERE e.kind='completed') > 0""", ([str(x) for x in ids],))
            task_latencies = [{'task_id': str(t), 'seconds': float(lat)} for t, lat in cur.fetchall()]
    finally: conn.close()
    errors = []
    for _, _, path in workers:
        try:
            with open(path, errors='replace') as f:
                for line in f:
                    if '"level":"ERROR"' in line or '"level":"WARN"' in line:
                        # Do not copy possible provider/DB detail into summary.
                        errors.append({'worker_log': os.path.basename(path), 'level': 'error' if '"level":"ERROR"' in line else 'warning'})
        except OSError: pass
    result = {'scenario': 'C2', 'run_id': run, 'database': actual, 'started_utc': datetime.now(timezone.utc).isoformat(),
              'tasks_requested': a.tasks, 'workers': a.workers, 'states': states, 'completed_events': completed_events,
              'usage_tokens_reported_by_worker_db': tokens, 'usage_note': 'sum tasks.spent_tokens; provider usage is persisted only if worker response exposes usage',
              'task_claim_to_complete_latency_seconds': task_latencies,
              'percentiles_seconds': {'p50': percentile([x['seconds'] for x in task_latencies], 0.50),
                                      'p95': percentile([x['seconds'] for x in task_latencies], 0.95),
                                      'p99': percentile([x['seconds'] for x in task_latencies], 0.99)},
              'duration_seconds': elapsed, 'tasks_per_second': completed_events / elapsed if elapsed else 0,
              'timed_out': timed_out, 'worker_exit_codes': [p.returncode for p, _, _ in workers],
              'worker_log_files': [os.path.basename(p) for _, _, p in workers], 'errors_and_warnings_count': len(errors),
              'errors_and_warnings': errors[:100]}
    os.makedirs(a.output, exist_ok=True)
    with open(os.path.join(a.output, f'{run}.json'), 'w') as f: json.dump(result, f, indent=2, default=float)
    print(json.dumps(result, indent=2, default=float))
    if timed_out or states.get('review', 0) != a.tasks or any(p.returncode not in (0, -signal.SIGTERM) for p, _, _ in workers):
        raise SystemExit(1)

if __name__ == '__main__': main()
