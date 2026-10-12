"""Тонкий клиент SQL-протокола swarm-dispatcher для демо-стенда.

Использует ТОЛЬКО публичные функции схемы (add_task, add_dep, register_worker,
claim_task, heartbeat, save_artifact, complete_task, submit_review, fail_task,
promote_ready, reclaim_expired, verify_chain) — то есть демо-стенд говорит с
движком ровно так же, как Rust-воркер, и получает настоящие fencing-аренды,
эпохи и хэш-цепочку событий.

Чтение витрины (канбан, воркеры, журнал) идёт из `tasks`/`events`/`workers`/
`artifacts`/`reviews` — это витрина движка, а не обход протокола.
"""
from __future__ import annotations

import hashlib
import os
from typing import Any

import psycopg
from psycopg.types.json import Jsonb

DSN = os.environ.get(
    "DEMO_DATABASE_URL",
    "postgres://swarm:***@postgres:5432/swarm",
)


def _conn() -> psycopg.Connection:
    return psycopg.connect(DSN, autocommit=True)


# --------------------------------------------------------------------------
# Протокол (то, чем говорит Rust-воркер/планировщик/сторож)
# --------------------------------------------------------------------------
def register_worker(worker_id: str, ram_gb: int = 16, coder_class: int = 2,
                    tags: list[str] | None = None) -> None:
    caps = {"ram_gb": ram_gb, "models": {"coder_class": coder_class}}
    with _conn() as c:
        c.execute("select register_worker(%s, %s, %s)",
                  (worker_id, Jsonb(caps), tags or []))


def add_task(task_id: str, contract: dict[str, Any], budget_cap: int = 200_000,
             priority: int = 0, root: str | None = None) -> None:
    """Создать задачу. `root=None` — задача сама себе корень (как у планировщика)."""
    with _conn() as c:
        c.execute("select add_task(%s::uuid, %s::uuid, %s, %s, %s)",
                  (task_id, root or task_id, Jsonb(contract), budget_cap, priority))


def add_dep(task_id: str, dep_id: str) -> None:
    """Ребро графа: `task_id` ждёт `dep_id` (движок проверит циклы/глубину)."""
    with _conn() as c:
        c.execute("select add_dep(%s::uuid, %s::uuid)", (task_id, dep_id))


def promote_ready() -> int:
    with _conn() as c:
        row = c.execute("select promote_ready()").fetchone()
    return int(row[0]) if row else 0


def reclaim_expired() -> int:
    with _conn() as c:
        row = c.execute("select reclaim_expired()").fetchone()
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


# --------------------------------------------------------------------------
# Витрина (чтение состояния движка)
# --------------------------------------------------------------------------
def task_state(task_id: str) -> dict[str, Any] | None:
    with _conn() as c:
        cur = c.execute(
            "select id, root_id, state, attempts, rework, budget_cap, spent_tokens, "
            "       result_ref, block_reason, created_at, updated_at, contract, "
            "       lease_owner, lease_epoch "
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


def task_chain(task_id: str, limit: int = 60) -> list[dict[str, Any]]:
    """Журнал событий с хэшами (для показа неизменяемой цепочки аудита)."""
    with _conn() as c:
        rows = c.execute(
            "select seq, kind, actor, at, encode(hash,'hex'), encode(prev_hash,'hex') "
            "  from events where task_id = %s::uuid order by seq asc limit %s",
            (task_id, limit)).fetchall()
    return [{"seq": r[0], "kind": r[1], "actor": r[2], "at": r[3],
             "hash": r[4], "prev_hash": r[5]} for r in rows]


def verify_chain(task_id: str) -> bool:
    with _conn() as c:
        r = c.execute("select verify_chain(%s::uuid)", (task_id,)).fetchone()
    return bool(r and r[0])


def board() -> list[dict[str, Any]]:
    """Все задачи для канбана: состояние, кто держит, зависимости, бюджет."""
    with _conn() as c:
        rows = c.execute(
            "select t.id, t.root_id, t.state, t.lease_owner, t.lease_epoch, t.attempts, "
            "       t.rework, t.spent_tokens, t.budget_cap, t.priority, t.block_reason, "
            "       t.created_at, t.updated_at, coalesce(t.contract->>'kind','task') as kind, "
            "       coalesce(t.contract->>'description','') as description, "
            "       (select count(*) from task_deps d where d.task_id = t.id) as deps_total, "
            "       (select count(*) from task_deps d join tasks p on p.id = d.dep_id "
            "          where d.task_id = t.id and p.state <> 'done') as deps_open "
            "  from tasks t order by t.created_at asc, t.id asc").fetchall()
    keys = ["id", "root_id", "state", "lease_owner", "lease_epoch", "attempts", "rework",
            "spent_tokens", "budget_cap", "priority", "block_reason", "created_at",
            "updated_at", "kind", "description", "deps_total", "deps_open"]
    return [dict(zip(keys, r)) for r in rows]


def workers() -> list[dict[str, Any]]:
    with _conn() as c:
        rows = c.execute(
            "select w.id, w.capabilities, w.affinity_tags, w.last_seen, "
            "       (select count(*) from tasks t "
            "          where t.lease_owner = w.id and t.state = 'leased') as leased "
            "  from workers w order by w.last_seen desc").fetchall()
    return [{"id": r[0], "capabilities": r[1], "affinity_tags": r[2],
             "last_seen": r[3], "leased": r[4]} for r in rows]


def task_artifacts(task_id: str) -> list[dict[str, Any]]:
    with _conn() as c:
        rows = c.execute(
            "select ref, created_by, lease_epoch, content_type, size_bytes, "
            "       storage_path, created_at from artifacts "
            " where task_id = %s::uuid order by created_at desc", (task_id,)).fetchall()
    return [{"ref": r[0], "created_by": r[1], "lease_epoch": r[2], "content_type": r[3],
             "size_bytes": r[4], "storage_path": r[5], "created_at": r[6]} for r in rows]


def task_reviews(task_id: str) -> list[dict[str, Any]]:
    with _conn() as c:
        rows = c.execute(
            "select reviewer, verdict, checks, created_at from reviews "
            " where task_id = %s::uuid order by created_at desc", (task_id,)).fetchall()
    return [{"reviewer": r[0], "verdict": r[1], "checks": r[2], "created_at": r[3]}
            for r in rows]


def review_candidates(limit: int = 10) -> list[dict[str, Any]]:
    with _conn() as c:
        rows = c.execute(
            "select id, result_ref, contract from tasks "
            " where state = 'review' order by updated_at asc limit %s",
            (limit,)).fetchall()
    return [{"id": r[0], "result_ref": r[1], "contract": r[2]} for r in rows]


def sha256_ref(data: bytes) -> str:
    return "sha256:" + hashlib.sha256(data).hexdigest()
