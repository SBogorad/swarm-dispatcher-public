"""FastAPI-мост демо-стенда: приём файла → задача в движке → статус/результат.

Мост сознательно тонкий: он НЕ анализирует Excel и не пишет справку. Его дело —
принять файл, положить его в каталог артефактов, создать задачу публичной
функцией `add_task` и отдавать UI состояние жизненного цикла из `tasks`/`events`.
Всю работу делает демо-воркер (`demo/engine/worker.py`) по SQL-протоколу.
"""
from __future__ import annotations

import os
import sys
import uuid
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

from fastapi import FastAPI, File, Form, HTTPException, UploadFile
from fastapi.responses import PlainTextResponse

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from db import client as db  # noqa: E402

ARTIFACTS_DIR = Path(os.environ.get("DEMO_ARTIFACTS_DIR", "/artifacts"))
UPLOADS_DIR = ARTIFACTS_DIR / "uploads"

app = FastAPI(title="ЦГЦУ Ассистент — демо-мост", version="0.1.0")

TERMINAL = {"done", "blocked"}


def _iso(v: Any) -> Any:
    if isinstance(v, datetime):
        return v.astimezone(timezone.utc).isoformat()
    return v


@app.get("/healthz")
def healthz() -> dict[str, str]:
    return {"status": "ok"}


@app.post("/tasks")
async def create_task(
    question: str = Form(""),
    file: UploadFile = File(...),
) -> dict[str, str]:
    """Принять Excel-отчёт и поставить задачу в рой."""
    task_id = str(uuid.uuid4())
    UPLOADS_DIR.mkdir(parents=True, exist_ok=True)
    suffix = Path(file.filename or "report.xlsx").suffix or ".xlsx"
    path = UPLOADS_DIR / f"{task_id}{suffix}"
    data = await file.read()
    if not data:
        raise HTTPException(status_code=400, detail="пустой файл")
    path.write_bytes(data)

    contract = {
        "description": (
            "Сформировать управленческую справку по суточному отчёту ЦГЦУ. "
            f"Вопрос пользователя: {question or 'общая сводка'}"
        ),
        "kind": "analytics_memo",
        "input": {"path": str(path), "question": question, "filename": file.filename},
        "checks": [
            {"id": "memo", "type": "manual", "run": "memo_present"},
            {"id": "numbers", "type": "test", "run": "numbers_from_source"},
        ],
        "budget": {"attempts": 3, "max_rework": 2, "max_tasks": 200},
    }
    try:
        db.add_task(task_id, contract, budget_cap=200_000)
    except Exception as e:  # noqa: BLE001
        raise HTTPException(status_code=500, detail=f"add_task: {e}") from e
    return {"task_id": task_id, "filename": path.name}


@app.get("/tasks/{task_id}")
def get_task(task_id: str) -> dict[str, Any]:
    st = db.task_state(task_id)
    if st is None:
        raise HTTPException(status_code=404, detail="задача не найдена")
    events = db.task_events(task_id)
    memo = None
    if st.get("result_ref"):
        p = ARTIFACTS_DIR / f"{task_id}.md"
        if p.exists():
            memo = p.read_text(encoding="utf-8")
    created, updated = st.get("created_at"), st.get("updated_at")
    elapsed = None
    if isinstance(created, datetime) and isinstance(updated, datetime):
        elapsed = round((updated - created).total_seconds(), 2)
    return {
        "task_id": task_id,
        "state": st["state"],
        "terminal": st["state"] in TERMINAL,
        "attempts": st["attempts"],
        "rework": st["rework"],
        "budget_cap": st["budget_cap"],
        "spent_tokens": st["spent_tokens"],
        "result_ref": st["result_ref"],
        "block_reason": st["block_reason"],
        "created_at": _iso(created),
        "updated_at": _iso(updated),
        "elapsed_sec": elapsed,
        "events": [
            {"seq": e["seq"], "kind": e["kind"], "actor": e["actor"],
             "payload": e["payload"], "at": _iso(e["at"])}
            for e in events
        ],
        "memo": memo,
    }


@app.get("/tasks/{task_id}/memo", response_class=PlainTextResponse)
def get_memo(task_id: str) -> str:
    p = ARTIFACTS_DIR / f"{task_id}.md"
    if not p.exists():
        raise HTTPException(status_code=404, detail="справка ещё не готова")
    return p.read_text(encoding="utf-8")
