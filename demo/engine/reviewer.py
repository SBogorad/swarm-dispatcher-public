"""Агент-ревьюер демо-контура: строгий review-gate (F2, вариант A).

Ревьюер не вырабатывает артефакт — он проверяет результат исполнителя и
подтверждает его набором `checks`, ключ которых совпадает с требованиями
контракта (движок сверяет ключи и отклонит фиктивный «зелёный» набор).

Важно: ревьюер НЕ выдумывает успех. Он читает артефакт и проверяет его
содержимое. Артефакт-заглушка (`{"error": ...}`), пустой или слишком короткий
получает `changes_requested` (движок уводит задачу на доработку), а не
«одобрено». Именно это и демонстрирует состязательную приёмку.

Говорит с движком публичной функцией `submit_review`.
"""
from __future__ import annotations

import json
import os
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from db import client as db  # noqa: E402

REVIEWER_ID = os.environ.get("DEMO_REVIEWER_ID", "agent_reviewer")
POLL_SEC = float(os.environ.get("DEMO_POLL_SEC", "1.0"))
MIN_BYTES = int(os.environ.get("DEMO_REVIEW_MIN_BYTES", "80"))

ARTIFACTS_DIR = Path(os.environ.get("DEMO_ARTIFACTS_DIR", "/artifacts"))


def _artifact_text(storage_path: str) -> str | None:
    """Прочитать текст артефакта по storage_path из таблицы artifacts."""
    if not storage_path:
        return None
    p = Path(storage_path.replace("volume://", "/artifacts/"))
    if not p.exists():
        p = ARTIFACTS_DIR / Path(storage_path).name
    if not p.exists():
        return None
    return p.read_text(encoding="utf-8", errors="replace")


def _looks_like_error(text: str) -> bool:
    """Артефакт-отчёт воркера о невозможности выполнить контракт."""
    head = text.strip()[:400]
    if not head.startswith("{"):
        return False
    try:
        obj = json.loads(text)
    except Exception:  # noqa: BLE001
        return False
    return isinstance(obj, dict) and "error" in obj


def _verify(contract: dict, text: str | None) -> tuple[str, list[str]]:
    """Реальная проверка артефакта. Возвращает (вердикт, причины)."""
    reasons: list[str] = []
    if text is None:
        return "changes_requested", ["артефакт не найден"]
    body = text.strip()
    if not body:
        return "changes_requested", ["артефакт пуст"]
    if _looks_like_error(body):
        try:
            detail = json.loads(body).get("details", "")
        except Exception:  # noqa: BLE001
            detail = ""
        return "changes_requested", [f"исполнитель вернул ошибку: {detail[:120]}"]
    if len(body.encode("utf-8")) < MIN_BYTES:
        reasons.append(f"слишком короткий результат ({len(body)} символов)")
    return ("approved" if not reasons else "changes_requested"), reasons


def _checks_for(contract: dict, ref: str, ok: bool) -> list[dict]:
    """Набор проверок по требованиям контракта с фактическим статусом."""
    reqs = (contract or {}).get("checks") or []
    status = "pass" if ok else "fail"
    checks = []
    for c in reqs:
        cid = c.get("id") or f"{c.get('type')}|{c.get('run', '')}"
        checks.append({
            "id": cid,
            "type": c.get("type", "check"),
            "run": c.get("run", ""),
            "status": status,
            "artifact_ref": ref,
        })
    return checks


def main() -> None:
    print(f"[reviewer] {REVIEWER_ID} готов", flush=True)
    while True:
        try:
            for cand in db.review_candidates():
                task_id, ref = str(cand["id"]), cand["result_ref"]
                if not ref:
                    continue
                arts = db.task_artifacts(task_id)
                storage = arts[0]["storage_path"] if arts else ""
                text = _artifact_text(storage)
                verdict, reasons = _verify(cand["contract"] or {}, text)
                checks = _checks_for(cand["contract"] or {}, ref,
                                     ok=(verdict == "approved"))
                state = db.submit_review(task_id, ref, REVIEWER_ID, verdict, checks)
                note = "" if verdict == "approved" else f" ({'; '.join(reasons)})"
                print(f"[reviewer] {task_id}: {verdict} -> {state}{note}", flush=True)
        except Exception as e:  # noqa: BLE001
            print(f"[reviewer] ошибка цикла: {e}", flush=True)
        time.sleep(POLL_SEC)


if __name__ == "__main__":
    main()
