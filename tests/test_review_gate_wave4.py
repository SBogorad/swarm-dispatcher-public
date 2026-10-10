#!/usr/bin/env python3
"""Тесты-арбитры волны 4 (F2, F15) — аудит swarm-dispatcher v0.4.

Назначение: КРАСНЫЕ на текущем стеке, ЗЕЛЁНЫЕ после правок F2/F15.

Запуск (против поднятого стека schema -> roles -> roles_2b, БЕЗ патча review):
    PGPASSWORD=... python3 -m pytest -q tests/test_review_gate_wave4.py

Требования: psycopg2, переменные окружения как в acceptance_harness.sh
(PGDSN или ROLES_*). Файл использует суперпользовательский DSN (как test_v0.py).

Механика проверки: каждый тест СНАЧАЛА утверждает текущее (дефектное) поведение
в маркере comment, но ассертит ЖЕЛАЕМОЕ. На коде до правки тест падает — это
и есть красный лог. После правки F2/F15 — зелёный. Откат правки снова краснит.
"""
import os
import uuid

import psycopg2
import pytest


def _dsn() -> str:
    return os.environ.get(
        "PGDSN",
        "dbname=c2_swarm user=postgres host=127.0.0.1 port=5432 password=pw",
    )


@pytest.fixture()
def conn():
    c = psycopg2.connect(_dsn())
    c.autocommit = False
    yield c
    c.rollback()
    c.close()


def _mk_task_in_review(cur, checks, result_ref="r1"):
    """tasks в состоянии review с артефактом и result_ref (two-phase, FK-safe)."""
    t = str(uuid.uuid4())
    cur.execute(
        """insert into tasks(id, root_id, state, contract, priority, attempts, budget_cap)
           values (%s,%s,'review',%s::jsonb,0,1,1000)""",
        (t, t, _contract(checks)),
    )
    cur.execute(
        """insert into artifacts(task_id, ref, created_by, lease_epoch, storage_path)
           values (%s,%s,'author',0,'/tmp/'||%s)""",
        (t, result_ref, result_ref),
    )
    cur.execute("update tasks set result_ref=%s where id=%s", (result_ref, t))
    return t


def _contract(checks):
    import json

    return json.dumps(
        {"kind": "t", "checks": checks, "budget": {"attempts": 3, "max_rework": 3}}
    )

def _submit(cur, t, checks_json, verdict="approved"):
    cur.execute(
        "select submit_review(%s,%s,'reviewer',%s,%s::jsonb)",
        (t, "r1", verdict, checks_json),
    )


# ---------------- F2: gate должен считать ПО КЛЮЧУ, а не по type ----------------


def test_f2_two_checks_same_type_need_two_passes(conn):
    """Два cmd-требования: один generic pass обязан быть отклонён (RV003)."""
    cur = conn.cursor()
    t = _mk_task_in_review(
        cur,
        [
            {"type": "cmd", "run": "cargo test"},
            {"type": "cmd", "run": "cargo clippy -- -D warnings"},
        ],
    )
    with pytest.raises(psycopg2.DatabaseError) as e:
        _submit(cur, t, '[{"type":"cmd","status":"pass"}]')
    assert "checks_failed" in str(e.value), "F2: один pass не должен закрывать два cmd"
    conn.rollback()


def test_f2_two_distinct_ids_need_two_passes(conn):
    """checks с id: для каждого id нужен свой pass."""
    cur = conn.cursor()
    t = _mk_task_in_review(
        cur,
        [
            {"type": "cmd", "id": "unit", "run": "cargo test"},
            {"type": "cmd", "id": "lint", "run": "cargo clippy"},
        ],
    )
    with pytest.raises(psycopg2.DatabaseError) as e:
        _submit(cur, t, '[{"type":"cmd","id":"unit","status":"pass"}]')
    assert "checks_failed" in str(e.value), "F2: id 'lint' остался без pass"
    conn.rollback()


def test_f2_matching_passes_accepted(conn):
    """Позитивный: два pass с нужными ключами → done."""
    cur = conn.cursor()
    t = _mk_task_in_review(
        cur,
        [
            {"type": "cmd", "id": "unit", "run": "cargo test"},
            {"type": "cmd", "id": "lint", "run": "cargo clippy"},
        ],
    )
    _submit(
        cur,
        t,
        '[{"type":"cmd","id":"unit","status":"pass"},{"type":"cmd","id":"lint","status":"pass"}]',
    )
    cur.execute("select state from tasks where id=%s", (t,))
    assert cur.fetchone()[0] == "done"
    conn.rollback()


# ---------------- F15: форма контракта ----------------


@pytest.mark.parametrize(
    "bad_checks",
    [
        ["pytest", 42],
        [{"run": "cargo test"}],
        [{"type": ""}],
        [{"type": 123}],
    ],
)
def test_f15_contract_rejects_malformed_checks(conn, bad_checks):
    """validate_contract не должен принимать checks без непустого строкового type."""
    cur = conn.cursor()
    with pytest.raises(psycopg2.DatabaseError) as e:
        cur.execute("select validate_contract(%s::jsonb)", (_contract(bad_checks),))
    assert "contract_invalid" in str(e.value)
    conn.rollback()
