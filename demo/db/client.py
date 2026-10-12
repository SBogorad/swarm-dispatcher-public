"""Тонкий клиент SQL-протокола swarm-dispatcher для демо-стенда.

Использует ТОЛЬКО публичные функции схемы (add_task, register_worker,
claim_task, heartbeat, save_artifact, complete_task, submit_review, fail_task) —
то есть демо-стенд говорит с движком ровно так же, как Rust-воркер, и получает
настоящие fencing-аренды, эпохи и хэш-цепочку событий.

Чтение статусов (канбан) идёт напрямую из `tasks`/`events` — это витрина, а не
обход протокола.
"""
from __future__ import annotations

import hashlib
import os
from typing import Any

import psycopg
from psycopg.types.json import Jsonb

DSN = os.environ.get(
    "DEMO_DATABASE_URL",
    "postgres://swarm:swarm@postgres:5432/swarm",
)


def _conn() -> psycopg.Connection:
    return psycopg.connect(DSN, autocommit=True)


def register_worker(worker_id: str, ram_gb: int = 16, coder_class: int = 2) -> None:
    caps = {"ram_gb": ram_gb, "models": {"coder_class": coder_class}}
    with _conn() as c:
        c.execute("select register_worker(%s, %s, %s)",
                  (worker_id, Jsonb(caps), []))


def add_task(task_id: str, contract: dict[str, Any], budget_cap: int = 200_000,
             priority: int = 0) -> None:
    with _conn() as c:
        c.execute("select add_task(%s::uuid, %s::uuid, %s, %s, %s)",
                  (task_id, task_id, Jsonb(contract), budget_cap, priority))


def promote_ready() -> int:
    with _conn() as c:
        row = c.execute("select promote_ready()").fetchone()
    return int(row[0]) if row else 0


def claim(worker_id: str, lease_sec: int = 120) -> dict[str, Any] | None:
    with _conn() as c:
        cur = c.execute("select * from claim_task(%s, %s)", (worker_id, lease_sec))
        row = cur.fetchone()
        if row is None:
            return None
        cols = [d.name for d in cur.description]
    return dict(zip(cols, row))


def heartbeat(task_id: str, worker_id: str, epoch: int, extend_sec: int = 120) -> bool:
    with _conn() as c:
        r = c.execute("select heartbeat(%s::uuid, %s, %s, %s)",
                      (task_id, worker_id, epoch, extend_sec)).fetchone()
    return bool(r and r[0])


def save_artifact(task_id: str, worker_id: str, epoch: int, ref: str,
                  storage_path: str, content_type: str, size: int) -> None:
    with _conn() as c:
        c.execute("select save_artifact(%s::uuid, %s, %s, %s, %s, %s, %s)",
                  (task_id, worker_id, epoch, ref, storage_path, content_type, size))


def complete(task_id: str, worker_id: str, epoch: int, result_ref: str,
             tokens: int = 0) -> str:
    with _conn() as c:
        r = c.execute("select complete_task(%s::uuid, %s, %s, %s, %s)",
                      (task_id, worker_id, epoch, result_ref, tokens)).fetchone()
    return str(r[0]) if r else "review"


def fail(task_id: str, worker_id: str, epoch: int, ref: str | None, reason: str) -> str:
    with _conn() as c:
        r = c.execute("select fail_task(%s::uuid, %s, %s, %s, %s)",
                      (task_id, worker_id, epoch, ref, reason)).fetchone()
    return str(r[0]) if r else "blocked"


def submit_review(task_id: str, ref: str, reviewer: str, verdict: str,
                  checks: list[dict[str, Any]]) -> str:
    with _conn() as c:
        r = c.execute("select submit_review(%s::uuid, %s, %s, %s, %s)",
                      (task_id, ref, reviewer, verdict, Jsonb(checks))).fetchone()
    return str(r[0]) if r else ""


def task_state(task_id: str) -> dict[str, Any] | None:
    with _conn() as c:
        cur = c.execute(
            "select id, state, attempts, rework, budget_cap, spent_tokens, "
            "       result_ref, block_reason, created_at, updated_at, contract "
            "  from tasks where id = %s::uuid", (task_id,))
        row = cur.fetchone()
        if row is None:
            return None
        cols = [d.name for d in cur.description]
    return dict(zip(cols, row))


def task_events(task_id: str, limit: int = 40) -> list[dict[str, Any]]:
    with _conn() as c:
        rows = c.execute(
            "select seq, kind, actor, payload, at from events "
            " where task_id = %s::uuid order by seq asc limit %s",
            (task_id, limit)).fetchall()
    return [{"seq": r[0], "kind": r[1], "actor": r[2], "payload": r[3], "at": r[4]}
            for r in rows]


def review_candidates(limit: int = 5) -> list[dict[str, Any]]:
    with _conn() as c:
        rows = c.execute(
            "select id, result_ref, contract from tasks "
            " where state = 'review' order by updated_at asc limit %s",
            (limit,)).fetchall()
    return [{"id": r[0], "result_ref": r[1], "contract": r[2]} for r in rows]


def sha256_ref(data: bytes) -> str:
    return "sha256:" + hashlib.sha256(data).hexdigest()
