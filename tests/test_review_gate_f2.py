#!/usr/bin/env python3
"""Тест-арбитр волны 4 — F2: review-gate должен считать по КЛЮЧУ проверки.

Дефект: submit_review сопоставлял проверки по `type` через EXISTS. Контракт с
двумя cmd-требованиями (cargo test, cargo clippy) закрывался ОДНИМ generic
{"type":"cmd","status":"pass"} — gate обходился.

Фикс (эталон reference_fix_wave4.sql, блок F2):
  ключ = coalesce(check->>'id', check->>'type');
  для каждого ключа число pass в отчёте >= числа требований контракта с этим ключом.

ТРИАДА:
  RED  pre-fix : one generic cmd pass -> task становится done (баг) => падает
  GREEN fixed  : one generic cmd pass -> RV003; два pass -> done
  RED  after revert: снова done => падает

Гейт BLOCKED (нет PG) — в tests/conftest.py.
Запуск:
  W4_ADMIN_DSN="host=... user=postgres password=... dbname=c2_swarm" \
  .venv/bin/python -m pytest -q tests/test_review_gate_f2.py
"""
import json
import os
import uuid

import psycopg2
import pytest


def _dsn() -> str:
    return os.environ.get("W4_ADMIN_DSN", "")


@pytest.fixture()
def conn():
    c = psycopg2.connect(_dsn())
    c.autocommit = False
    yield c
    c.rollback()
    c.close()


def _contract(checks):
    return json.dumps(
        {"kind": "code.test", "checks": checks, "needs": {}, "budget": {"attempts": 3, "max_rework": 3}}
    )


def _mk_task_in_review(cur, checks, result_ref="r1"):
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


def _submit(cur, t, checks_json, verdict="approved"):
    cur.execute(
        "select submit_review(%s,%s,'reviewer',%s,%s::jsonb)",
        (t, "r1", verdict, checks_json),
    )
    return cur.fetchone()[0]


def _state(cur, t):
    cur.execute("select state from tasks where id=%s", (t,))
    return cur.fetchone()[0]


# ---------------- F2 core: два cmd одного типа требуют двух pass ----------------


def test_f2_two_checks_same_type_need_two_passes(conn):
    """Контракт: два cmd разных run, без id -> один generic cmd pass = RV003."""
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


def test_f2_two_ids_need_two_passes(conn):
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
    """Позитивный: два pass по ключам -> done (регрессия легитимного пути)."""
    cur = conn.cursor()
    t = _mk_task_in_review(
        cur,
        [
            {"type": "cmd", "id": "unit", "run": "cargo test"},
            {"type": "cmd", "id": "lint", "run": "cargo clippy"},
        ],
    )
    v = _submit(
        cur,
        t,
        '[{"type":"cmd","id":"unit","status":"pass"},{"type":"cmd","id":"lint","status":"pass"}]',
    )
    assert v == "done", f"F2: два корректных pass должны дать done, получено {v}"
    assert _state(cur, t) == "done"
    conn.rollback()


def test_f2_same_type_no_id_two_passes_accepted(conn):
    """Позитивный без id: два cmd pass с совпадающими run (ключ = type|run) -> done."""
    cur = conn.cursor()
    t = _mk_task_in_review(
        cur,
        [
            {"type": "cmd", "run": "cargo test"},
            {"type": "cmd", "run": "cargo clippy"},
        ],
    )
    v = _submit(
        cur,
        t,
        '[{"type":"cmd","run":"cargo test","status":"pass"},'
        '{"type":"cmd","run":"cargo clippy","status":"pass"}]',
    )
    assert v == "done", f"F2: два совпадающих pass закрывают два cmd, получено {v}"
    conn.rollback()


def test_f2_wrong_run_does_not_close_other_command(conn):
    """F2-ядро: pass с run='cargo test' НЕ закрывает требование run='cargo clippy'."""
    cur = conn.cursor()
    t = _mk_task_in_review(
        cur,
        [
            {"type": "cmd", "run": "cargo test"},
            {"type": "cmd", "run": "cargo clippy"},
        ],
    )
    with pytest.raises(psycopg2.DatabaseError) as e:
        # отчёт дублирует один и тот же run дважды — clippy всё равно не покрыт
        _submit(
            cur,
            t,
            '[{"type":"cmd","run":"cargo test","status":"pass"},'
            '{"type":"cmd","run":"cargo test","status":"pass"}]',
        )
    assert "checks_failed" in str(e.value), "F2: дубль run не должен закрывать clippy"
    conn.rollback()
