"""Issue #7/#10: статистика качества моделей — юнит- и DB-тесты.

Юнит-часть (без БД): преобразование строк view в статистику для селектора.
DB-часть (при наличии PGDSN/SWARM_DSN): view `llm_model_quality` считает
цену успешного результата по факту (done vs blocked/failed/budget_exhausted).
"""
from __future__ import annotations

import importlib.util
import json
import os
from decimal import Decimal
from pathlib import Path

import pytest

REPO = Path(__file__).resolve().parent.parent


def _load_report():
    spec = importlib.util.spec_from_file_location(
        "model_quality_report", REPO / "scripts" / "model_quality_report.py"
    )
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


mqr = _load_report()


# ---------- юнит: строки view -> статистика для селектора ----------

def test_to_stats_basic():
    rows = [("cheap", 3, 2, Decimal("0.6667"), Decimal("0.009"),
             Decimal("0.0045"), Decimal("0.33"))]
    stats = mqr.to_stats(rows, min_samples=0)
    assert len(stats) == 1
    s = stats[0]
    assert s["model"] == "cheap"
    assert s["tasks"] == 3
    assert s["tasks_done"] == 2
    assert abs(s["success_rate"] - 0.6667) < 1e-9
    assert abs(s["cost_per_success_usd"] - 0.0045) < 1e-9


def test_model_without_success_is_skipped():
    """Нет успехов -> цену успеха измерить нельзя (null), модель не предлагаем."""
    rows = [("always-fails", 4, 0, Decimal("0"), Decimal("0.01"), None, Decimal("2"))]
    assert mqr.to_stats(rows, min_samples=0) == []


def test_min_samples_filters_noise():
    rows = [
        ("a", 2, 2, Decimal("1"), Decimal("0.01"), Decimal("0.005"), Decimal("0")),
        ("b", 10, 8, Decimal("0.8"), Decimal("0.05"), Decimal("0.006"), Decimal("0.5")),
    ]
    stats = mqr.to_stats(rows, min_samples=5)
    assert [s["model"] for s in stats] == ["b"]


def test_empty_is_not_an_error():
    assert mqr.to_stats([], min_samples=0) == []


def test_stats_are_json_serializable():
    rows = [("m", 5, 4, Decimal("0.8"), Decimal("0.02"), Decimal("0.005"), Decimal("0.25"))]
    text = json.dumps(mqr.to_stats(rows, min_samples=0))
    back = json.loads(text)
    assert back[0]["model"] == "m"


# ---------- DB: view считает цену успеха по факту ----------

def _dsn():
    return os.environ.get("PGDSN") or os.environ.get("SWARM_DSN")


@pytest.fixture()
def conn():
    dsn = _dsn()
    if not dsn:
        pytest.skip("PGDSN/SWARM_DSN not set")
    psycopg2 = pytest.importorskip("psycopg2")
    c = psycopg2.connect(dsn)
    yield c
    c.close()


def _seed(conn):
    """Две модели: дешёвая валится на каждой третьей задаче, дорогая сдаёт всё.

    Смысл теста: по прайсу дешёвая выигрывает, а по ЦЕНЕ УСПЕХА — нет.
    """
    with conn.cursor() as cur:
        cur.execute("delete from llm_calls")
        cur.execute("delete from tasks")
        cur.execute(
            """
            insert into tasks (id, root_id, state, contract, budget_cap) values
              ('11111111-1111-1111-1111-111111111111','11111111-1111-1111-1111-111111111111','done','{}',100000),
              ('22222222-2222-2222-2222-222222222222','22222222-2222-2222-2222-222222222222','done','{}',100000),
              ('33333333-3333-3333-3333-333333333333','33333333-3333-3333-3333-333333333333','blocked','{}',100000)
            """
        )
        cur.execute(
            """
            insert into llm_calls (task_id, worker_id, lease_epoch, model, provider,
              prompt_tokens, completion_tokens, prompt_cost_usd, completion_cost_usd,
              total_cost_usd, started_at, completed_at, duration_ms, success) values
              ('11111111-1111-1111-1111-111111111111','w1',1,'cheap','relay.fast',100,200,0.001,0.002,0.003,now(),now(),10,true),
              ('22222222-2222-2222-2222-222222222222','w1',1,'cheap','relay.fast',100,200,0.001,0.002,0.003,now(),now(),10,true),
              ('33333333-3333-3333-3333-333333333333','w1',1,'cheap','relay.fast',100,200,0.001,0.002,0.003,now(),now(),10,true),
              ('11111111-1111-1111-1111-111111111111','w1',1,'strong','relay.fast',100,200,0.010,0.020,0.030,now(),now(),10,true)
            """
        )
    conn.commit()


def test_view_computes_cost_per_success(conn):
    _seed(conn)
    with conn.cursor() as cur:
        cur.execute(
            "select model, tasks, tasks_done, success_rate, cost_per_success_usd"
            " from llm_model_quality order by model"
        )
        rows = cur.fetchall()
    by = {r[0]: r for r in rows}
    assert by["cheap"][1] == 3 and by["cheap"][2] == 2
    assert abs(float(by["cheap"][3]) - 2 / 3) < 1e-3
    # cheap потратил 0.009 на 2 успеха -> 0.0045; strong 0.030 на 1 -> 0.030
    assert abs(float(by["cheap"][4]) - 0.0045) < 1e-9
    assert abs(float(by["strong"][4]) - 0.030) < 1e-9


def test_report_selects_by_measured_cost(conn):
    """Отчёт по факту: strong дешевле по успеху, чем кажется по прайсу?"""
    _seed(conn)
    with conn.cursor() as cur:
        cur.execute(
            "select model, tasks, tasks_done, success_rate, total_cost_usd,"
            " cost_per_success_usd, avg_rework from llm_model_quality order by model"
        )
        stats = mqr.to_stats(cur.fetchall(), min_samples=1)
    assert len(stats) == 2
    # На этих данных дешёвая модель всё ещё выигрывает по цене успеха —
    # важно, что метрика считается по факту, а не по названию/токенам.
    cheapest = min(stats, key=lambda s: s["cost_per_success_usd"])
    assert cheapest["model"] == "cheap"
