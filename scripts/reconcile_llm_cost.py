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


def fetch_relay_usage(api_key: str, base_url: str = "https://relay.fast",
                      timeout: float = 20.0) -> Decimal | None:
    """Подтверждённый расход ключа через эндпоинт relay.fast.

    `GET /v1/dashboard/billing/usage` -> `{"object":"list","total_usage":0.2342}`
    (проверено: HTTP 200). Публичной документации нет — эндпоинт найден
    эмпирически, поэтому недоступность не ошибка: возвращаем None -> stale.
    """
    import json
    import urllib.request

    req = urllib.request.Request(
        base_url.rstrip("/") + "/v1/dashboard/billing/usage",
        headers={"Authorization": f"Bearer {api_key}"},
    )
    try:
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            payload = json.loads(resp.read().decode("utf-8"))
    except Exception as exc:  # noqa: BLE001
        print(f"WARN: relay.fast недоступен: {exc}", file=sys.stderr)
        return None
    if not isinstance(payload, dict) or "total_usage" not in payload:
        print(f"WARN: неожиданный ответ relay.fast: {payload!r}", file=sys.stderr)
        return None
    return Decimal(str(payload["total_usage"]))


def previous_confirmed(conn) -> tuple[Decimal, Decimal] | None:
    """(external_total, local_total) последнего подтверждённого снимка, или None."""
    with conn.cursor() as cur:
        cur.execute(
            "select external_total_usd, local_total_usd from public.llm_cost_reconciliation "
            "where not stale order by observed_at desc, id desc limit 1"
        )
        row = cur.fetchone()
    return (Decimal(row[0]), Decimal(row[1])) if row else None


def record_snapshot(conn, external: Decimal | None, local: Decimal, threshold: Decimal) -> int:
    """Записать снимок сверки; вернуть код (0 — ок/stale/baseline, 1 — алерт).

    Оконная логика (Issue #2): `total_usage` relay.fast накопительный за всё
    время жизни ключа, поэтому первый подтверждённый снимок — базовая точка
    (расхождение не считается). Последующие сравнивают приращение внешнего
    расхода с приращением локального.
    """
    prev = previous_confirmed(conn)
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
        if prev is None:
            # Базовая точка: абсолютные суммы несопоставимы (история ключа vs ledger).
            cur.execute(
                "insert into public.llm_cost_reconciliation "
                "(external_total_usd, local_total_usd, divergence_pct, stale, "
                " window_external_usd, window_local_usd, is_baseline, note) "
                "values (%s, %s, %s, false, %s, %s, true, %s) returning id",
                (external, local, 0, external, local,
                 "базовая точка: абсолютные суммы несопоставимы, сверка идёт по окну"),
            )
            conn.commit()
            print(f"BASELINE: external=${external}  local=${local} (базовая точка, расхождение не считается)")
            return 0

        prev_ext, prev_loc = prev
        win_ext = external - prev_ext
        win_loc = local - prev_loc
        # Отрицательное приращение внешнего расхода — рассинхрон источника
        # (сброс/смена ключа), окно не считается.
        if win_ext < 0:
            cur.execute(
                "insert into public.llm_cost_reconciliation "
                "(external_total_usd, local_total_usd, divergence_pct, stale, note) "
                "values (%s, %s, %s, true, %s) returning id",
                (external, local, 0, "приращение внешнего расхода отрицательное: источник рассинхронизирован"),
            )
            conn.commit()
            print(f"STALE: внешний расход уменьшился (${prev_ext} -> ${external}); окно пропущено")
            return 0

        pct = divergence_pct(win_loc, win_ext)
        cur.execute(
            "insert into public.llm_cost_reconciliation "
            "(external_total_usd, local_total_usd, divergence_pct, stale, "
            " window_external_usd, window_local_usd, is_baseline, note) "
            "values (%s, %s, %s, false, %s, %s, false, %s) returning id",
            (external, local, pct, win_ext, win_loc, None),
        )
        conn.commit()
    print(
        f"window: local=+${win_loc}  external=+${win_ext}  "
        f"divergence={pct:.4f}%  threshold={threshold}%"
    )
    if abs(pct) > threshold:
        print(f"ALERT: расхождение {pct:.4f}% превышает порог {threshold}% (Issue #2)")
        return 1
    print("OK: расхождение в пределах порога")
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description="Сверка cost ledger с relay.fast (Issue #2)")
    ap.add_argument("--external-usd", help="подтверждённый расход relay.fast (число)")
    ap.add_argument("--external-file", help="файл с подтверждённым расходом (число или HTML/текст)")
    ap.add_argument("--fetch", action="store_true",
                    help="взять расход с relay.fast (/v1/dashboard/billing/usage)")
    ap.add_argument("--api-key", default=os.environ.get("RELAY_API_KEY", ""),
                    help="ключ relay.fast (по умолчанию $RELAY_API_KEY)")
    ap.add_argument("--base-url", default=os.environ.get("RELAY_BASE_URL", "https://relay.fast"))
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
    elif args.fetch:
        if not args.api_key:
            print("WARN: --fetch без --api-key/$RELAY_API_KEY — считаем relay.fast недоступным",
                  file=sys.stderr)
        else:
            external = fetch_relay_usage(args.api_key, args.base_url)

    conn = psycopg2.connect(dsn())
    try:
        local = local_total(conn)
        return record_snapshot(conn, external, local, args.threshold_pct)
    finally:
        conn.close()


if __name__ == "__main__":
    sys.exit(main())
