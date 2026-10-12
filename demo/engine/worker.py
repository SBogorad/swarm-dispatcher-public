"""Демо-воркер: забирает задачу по SQL-протоколу, анализирует Excel, пишет справку.

Честно о природе: это Python-воркер демо-стенда, а не Rust `swarm-worker`.
Причина — в текущем ядре (v0.5.3) `Sandbox::start`/`agent_loop::run` к циклу
воркера НЕ подключены (см. docs/KNOWN_LIMITATIONS §5.6/§5.11), а в образе
`agent-sandbox` нет Python. Поэтому анализ выполняет этот процесс, но говорит с
движком теми же публичными SQL-функциями: fencing по `lease_epoch`, `heartbeat`,
`save_artifact`, `complete_task`. Канбан в UI читает те же `tasks`/`events`.

Аналитика — локальная (pandas), сеть не используется. Формулировка справки —
локальная Ollama при наличии, иначе детерминированный rule-based шаблон.
"""
from __future__ import annotations

import os
import sys
import time
import traceback
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))  # demo/ в sys.path

from db import client as db          # noqa: E402
from lib.analyze import analyze      # noqa: E402
from lib.summarize import summarize  # noqa: E402

WORKER_ID = os.environ.get("DEMO_WORKER_ID", "demo_w1")
ARTIFACTS_DIR = Path(os.environ.get("DEMO_ARTIFACTS_DIR", "/artifacts"))
LEASE_SEC = int(os.environ.get("DEMO_LEASE_SEC", "120"))
POLL_SEC = float(os.environ.get("DEMO_POLL_SEC", "1.5"))
PREFER_LLM = os.environ.get("DEMO_PREFER_LLM", "1") == "1"


def _input_path(contract: dict) -> str | None:
    return (contract.get("input") or {}).get("path")


def _question(contract: dict) -> str:
    return (contract.get("input") or {}).get("question", "") or ""


def process(lease: dict) -> None:
    task_id = str(lease["out_task_id"])
    epoch = int(lease["out_epoch"])
    contract = lease["out_contract"] or {}
    path = _input_path(contract)
    if not path or not Path(path).exists():
        ref = db.sha256_ref(f"missing_input:{task_id}".encode())
        out = ARTIFACTS_DIR / f"{task_id}.missing.txt"
        out.write_text(
            "Не найден входной файл отчёта.\n"
            f"Ожидался путь: {path}\n", encoding="utf-8")
        db.fail(task_id, WORKER_ID, epoch, None, "missing_input")
        print(f"[worker] {task_id}: нет входного файла {path}", flush=True)
        return

    db.heartbeat(task_id, WORKER_ID, epoch, LEASE_SEC)
    metrics = analyze(path)
    result = summarize(metrics, _question(contract), prefer_llm=PREFER_LLM)

    text = result["text"]
    body = text.encode("utf-8")
    ref = db.sha256_ref(body)
    out_path = ARTIFACTS_DIR / f"{task_id}.md"
    out_path.write_text(text, encoding="utf-8")

    db.save_artifact(task_id, WORKER_ID, epoch, ref, str(out_path),
                     "text/markdown", len(body))
    # Токены: у rule-based — 0; у Ollama — оценка (грубо, по длине ответа).
    tokens = 0 if result["engine"] == "rule-based" else max(1, len(body) // 4)
    state = db.complete(task_id, WORKER_ID, epoch, ref, tokens)
    print(f"[worker] {task_id}: engine={result['engine']} -> {state}", flush=True)


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

    print(f"[worker] {WORKER_ID} готов, lease={LEASE_SEC}s, LLM={PREFER_LLM}", flush=True)
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
