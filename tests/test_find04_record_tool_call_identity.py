#!/usr/bin/env python3
"""Тест-арбитр волны 4 — FIND-04: подделка аудита через record_tool_call.

Дефект: record_tool_call() вызывает только assert_lease(), но НЕ
assert_worker_identity(). Функция принимает p_worker параметром, который
контролирует вызывающий. Поэтому воркер w1 (session_user=swarm_worker_w1)
может записать событие tool_call для аренды w2, указав p_worker='w2' —
hash-chain зафиксирует фальшивое авторство w2.

Требование: `perform assert_worker_identity(p_worker);` первой строкой тела.

ТРИАДА (три кейса в одном файле):
  case1 (собственно триада): w1 -> record_tool_call(task_w2, p_worker='w2')
        ДО фикса: успех (подделка проходит)   -> КРАСНЫЙ
        ПОСЛЕ:    WK003 worker_identity_mismatch -> ЗЕЛЁНЫЙ
        ПОСЛЕ revert: снова успех              -> КРАСНЫЙ
  case2 (регресс легит w1): w1 -> record_tool_call(task_w1, p_worker='w1') -> успех
  case3 (регресс легит w2): w2 -> record_tool_call(task_w2, p_worker='w2') -> успех

Без case2/case3 фикс «отклонять всё подряд» пройдёт триаду и сломает воркер.

PRE-CONDITION: отсутствие DSN => BLOCKED (env-гейт, exit 2), НЕ skip.
Это применение урока FIND-08 к нашему собственному PG-тесту.

Запуск (против свежего initdb из HEAD, schema->roles->roles_2b->patches):
    W4_ADMIN_DSN="host=... port=... user=postgres password=... dbname=c2_swarm" \
    W4_W1_PW=... W4_W2_PW=... python3 -m pytest -q tests/test_find04_record_tool_call_identity.py
"""
import json
import os
import sys
import uuid

import psycopg2
import psycopg2.extensions
import pytest

BLOCKED_EXIT = 2


def _admin_dsn() -> str:
    return os.environ.get("W4_ADMIN_DSN", "")


def _role_dsn(worker: str, password: str) -> str:
    dsn = _admin_dsn()
    # swap user/password for the per-worker role
    parts = dict(
        kv.split("=", 1) for kv in dsn.split() if "=" in kv
    )
    host = parts.get("host", "127.0.0.1")
    port = parts.get("port", "5432")
    db = parts.get("dbname", "c2_swarm")
    return f"host={host} port={port} user=swarm_worker_{worker} password={password} dbname={db}"


def _env_missing():
    return [
        n for n in ("W4_ADMIN_DSN", "W4_W1_PW", "W4_W2_PW") if not os.environ.get(n)
    ]


BLOCKED_MSG = (
    "BLOCKED: PostgreSQL not provisioned; FIND-04 arbiter requires a live DB "
    "(set W4_ADMIN_DSN, W4_W1_PW, W4_W2_PW)"
)


def pytest_configure(config):
    # PRE-CONDITION FAIL = BLOCKED, never skip (lesson from FIND-08).
    if _env_missing():
        config.option.collectonly = False
        raise pytest.UsageError(BLOCKED_MSG)


@pytest.fixture()
def admin():
    # Second barrier: even if configure is bypassed, never silently proceed to DB-less run.
    if _env_missing():
        pytest.fail(BLOCKED_MSG, pytrace=False)
    c = psycopg2.connect(_admin_dsn())
    c.autocommit = True
    yield c
    c.close()


def _worker_conn(worker: str):
    pw = os.environ.get(f"W4_{worker.upper()}_PW")
    c = psycopg2.connect(_role_dsn(worker, pw))
    c.autocommit = True
    return c


def _contract():
    return json.dumps(
        {
            "kind": "code.test",
            "checks": [{"type": "cmd", "run": "true"}],
            "needs": {},
            "budget": {"attempts": 3, "max_rework": 2},
        }
    )


def _make_leased_task(admin, worker: str) -> tuple[str, int]:
    """Создаёт ready-задачу, регистрирует worker и захватывает её. Возвращает (task_id, epoch)."""
    cur = admin.cursor()
    task_id = str(uuid.uuid4())
    cur.execute(
        "select add_task(%s, null, %s::jsonb, 10000, 0);",
        (task_id, _contract()),
    )
    cur.execute("select promote_ready();")
    cur.execute(
        "select register_worker(%s, %s::jsonb);",
        (worker, json.dumps({"ram_gb": 16})),
    )
    cur.execute("select out_epoch from claim_task(%s, 60);", (worker,))
    epoch = cur.fetchone()[0]
    assert epoch is not None, f"claim_task({worker}) не выдал аренду"
    return task_id, int(epoch)


# ---------------- CASE 1: триада — спуфинг должен быть отклонён ----------------


def test_case1_w1_cannot_forge_event_as_w2(admin):
    """w1 пишет tool_call на аренде w2, указав p_worker='w2'.

    ДО фикса: вызов проходит, событие атрибутируется w2 -> assert падает (КРАСНЫЙ).
    ПОСЛЕ:    WK003 worker_identity_mismatch -> зелёный.
    """
    task_w2, epoch_w2 = _make_leased_task(admin, "w2")

    w1 = _worker_conn("w1")
    try:
        cur = w1.cursor()
        forged = json.dumps({"tool": "run_shell", "command": "spoofed_by_w1"})
        with pytest.raises(psycopg2.DatabaseError) as e:
            cur.execute(
                "select record_tool_call(%s,'w2',%s,'spoofed_evt_1',%s::jsonb);",
                (task_w2, epoch_w2, forged),
            )
        assert "WK003" in str(e.value) or "worker_identity_mismatch" in str(e.value), (
            f"CASE1: ожидался WK003 worker_identity_mismatch, получено: {e.value}"
        )
    finally:
        w1.close()

    # доп.: события с этим id быть не должно
    ac = admin.cursor()
    ac.execute(
        "select count(*) from events where task_id=%s and event_id='spoofed_evt_1';",
        (task_w2,),
    )
    assert ac.fetchone()[0] == 0, "CASE1: поддельное событие всё же попало в журнал"


# ---------------- CASE 2: регресс — легитимный вызов w1 на своей аренде ----------------


def test_case2_legit_w1_can_record_own_event(admin):
    """w1 пишет tool_call на СВОЕЙ аренде под своим именем — должно работать."""
    task_w1, epoch_w1 = _make_leased_task(admin, "w1")

    w1 = _worker_conn("w1")
    try:
        cur = w1.cursor()
        good = json.dumps({"tool": "run_shell", "command": "cargo test", "exit_code": 0})
        cur.execute(
            "select record_tool_call(%s,'w1',%s,'w1_evt_1',%s::jsonb);",
            (task_w1, epoch_w1, good),
        )
        assert cur.fetchone()[0] is True, "CASE2: легитимный вызов w1 должен вернуть true"
    finally:
        w1.close()

    ac = admin.cursor()
    ac.execute(
        "select actor from events where task_id=%s and event_id='w1_evt_1';",
        (task_w1,),
    )
    row = ac.fetchone()
    assert row is not None and row[0] == "w1", "CASE2: событие должно быть атрибутировано w1"


# ---------------- CASE 3: регресс — легитимный вызов w2 на своей аренде ----------------


def test_case3_legit_w2_can_record_own_event(admin):
    """w2 пишет tool_call на СВОЕЙ аренде под своим именем — должно работать."""
    task_w2, epoch_w2 = _make_leased_task(admin, "w2")

    w2 = _worker_conn("w2")
    try:
        cur = w2.cursor()
        good = json.dumps({"tool": "run_shell", "command": "cargo clippy", "exit_code": 0})
        cur.execute(
            "select record_tool_call(%s,'w2',%s,'w2_evt_1',%s::jsonb);",
            (task_w2, epoch_w2, good),
        )
        assert cur.fetchone()[0] is True, "CASE3: легитимный вызов w2 должен вернуть true"
    finally:
        w2.close()

    ac = admin.cursor()
    ac.execute(
        "select actor from events where task_id=%s and event_id='w2_evt_1';",
        (task_w2,),
    )
    row = ac.fetchone()
    assert row is not None and row[0] == "w2", "CASE3: событие должно быть атрибутировано w2"
