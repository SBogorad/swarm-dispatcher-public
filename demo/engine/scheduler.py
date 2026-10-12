"""Планировщик демо-стенда: роль watchdog в миниатюре.

В боевом контуре `pending → ready` переводит сторож (`promote_ready`),
а истёкшие аренды отбирает `reclaim_expired`. В демо-составе нет отдельного
Rust-сторожа, поэтому эти две публичные функции вызывает этот цикл. Никакой
логики он не добавляет — только дёргает функции движка по расписанию.
"""
from __future__ import annotations

import os
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from db import client as db  # noqa: E402

INTERVAL = float(os.environ.get("DEMO_SCHED_INTERVAL", "1.0"))


def main() -> None:
    print("[scheduler] promote_ready/reclaim_expired запущены", flush=True)
    while True:
        try:
            n = db.promote_ready()
            if n:
                print(f"[scheduler] promoted {n}", flush=True)
            with db._conn() as c:  # noqa: SLF001 — демо-утилита
                r = c.execute("select reclaim_expired()").fetchone()
            if r and r[0]:
                print(f"[scheduler] reclaimed {r[0]}", flush=True)
        except Exception as e:  # noqa: BLE001
            print(f"[scheduler] ошибка: {e}", flush=True)
        time.sleep(INTERVAL)


if __name__ == "__main__":
    main()
