#!/usr/bin/env python3
"""C3 watchdog recovery test: mock workers, one crash immediately after claim."""
import argparse
import concurrent.futures
import csv
import json
import os
import threading
import time
import uuid
from datetime import datetime, timezone

import psycopg2
from psycopg2.extras import Json

CONTRACT = {
    "version": 1,
    "description": "C3 fault-injection mock task",
    "needs": {"min_ram_gb": 1, "coder_class": 1, "affinity_tags": []},
    "budget": {"tokens": 100, "attempts": 5, "max_tasks": 2000, "max_depth": 10, "max_children": 20},
    "checks": [{"type": "cmd", "run": "true"}],
}


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument('--dsn', default=os.getenv('LOAD_TEST_DATABASE_URL'))
    ap.add_argument('--tasks', type=int, default=1000)
    ap.add_argument('--workers', type=int, default=10)
    ap.add_argument('--crash-workers', type=int, default=1, help='workers that exit immediately after first claim')
    ap.add_argument('--lease-sec', type=int, default=5)
    ap.add_argument('--watchdog-interval', type=float, default=5.0)
    ap.add_argument('--output', default='bench/results/c3')
    a = ap.parse_args()
    if not a.dsn:
        ap.error('provide --dsn or LOAD_TEST_DATABASE_URL for a disposable isolated DB')
    if a.tasks < 1 or a.workers < 2 or not 1 <= a.crash_workers < a.workers:
        ap.error('require tasks>0 and 1 <= crash-workers < workers')
    if a.lease_sec < 1 or a.watchdog_interval <= 0:
        ap.error('lease and watchdog intervals must be positive')

    check = psycopg2.connect(a.dsn)
    dbname = check.get_dsn_parameters().get('dbname', '')
    check.close()
    if dbname.lower() in {'swarm', 'production', 'prod'}:
        ap.error('refusing production-like database name')

    run_id = uuid.uuid4().hex[:12]
    ids = [uuid.uuid4() for _ in range(a.tasks)]
    setup = psycopg2.connect(a.dsn)
    setup.autocommit = True
    with setup.cursor() as c:
        for i in range(a.workers):
            c.execute('SELECT register_worker(%s,%s::jsonb,%s)',
                      (f'c3-{run_id}-{i}', Json({'ram_gb': 16, 'models': {'coder_class': 2}}), []))
        for task in ids:
            c.execute('SELECT add_task(%s,%s,%s::jsonb,%s,%s)',
                      (str(task), str(task), Json(CONTRACT), 1000, 0))
        c.execute('SELECT promote_ready()')
    setup.close()

    crashed = []
    errors = []
    recovered_crash_tasks = []
    cycle = []
    all_done = threading.Event()
    stop_watchdog = threading.Event()
    watchdog_runs = []
    lock = threading.Lock()
    begin = time.monotonic()

    def watchdog():
        conn = psycopg2.connect(a.dsn)
        conn.autocommit = True
        try:
            while not stop_watchdog.is_set():
                try:
                    with conn.cursor() as c:
                        c.execute('SELECT reclaim_expired()')
                        reclaimed = c.fetchone()[0]
                        c.execute('SELECT promote_ready()')
                        promoted = c.fetchone()[0]
                    with lock:
                        watchdog_runs.append({'at_seconds': time.monotonic() - begin,
                                              'reclaimed': reclaimed, 'promoted': promoted})
                except Exception as e:
                    with lock:
                        errors.append(f'watchdog: {e}')
                    return
                stop_watchdog.wait(a.watchdog_interval)
        finally:
            conn.close()

    wh = threading.Thread(target=watchdog, daemon=True)
    wh.start()

    def worker(i):
        conn = psycopg2.connect(a.dsn)
        conn.autocommit = True
        wid = f'c3-{run_id}-{i}'
        first_claim = True
        try:
            while not all_done.is_set():
                t0 = time.monotonic()
                with conn.cursor() as c:
                    c.execute('SELECT out_task_id,out_epoch FROM claim_task(%s,%s)', (wid, a.lease_sec))
                    row = c.fetchone()
                if row is None or row[0] is None:
                    with conn.cursor() as c:
                        c.execute("SELECT count(*) FROM tasks WHERE id::text = ANY(%s) AND state = 'review'",
                                  ([str(x) for x in ids],))
                        completed = c.fetchone()[0]
                    if completed == a.tasks:
                        all_done.set()
                        break
                    time.sleep(0.05)
                    continue
                task, epoch = row
                if i < a.crash_workers and first_claim:
                    first_claim = False
                    with lock:
                        crashed.append({'worker': wid, 'task': str(task), 'epoch': epoch,
                                        'after_claim_seconds': time.monotonic() - begin})
                    # Deliberately abandon the lease, simulating a dead process.
                    conn.close()
                    return
                if str(task) in {x['task'] for x in crashed}:
                    with lock:
                        recovered_crash_tasks.append(str(task))
                ref = f'c3:{task}'
                with conn.cursor() as c:
                    c.execute('SELECT save_artifact(%s,%s,%s,%s,%s,%s,%s)',
                              (task, wid, epoch, ref, 'mock://c3', 'application/octet-stream', 0))
                    c.execute('SELECT complete_task(%s,%s,%s,%s,%s)', (task, wid, epoch, ref, 0))
                cycle.append(time.monotonic() - t0)
        except Exception as e:
            with lock:
                errors.append(f'{wid}: {e}')
        finally:
            if not conn.closed:
                conn.close()

    with concurrent.futures.ThreadPoolExecutor(max_workers=a.workers) as pool:
        futures = [pool.submit(worker, i) for i in range(a.workers)]
        for f in futures:
            f.result()
    elapsed = time.monotonic() - begin
    stop_watchdog.set()
    wh.join()

    verify = psycopg2.connect(a.dsn)
    with verify.cursor() as c:
        c.execute("SELECT count(*) FROM tasks WHERE id::text = ANY(%s) AND state = 'review'", ([str(x) for x in ids],))
        reviewed = c.fetchone()[0]
        c.execute('SELECT bool_and(verify_chain(id)) FROM tasks WHERE id::text = ANY(%s)', ([str(x) for x in ids],))
        chains_ok = c.fetchone()[0]
        c.execute("SELECT count(*) FROM tasks WHERE id::text = ANY(%s) AND state <> 'review'", ([str(x) for x in ids],))
        unfinished = c.fetchone()[0]
    verify.close()
    result = {
        'scenario': 'C3', 'run_id': run_id, 'database': dbname,
        'started_utc': datetime.now(timezone.utc).isoformat(),
        'tasks_requested': a.tasks, 'workers': a.workers,
        'configured_crashing_workers': a.crash_workers, 'crashes_after_claim': crashed,
        'lease_seconds': a.lease_sec, 'watchdog_interval_seconds': a.watchdog_interval,
        'duration_seconds': elapsed,
        'throughput_tasks_per_second': reviewed / elapsed if elapsed else 0,
        'tasks_in_review': reviewed, 'unfinished_tasks': unfinished,
        'all_verify_chains_true': chains_ok, 'reclaim_runs': watchdog_runs,
        'successful_reclaims': sum(x['reclaimed'] for x in watchdog_runs),
        'recovered_crash_tasks': recovered_crash_tasks,
        'errors': errors,
    }
    os.makedirs(a.output, exist_ok=True)
    with open(os.path.join(a.output, f'{run_id}.json'), 'w') as f:
        json.dump(result, f, indent=2)
    with open(os.path.join(a.output, f'{run_id}.csv'), 'w', newline='') as f:
        w = csv.DictWriter(f, fieldnames=result.keys())
        w.writeheader()
        w.writerow({k: json.dumps(v) if isinstance(v, (dict, list)) else v for k, v in result.items()})
    print(json.dumps(result, indent=2))
    if errors or len(crashed) != a.crash_workers or reviewed != a.tasks or unfinished or not chains_ok:
        raise SystemExit(1)


if __name__ == '__main__':
    main()
