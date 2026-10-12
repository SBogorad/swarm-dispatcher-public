"""FastAPI-мост демо-контура: цель → граф задач → рой → протокол координации.

Мост сознательно тонкий: он НЕ решает задачу. Его дело — принять цель,
декомпозировать её в граф (публичными `add_task` + `add_dep`, как планировщик),
и отдавать витрину: канбан, воркеров, карточку задачи, журнал хэш-цепочки и
артефакты. Всю работу над задачами делают агенты-исполнители по SQL-протоколу,
а зависимостями и состояниями управляет движок.
"""
from __future__ import annotations

import os
import sys
import uuid
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

from fastapi import FastAPI, Form, HTTPException
from fastapi.responses import PlainTextResponse

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from db import client as db              # noqa: E402
from lib import decompose as dec         # noqa: E402

ARTIFACTS_DIR = Path(os.environ.get("DEMO_ARTIFACTS_DIR", "/artifacts"))

app = FastAPI(title="Swarm Ops Console — демо-мост", version="0.2.0")

TERMINAL = {"done", "blocked"}


def _iso(v: Any) -> Any:
    if isinstance(v, datetime):
        return v.astimezone(timezone.utc).isoformat()
    return v


@app.get("/healthz")
def healthz() -> dict[str, str]:
    return {"status": "ok"}


@app.post("/goals")
def create_goal(goal: str = Form(...)) -> dict[str, Any]:
    """Поставить цель: декомпозировать в граф задач и внести его в движок.

    Граф вносится публичными `add_task` + `add_dep` — ровно так же, как это
    делает `swarm-planner`. Зависимости и промоушен считает движок.
    """
    goal = (goal or "").strip()
    if not goal:
        raise HTTPException(status_code=400, detail="пустая цель")

    stages = dec.decompose(goal)
    root_id = str(uuid.uuid5(uuid.NAMESPACE_URL, "swarm-demo:" + goal))

    # Идемпотентность: если граф уже есть — вернуть его.
    existing = [t for t in db.board() if str(t["root_id"]) == root_id]
    if existing:
        return {"root_id": root_id, "tasks_created": 0, "already_existed": True,
                "tasks": [{"id": str(t["id"]), "kind": t["kind"], "state": t["state"]}
                          for t in existing]}

    ids: list[str] = []
    try:
        # Корень — первая стадия; лимиты графа живут в его контракте.
        root_contract = dec.build_contract(stages[0])
        db.add_task(root_id, root_contract, budget_cap=200_000)
        ids.append(root_id)

        for st in stages[1:]:
            tid = str(uuid.uuid5(uuid.UUID(root_id), st["kind"]))
            db.add_task(tid, dec.build_contract(st), budget_cap=200_000, root=root_id)
            ids.append(tid)

        for st, tid in zip(stages, ids):
            for dep_idx in st["depends_on"]:
                db.add_dep(tid, ids[dep_idx])
    except Exception as e:  # noqa: BLE001
        raise HTTPException(status_code=500, detail=f"add_task/add_dep: {e}") from e

    return {"root_id": root_id, "tasks_created": len(ids), "already_existed": False,
            "tasks": [{"id": tid, "kind": st["kind"]} for st, tid in zip(stages, ids)]}


@app.get("/board")
def board() -> dict[str, Any]:
    """Витрина канбана: задачи + воркеры. Читается из tasks/events/workers."""
    tasks = []
    for t in db.board():
        tasks.append({
            "id": str(t["id"]),
            "root_id": str(t["root_id"]),
            "state": t["state"],
            "kind": t["kind"],
            "description": t["description"],
            "lease_owner": t["lease_owner"],
            "lease_epoch": t["lease_epoch"],
            "attempts": t["attempts"],
            "rework": t["rework"],
            "spent_tokens": t["spent_tokens"],
            "budget_cap": t["budget_cap"],
            "priority": t["priority"],
            "block_reason": t["block_reason"],
            "deps_total": t["deps_total"],
            "deps_open": t["deps_open"],
            "created_at": _iso(t["created_at"]),
            "updated_at": _iso(t["updated_at"]),
        })
    ws = []
    for w in db.workers():
        ws.append({
            "id": w["id"],
            "capabilities": w["capabilities"],
            "affinity_tags": w["affinity_tags"],
            "last_seen": _iso(w["last_seen"]),
            "leased": w["leased"],
        })
    columns = ["pending", "ready", "leased", "review", "done", "blocked"]
    counts = {c: sum(1 for t in tasks if t["state"] == c) for c in columns}
    return {"tasks": tasks, "workers": ws, "columns": columns, "counts": counts}


@app.get("/tasks/{task_id}")
def get_task(task_id: str) -> dict[str, Any]:
    st = db.task_state(task_id)
    if st is None:
        raise HTTPException(status_code=404, detail="задача не найдена")

    events = db.task_events(task_id, limit=200)
    chain = db.task_chain(task_id, limit=200)
    try:
        chain_valid = db.verify_chain(task_id)
    except Exception:  # noqa: BLE001
        chain_valid = None

    created, updated = st.get("created_at"), st.get("updated_at")
    elapsed = None
    if isinstance(created, datetime) and isinstance(updated, datetime):
        elapsed = round((updated - created).total_seconds(), 2)

    arts = [{"ref": a["ref"], "created_by": a["created_by"],
             "lease_epoch": a["lease_epoch"], "content_type": a["content_type"],
             "size_bytes": a["size_bytes"], "storage_path": a["storage_path"],
             "created_at": _iso(a["created_at"])} for a in db.task_artifacts(task_id)]
    revs = [{"reviewer": r["reviewer"], "verdict": r["verdict"],
             "checks": r["checks"], "created_at": _iso(r["created_at"])}
            for r in db.task_reviews(task_id)]

    return {
        "task_id": task_id,
        "root_id": str(st["root_id"]),
        "state": st["state"],
        "terminal": st["state"] in TERMINAL,
        "attempts": st["attempts"],
        "rework": st["rework"],
        "budget_cap": st["budget_cap"],
        "spent_tokens": st["spent_tokens"],
        "result_ref": st["result_ref"],
        "block_reason": st["block_reason"],
        "lease_owner": st["lease_owner"],
        "lease_epoch": st["lease_epoch"],
        "contract": st["contract"],
        "created_at": _iso(created),
        "updated_at": _iso(updated),
        "elapsed_sec": elapsed,
        "chain_valid": chain_valid,
        "events": [
            {"seq": e["seq"], "kind": e["kind"], "actor": e["actor"],
             "payload": e["payload"], "at": _iso(e["at"])}
            for e in events
        ],
        "chain": [
            {"seq": c["seq"], "kind": c["kind"], "actor": c["actor"],
             "at": _iso(c["at"]), "hash": c["hash"], "prev_hash": c["prev_hash"]}
            for c in chain
        ],
        "artifacts": arts,
        "reviews": revs,
    }


@app.get("/tasks/{task_id}/artifact", response_class=PlainTextResponse)
def get_artifact(task_id: str) -> str:
    """Текст артефакта задачи (по storage_path из таблицы artifacts)."""
    arts = db.task_artifacts(task_id)
    if not arts:
        raise HTTPException(status_code=404, detail="артефакт ещё не готов")
    raw = arts[0]["storage_path"] or ""
    path = Path(raw.replace("volume://", "/artifacts/")) if raw else None
    if path is None or not path.exists():
        path = ARTIFACTS_DIR / f"{task_id}.md"
    if not path.exists():
        raise HTTPException(status_code=404, detail="файл артефакта не найден")
    return path.read_text(encoding="utf-8")


@app.get("/tasks/{task_id}/chain-verify")
def chain_verify(task_id: str) -> dict[str, Any]:
    try:
        return {"task_id": task_id, "chain_valid": db.verify_chain(task_id)}
    except Exception as e:  # noqa: BLE001
        raise HTTPException(status_code=500, detail=f"verify_chain: {e}") from e
