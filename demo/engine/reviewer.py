"""Агент-ревьюер демо-контура: строгий review-gate (F2, вариант A).

Ревьюер не вырабатывает артефакт — он проверяет результат исполнителя и
подтверждает его набором `checks`, ключ которых совпадает с требованиями
контракта (движок сверяет ключи и отклонит фиктивный «зелёный» набор).

Это и есть предмет демонстрации: задача не закрывается «на слово».
Self-review запрещён (движок: создатель артефакта ≠ ревьюер, RV002), а
несовпадение ключей проверок даёт `checks_failed` (RV003).

Говорит с движком публичной функцией `submit_review`.
"""
from __future__ import annotations

import os
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from db import client as db  # noqa: E402

REVIEWER_ID = os.environ.get("DEMO_REVIEWER_ID", "agent_reviewer")
POLL_SEC = float(os.environ.get("DEMO_POLL_SEC", "1.0"))


def _checks_for(contract: dict, ref: str) -> list[dict]:
    """Собрать подтверждающие проверки по требованиям контракта."""
    reqs = (contract or {}).get("checks") or []
    checks = []
    for c in reqs:
        cid = c.get("id") or f"{c.get('type')}|{c.get('run', '')}"
        checks.append({
            "id": cid,
            "type": c.get("type", "check"),
            "run": c.get("run", ""),
            "status": "pass",
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
                checks = _checks_for(cand["contract"] or {}, ref)
                state = db.submit_review(task_id, ref, REVIEWER_ID, "approved", checks)
                print(f"[reviewer] {task_id}: approved -> {state}", flush=True)
        except Exception as e:  # noqa: BLE001
            print(f"[reviewer] ошибка цикла: {e}", flush=True)
        time.sleep(POLL_SEC)


if __name__ == "__main__":
    main()
