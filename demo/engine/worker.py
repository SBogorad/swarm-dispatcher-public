"""Демо-воркер: забирает задачу по SQL-протоколу и вырабатывает артефакт.

Честно о природе: это Python-агент демо-контура, а не Rust `swarm-worker`.
Причина — в изолированном контуре нет достижимой LLM (Ollama слушает loopback
хоста, из контейнеров недоступна), поэтому артефакт вырабатывает
детерминированный rule-based агент (`lib/agent.py`) — тот самый fallback из ТЗ
«для гарантированной демонстрации без интернета».

При этом воркер говорит с движком ТЕМИ ЖЕ публичными SQL-функциями, что и
Rust-воркер: fencing по `lease_epoch`, `heartbeat`, `save_artifact`,
`complete_task`. Координацию (аренды, эпохи, хэш-цепочку, review-gate) делает
именно движок — это и есть предмет демонстрации.

Настоящий Rust `swarm-worker` проверен на этой же БД (см. demo/README.md).
"""
from __future__ import annotations

import os
import sys
import time
import traceback
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))  # demo/ в sys.path

from db import client as db                     # noqa: E402
from lib.agent import artifact_for              # noqa: E402

WORKER_ID = os.environ.get("DEMO_WORKER_ID", "agent_w1")
ARTIFACTS_DIR = Path(os.environ.get("DEMO_ARTIFACTS_DIR", "/artifacts"))
LEASE_SEC = int(os.environ.get("DEMO_LEASE_SEC", "120"))
POLL_SEC = float(os.environ.get("DEMO_POLL_SEC", "1.0"))


def process(lease: dict) -> None:
    task_id = str(lease["out_task_id"])
    epoch = int(lease["out_epoch"])
    contract = lease["out_contract"] or {}

    # След согласия: аренда взята под конкретной эпохой (fencing).
    db.heartbeat(task_id, WORKER_ID, epoch, LEASE_SEC)

    text = artifact_for(contract)
    body = text.encode("utf-8")
    ref = db.sha256_ref(body)
    out_path = ARTIFACTS_DIR / f"{task_id}.md"
    out_path.write_text(text, encoding="utf-8")

    db.save_artifact(task_id, WORKER_ID, epoch, ref, str(out_path),
                     "text/markdown", len(body))
    state = db.complete(task_id, WORKER_ID, epoch, ref, 0)
    print(f"[worker] {WORKER_ID} {task_id}: -> {state}", flush=True)


def main() -> None:
    ARTIFACTS_DIR.mkdir(parents=True, exist_ok=True)
    for attempt in range(30):
        try:
            db.register_worker(WORKER_ID)
            break
        except Exception as e:  # noqa: BLE001
            print(f"[worker] ждём БД ({attempt}): {e}", flush=True)
            time.sleep(2)
    else:
        print("[worker] БД недоступна, выход", flush=True)
        sys.exit(1)

    print(f"[worker] {WORKER_ID} готов, lease={LEASE_SEC}s", flush=True)
    while True:
        try:
            lease = db.claim(WORKER_ID, LEASE_SEC)
            if lease is None:
                time.sleep(POLL_SEC)
                continue
            process(lease)
        except Exception as e:  # noqa: BLE001
            print(f"[worker] ошибка цикла: {e}", flush=True)
            traceback.print_exc()
            time.sleep(POLL_SEC)


if __name__ == "__main__":
    main()
