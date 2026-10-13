"""Issue #2: сверка cost ledger с relay.fast — юнит- и DB-тесты.

Юнит-часть (без БД): разбор подтверждённого расхода и расчёт расхождения.
DB-часть (при наличии PGDSN): снимок сверки пишется, view показывает алерт >5%.
"""
from __future__ import annotations

import importlib.util
import os
from decimal import Decimal
from pathlib import Path

import pytest

REPO = Path(__file__).resolve().parent.parent


def _load_reconcile():
    spec = importlib.util.spec_from_file_location(
        "reconcile_llm_cost", REPO / "scripts" / "reconcile_llm_cost.py"
    )
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


rc = _load_reconcile()


# ---------- юнит: разбор подтверждённого расхода ----------

def test_parse_bare_number():
    assert rc.parse_external("0.074") == Decimal("0.074")


def test_parse_usage_log_total_line():
    """Строка usage-логов relay.fast: `Total: $0.005508`."""
    text = "Time: 2026-10-12 18:19:26\nKey: 4 codex\nTotal: $0.005508\n"
    assert rc.parse_external(text) == Decimal("0.005508")


def test_parse_total_case_insensitive():
    assert rc.parse_external("TOTAL  $1.5") == Decimal("1.5")


def test_parse_rejects_garbage():
    with pytest.raises(ValueError):
        rc.parse_external("no numbers here")


# ---------- юнит: расхождение ----------

def test_divergence_zero_when_equal():
    assert rc.divergence_pct(Decimal("1"), Decimal("1")) == 0


def test_divergence_positive_when_local_higher():
    # local 1.2, external 1.0 -> +20%
    assert rc.divergence_pct(Decimal("1.2"), Decimal("1.0")) == Decimal("20")


def test_divergence_negative_when_local_lower():
    # local 0.8, external 1.0 -> -20%
    assert rc.divergence_pct(Decimal("0.8"), Decimal("1.0")) == Decimal("-20")


def test_divergence_external_zero_local_zero():
    assert rc.divergence_pct(Decimal("0"), Decimal("0")) == 0


def test_divergence_external_zero_local_nonzero_is_full():
    """Деления на ноль нет: при нулевом внешнем и ненулевом локальном — 100%."""
    assert rc.divergence_pct(Decimal("0.5"), Decimal("0")) == Decimal("100")


def test_threshold_is_five_percent_by_default():
    assert rc.DEFAULT_THRESHOLD_PCT == Decimal("5")


# ---------- DB: снимок сверки + view ----------

@pytest.fixture()
def db():
    dsn = os.environ.get("PGDSN") or os.environ.get("SWARM_DSN")
    if not dsn:
        pytest.skip("PGDSN/SWARM_DSN not set")
    psycopg2 = pytest.importorskip("psycopg2")
    conn = psycopg2.connect(dsn)
    yield conn
    conn.close()


def _reset(conn):
    with conn.cursor() as cur:
        cur.execute("delete from public.llm_cost_reconciliation")
        conn.commit()


def test_snapshot_stale_when_relay_unavailable(db):
    """relay.fast недоступен: снимок пишется как stale, без алерта, exit 0."""
    _reset(db)
    local = rc.local_total(db)
    code = rc.record_snapshot(db, None, local, Decimal("5"))
    assert code == 0
    with db.cursor() as cur:
        cur.execute("select stale, divergence_pct from public.llm_cost_reconciliation order by id desc limit 1")
        stale, pct = cur.fetchone()
    assert stale is True
    assert pct == 0


def test_snapshot_alert_above_threshold(db):
    """Расхождение >5% -> exit 1 и строка с alert в view.

    Тест не зависит от наполнения ledger: на чистой БД `local` = 0, поэтому
    внешний расход берётся так, чтобы расхождение гарантированно превышало порог.
    """
    _reset(db)
    local = rc.local_total(db)
    # Внешний расход заметно отличается от локального (в любую сторону).
    external = local / 2 if local > 0 else Decimal("1")
    code = rc.record_snapshot(db, external, local, Decimal("5"))
    assert code == 1
    with db.cursor() as cur:
        cur.execute("select alert, stale from public.llm_cost_divergence")
        alert, stale = cur.fetchone()
    assert alert is True
    assert stale is False


def test_snapshot_ok_within_threshold(db):
    """Расхождение в пределах порога -> exit 0, alert = false."""
    _reset(db)
    local = rc.local_total(db)
    code = rc.record_snapshot(db, local, local, Decimal("5"))
    assert code == 0
    with db.cursor() as cur:
        cur.execute("select alert from public.llm_cost_divergence")
        (alert,) = cur.fetchone()
    assert alert is False
