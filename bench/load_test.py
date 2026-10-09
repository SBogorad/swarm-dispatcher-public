#!/usr/bin/env python3
"""C1 deterministic PostgreSQL claim/complete load test (mock worker workload)."""
import argparse
import concurrent.futures
import csv
import json
import os
import statistics
import threading
import time
import uuid
from datetime import datetime, timezone

import psycopg2
from psycopg2.extras import Json

CONTRACT = {"version": 1, "description": "C1 mock load task", "needs": {"min_ram_gb": 1, "coder_class": 1, "affinity_tags": []}, "budget": {"tokens": 100, "attempts": 3, "max_tasks": 2000, "max_depth": 10, "max_children": 20}, "checks": [{"type": "cmd", "run": "true"}]}


def pct(values, p):
    if not values:
        return None
    s = sorted(values)
    return s[min(len(s)-1, max(0, int((len(s)-1)*p + .5)))]


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument('--dsn', default=os.getenv('LOAD_TEST_DATABASE_URL'), help='isolated test DB only; no default')
    ap.add_argument('--tasks', type=int, default=1000)
    ap.add_argument('--workers', type=int, default=50)
    ap.add_argument('--output', default='bench/results/c1')
    ap.add_argument('--lease-sec', type=int, default=300)
    a = ap.parse_args()
    if not a.dsn:
        ap.error('provide --dsn or LOAD_TEST_DATABASE_URL for an isolated test database')
    if a.tasks < 1 or a.workers < 1:
        ap.error('tasks and workers must be positive')
    # Strong guard against accidentally targeting the production-like default DB.
    check = psycopg2.connect(a.dsn)
    check.autocommit = True
    dbname = check.get_dsn_parameters().get('dbname', '')
    if dbname.lower() in {'swarm', 'production', 'prod'}:
        check.close()
        ap.error('refusing production-like database name; use an isolated test database')
    check.close()
    run_id = uuid.uuid4().hex[:12]
    root = uuid.uuid4()
    ids = [uuid.uuid4() for _ in range(a.tasks)]
    setup = psycopg2.connect(a.dsn)
    setup.autocommit = True
    with setup.cursor() as c:
        for i in range(a.workers):
            c.execute('SELECT register_worker(%s,%s::jsonb,%s)', (f'c1-{run_id}-{i}', Json({'ram_gb': 16, 'models': {'coder_class': 2}}), []))
        # Every task self-roots; parent topology would distort claim contention.
        for task in ids:
            c.execute('SELECT add_task(%s,%s,%s::jsonb,%s,%s)', (str(task), str(task), Json(CONTRACT), 1000, 0))
        c.execute('SELECT promote_ready()')
    setup.close()

    latencies, errors = [], []
    counts = [0] * a.workers
    lock = threading.Lock()
    stop_monitor = threading.Event()
    peak_connections = 0

    def monitor_connections():
        nonlocal peak_connections
        conn = psycopg2.connect(a.dsn)
        conn.autocommit = True
        try:
            while not stop_monitor.is_set():
                with conn.cursor() as c:
                    c.execute('SELECT count(*) FROM pg_stat_activity WHERE datname=current_database()')
                    observed = c.fetchone()[0]
                with lock:
                    peak_connections = max(peak_connections, observed)
                stop_monitor.wait(0.05)
        finally:
            conn.close()

    start = time.monotonic()
    monitor = threading.Thread(target=monitor_connections, daemon=True)
    monitor.start()

    def worker(i):
        conn = psycopg2.connect(a.dsn)
        conn.autocommit = True
        wid = f'c1-{run_id}-{i}'
        while True:
            t0 = time.monotonic()
            try:
                with conn.cursor() as c:
                    c.execute('SELECT out_task_id,out_epoch FROM claim_task(%s,%s)', (wid, a.lease_sec))
                    row = c.fetchone()
                elapsed = time.monotonic() - t0
                with lock:
                    latencies.append(elapsed)
                if row is None or row[0] is None:
                    break
                task, epoch = row
                ref = 'c1:' + str(task)
                with conn.cursor() as c:
                    c.execute('SELECT save_artifact(%s,%s,%s,%s,%s,%s,%s)', (task, wid, epoch, ref, 'mock://c1', 'application/octet-stream', 0))
                    c.execute('SELECT complete_task(%s,%s,%s,%s,%s)', (task, wid, epoch, ref, 0))
                counts[i] += 1
            except Exception as e:
                with lock:
                    errors.append(str(e))
                break
        conn.close()

    with concurrent.futures.ThreadPoolExecutor(max_workers=a.workers) as pool:
        list(pool.map(worker, range(a.workers)))
    elapsed = time.monotonic() - start
    stop_monitor.set()
    monitor.join()
    total = sum(counts)
    # Peak/current connections for this isolated DB, measured after clients close (0 expected).
    monitor = psycopg2.connect(a.dsn)
    with monitor.cursor() as c:
        c.execute('SELECT count(*) FROM pg_stat_activity WHERE datname=current_database()')
        current_connections = c.fetchone()[0]
        c.execute("SELECT count(*) FROM tasks WHERE id::text = ANY(%s) AND state = 'review'", ([str(x) for x in ids],))
        reviewed = c.fetchone()[0]
    monitor.close()
    result = {'scenario':'C1', 'run_id':run_id, 'database':dbname, 'started_utc':datetime.now(timezone.utc).isoformat(), 'tasks_requested':a.tasks, 'workers':a.workers, 'tasks_completed':total, 'review_state_count':reviewed, 'duration_seconds':elapsed, 'throughput_tasks_per_second':total/elapsed if elapsed else 0, 'claim_latency_seconds':{'p50':pct(latencies,.50),'p95':pct(latencies,.95),'p99':pct(latencies,.99),'samples':len(latencies)}, 'cycle_time_seconds':elapsed, 'db_connections_peak':peak_connections, 'db_connections_after_run':current_connections, 'errors':errors}
    os.makedirs(a.output, exist_ok=True)
    with open(os.path.join(a.output, f'{run_id}.json'), 'w') as f:
        json.dump(result, f, indent=2)
    with open(os.path.join(a.output, f'{run_id}.csv'), 'w', newline='') as f:
        w = csv.DictWriter(f, fieldnames=result.keys()); w.writeheader(); w.writerow({k: json.dumps(v) if isinstance(v, (dict,list)) else v for k,v in result.items()})
    print(json.dumps(result, indent=2))
    if errors or total != a.tasks or reviewed != a.tasks:
        raise SystemExit(1)

if __name__ == '__main__':
    main()
