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


def test_first_confirmed_snapshot_is_baseline(db):
    """Первый подтверждённый снимок — базовая точка, без алерта (Issue #2).

    Абсолютные суммы несопоставимы: `total_usage` relay.fast накопительный за
    всё время жизни ключа, а ledger считает с момента появления.
    """
    _reset(db)
    local = rc.local_total(db)
    code = rc.record_snapshot(db, local + Decimal("1"), local, Decimal("5"))
    assert code == 0
    with db.cursor() as cur:
        cur.execute("select is_baseline, alert from public.llm_cost_divergence")
        baseline, alert = cur.fetchone()
    assert baseline is True
    assert alert is False


def test_window_divergence_alert_above_threshold(db):
    """Приращение расходится >5% -> exit 1 и alert в view."""
    _reset(db)
    local = rc.local_total(db)
    # Базовая точка.
    assert rc.record_snapshot(db, Decimal("10"), local, Decimal("5")) == 0
    # Окно: внешне +1.0, локально +0.1 -> расхождение -90%.
    code = rc.record_snapshot(db, Decimal("11"), local + Decimal("0.1"), Decimal("5"))
    assert code == 1
    with db.cursor() as cur:
        cur.execute("select alert, stale, is_baseline from public.llm_cost_divergence")
        alert, stale, baseline = cur.fetchone()
    assert alert is True
    assert stale is False
    assert baseline is False


def test_window_ok_within_threshold(db):
    """Приращения совпадают -> exit 0, alert = false."""
    _reset(db)
    local = rc.local_total(db)
    assert rc.record_snapshot(db, Decimal("10"), local, Decimal("5")) == 0
    code = rc.record_snapshot(db, Decimal("10.001"), local + Decimal("0.001"), Decimal("5"))
    assert code == 0
    with db.cursor() as cur:
        cur.execute("select alert from public.llm_cost_divergence")
        (alert,) = cur.fetchone()
    assert alert is False


def test_negative_window_is_stale_not_alert(db):
    """Внешний расход уменьшился (сброс/смена ключа) -> stale, без алерта."""
    _reset(db)
    local = rc.local_total(db)
    assert rc.record_snapshot(db, Decimal("10"), local, Decimal("5")) == 0
    code = rc.record_snapshot(db, Decimal("5"), local + Decimal("1"), Decimal("5"))
    assert code == 0
    with db.cursor() as cur:
        cur.execute("select stale, alert from public.llm_cost_divergence")
        stale, alert = cur.fetchone()
    assert stale is True
    assert alert is False


def test_view_deterministic_when_observed_at_ties(db):
    """Одинаковый `observed_at` (снимки в одной транзакции) — view берёт
    последнюю строку по `id`, а не произвольную.

    Регрессия гейта #2c: `order by observed_at desc` без тайбрейкера давал
    недетерминированный результат, и `stale` терялся. `now()` в PostgreSQL —
    время начала транзакции, поэтому два снимка в одной транзакции имеют
    идентичный `observed_at`.
    """
    _reset(db)
    local = rc.local_total(db)
    with db.cursor() as cur:
        # Оба снимка в ОДНОЙ транзакции -> одинаковый now().
        cur.execute(
            "insert into public.llm_cost_reconciliation "
            "(external_total_usd, local_total_usd, divergence_pct, stale, is_baseline) "
            "values (10, 0, 0, false, true), (5, 1, 0, true, false) returning id, observed_at"
        )
        rows = cur.fetchall()
    db.commit()
    assert rows[0][1] == rows[1][1], "тест требует одинаковый observed_at (tie)"
    with db.cursor() as cur:
        cur.execute("select stale, is_baseline, alert from public.llm_cost_divergence")
        stale, baseline, alert = cur.fetchone()
    # Последняя по id — stale-строка, она и должна быть выбрана.
    assert stale is True, "view обязан брать последнюю строку по id при tie observed_at"
    assert baseline is True  # последний подтверждённый снимок остаётся базовой точкой
    assert alert is False


# ---------- юнит: разбор ответа relay.fast ----------

def test_parse_relay_usage_payload():
    """Реальный ответ эндпоинта: {"object":"list","total_usage":0.2342}."""
    import json as _json
    assert _json.loads('{"object":"list","total_usage":0.2342}')["total_usage"] == 0.2342


def test_fetch_relay_usage_returns_none_on_failure():
    """Недоступный relay.fast -> None (stale), без исключения."""
    assert rc.fetch_relay_usage("dummy", base_url="http://127.0.0.1:1", timeout=0.5) is None
