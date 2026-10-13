#!/usr/bin/env python3
"""Сверка локального cost ledger с подтверждённым расходом relay.fast (Issue #2).

Зачем: `llm_calls` считает расход по **оценкам** цен (`llm_cost.rs`), а
подтверждённый расход живёт только в web-интерфейсе relay.fast — публичного
API баланса/usage нет (см. исследование в Issue #2). Этот скрипт фиксирует
снимок сверки и кричит, если расхождение превышает порог.

Режимы:
  * подтверждённый расход передан (`--external-usd` или `--external-file`) —
    считается расхождение, снимок пишется как свежий;
  * relay.fast недоступен — снимок помечается `stale = true`, локальный расход
    сохраняется как последнее известное состояние, скрипт НЕ падает (Issue #2:
    «показывать последнее подтверждённое состояние + timestamp + признак
    устаревших данных»).

Код возврата: 0 — ок или устаревание; 1 — расхождение > порога (алерт); 2 — ошибка.

Переменные окружения:
  SWARM_DSN / DATABASE_URL — DSN (обязательно)
  RECONCILE_THRESHOLD_PCT  — порог расхождения в процентах (по умолчанию 5)
"""
from __future__ import annotations

import argparse
import os
import re
import sys
from decimal import Decimal

try:
    import psycopg2
except ImportError:  # pragma: no cover
    print("ERROR: psycopg2 required (pip install psycopg2-binary)", file=sys.stderr)
    sys.exit(2)

DEFAULT_THRESHOLD_PCT = Decimal("5")


def dsn() -> str:
    d = os.environ.get("SWARM_DSN") or os.environ.get("DATABASE_URL")
    if not d:
        print("ERROR: SWARM_DSN or DATABASE_URL must be set", file=sys.stderr)
        sys.exit(2)
    return d


def local_total(conn) -> Decimal:
    with conn.cursor() as cur:
        cur.execute("select coalesce(sum(total_cost_usd), 0) from public.llm_calls")
        return Decimal(cur.fetchone()[0])


def parse_external(text: str) -> Decimal:
    """Достать подтверждённый расход из текста/HTML usage-логов relay.fast.

    Ищем либо голое число (`--external-usd`), либо строку вида `Total: $0.074`.
    """
    text = text.strip()
    # Голое число.
    try:
        return Decimal(text)
    except Exception:  # noqa: BLE001
        pass
    # Строка с суммой: `Total ... $0.074` (регистр не важен).
    m = re.search(r"total[^$]*\$\s*([0-9]+(?:\.[0-9]+)?)", text, re.IGNORECASE)
    if m:
        return Decimal(m.group(1))
    raise ValueError(f"не удалось разобрать внешний расход из: {text[:200]!r}")


def divergence_pct(local: Decimal, external: Decimal) -> Decimal:
    """(local - external) / external * 100.

    Внешний ноль трактуем особо: если локально тоже ноль — 0%, иначе 100%
    (нельзя делить на ноль, а расхождение очевидно).
    """
    if external == 0:
        return Decimal("0") if local == 0 else Decimal("100")
    return (local - external) / external * Decimal("100")


def record_snapshot(conn, external: Decimal | None, local: Decimal, threshold: Decimal) -> int:
    with conn.cursor() as cur:
        if external is None:
            cur.execute(
                "insert into public.llm_cost_reconciliation "
                "(external_total_usd, local_total_usd, divergence_pct, stale, note) "
                "values (%s, %s, %s, true, %s) returning id",
                (local, local, 0, "relay.fast недоступен: показано последнее известное состояние"),
            )
            conn.commit()
            print(f"STALE: relay.fast недоступен; локальный расход ${local} зафиксирован как последнее известное")
            return 0
        pct = divergence_pct(local, external)
        cur.execute(
            "insert into public.llm_cost_reconciliation "
            "(external_total_usd, local_total_usd, divergence_pct, stale, note) "
            "values (%s, %s, %s, false, %s) returning id",
            (external, local, pct, None),
        )
        conn.commit()
    print(f"local=${local}  external=${external}  divergence={pct:.4f}%  threshold={threshold}%")
    if abs(pct) > threshold:
        print(f"ALERT: расхождение {pct:.4f}% превышает порог {threshold}% (Issue #2)")
        return 1
    print("OK: расхождение в пределах порога")
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description="Сверка cost ledger с relay.fast (Issue #2)")
    ap.add_argument("--external-usd", help="подтверждённый расход relay.fast (число)")
    ap.add_argument("--external-file", help="файл с подтверждённым расходом (число или HTML/текст)")
    ap.add_argument(
        "--threshold-pct",
        type=Decimal,
        default=Decimal(os.environ.get("RECONCILE_THRESHOLD_PCT", str(DEFAULT_THRESHOLD_PCT))),
    )
    args = ap.parse_args()

    external: Decimal | None = None
    if args.external_usd is not None:
        external = Decimal(args.external_usd)
    elif args.external_file:
        with open(args.external_file, encoding="utf-8") as f:
            external = parse_external(f.read())

    conn = psycopg2.connect(dsn())
    try:
        local = local_total(conn)
        return record_snapshot(conn, external, local, args.threshold_pct)
    finally:
        conn.close()


if __name__ == "__main__":
    sys.exit(main())
