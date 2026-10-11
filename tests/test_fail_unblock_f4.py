"""F4 — честный отказ воркера (fail_task) + восстановимый unblock.

Дефект (pre-fix):
  * На HEAD нет функции fail_task. Отказ LLM оформлялся через complete_task()
    с подделанным overspend (worker.rs: `(cap - spent).max(0) + 1`), чтобы SQL
    сам перевёл задачу в blocked. Токены в журнале врут, поведение зависит от
    знака '>' в complete_task.
  * unblock_task(uuid,text) не знал про бюджет: исчерпанную задачу можно было
    «разблокировать», и она тут же снова уходила в blocked. Плейсхолдера для
    поднятия cap не было.

Фикс (reference_fix_wave4.sql, блок F4):
  * fail_task(p_task,p_worker,p_epoch,p_result_ref,p_reason) -> 'blocked';
    assert_lease + проверка артефакта (AR001); block_reason='llm_error: ...';
    событие completed{failed:true}.
  * unblock_task(uuid,text,bigint default null): blocked-only (GR009);
    p_new_budget_cap должен быть > spent_tokens (CT001), иначе при
    исчерпанном бюджете отказ (CT001).

Гейт BLOCKED (нет PG) — в tests/conftest.py.
Запуск:
  W4_ADMIN_DSN="host=... user=postgres password=*** dbname=c2_swarm" \
  W4_W1_PW=... W4_W2_PW=... \
  .venv/bin/python -m pytest -q tests/test_fail_unblock_f4.py
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


def _mk_leased_task(cur, worker="w1", spent=0, cap=1000, state="leased", epoch=0):
    """Создаёт задачу. Для state='leased' сразу проставляет владельца и срок
    аренды (CHECK tasks_check этого требует)."""
    t = str(uuid.uuid4())
    owner = worker if state == "leased" else None
    cur.execute(
        """insert into tasks(id, root_id, state, contract, priority, attempts,
                             budget_cap, spent_tokens, lease_epoch,
                             lease_owner, lease_until)
           values (%s,%s,%s,%s::jsonb,0,1,%s,%s,%s,%s,
                   case when %s is null then null else now()+interval '5 min' end)""",
        (t, t, state, json.dumps({"kind": "k", "checks": [{"type": "cmd"}]}),
         cap, spent, epoch, owner, owner),
    )
    return t


# ---------- fail_task ----------

def test_f4_fail_task_exists_and_blocks(conn):
    """fail_task присутствует и честно переводит leased-задачу в blocked."""
    cur = conn.cursor()
    cur.execute(
        "select p.proname from pg_proc p where p.proname = 'fail_task'"
    )
    assert cur.fetchone() is not None, "F4: fail_task отсутствует (pre-fix)"

    t = _mk_leased_task(cur, worker="w1", spent=10, cap=1000, epoch=1)
    cur.execute("select fail_task(%s,'w1',1,null,'boom')", (t,))
    v = cur.fetchone()[0]
    assert v == "blocked", f"F4: fail_task должен вернуть blocked, получено {v}"

    cur.execute("select state, block_reason, spent_tokens from tasks where id=%s", (t,))
    state, reason, spent = cur.fetchone()
    assert state == "blocked"
    assert reason == "llm_error: boom", f"F4: block_reason={reason!r}"
    # spent_tokens НЕ подделывается (честность журнала)
    assert spent == 10, f"F4: spent_tokens изменён ({spent}) — воркер врёт про бюджет"
    conn.rollback()


def test_f4_fail_task_requires_holdable_lease(conn):
    """fail_task без живой аренды -> LS001 (нельзя закрыть чужую задачу)."""
    cur = conn.cursor()
    t = _mk_leased_task(cur, worker="w1", state="leased", epoch=1)
    # нет lease_owner / не этот epoch
    with pytest.raises(psycopg2.DatabaseError) as e:
        cur.execute("select fail_task(%s,'w1',99,null,'x')", (t,))
    assert "LS001" in str(e.value) or "lease_lost" in str(e.value).lower()
    conn.rollback()


def test_f4_fail_task_artifact_missing(conn):
    """fail_task с несуществующим result_ref -> AR001."""
    cur = conn.cursor()
    t = _mk_leased_task(cur, worker="w1", epoch=1)
    with pytest.raises(psycopg2.DatabaseError) as e:
        cur.execute("select fail_task(%s,'w1',1,'no-such-ref','x')", (t,))
    assert "AR001" in str(e.value) or "artifact_missing" in str(e.value).lower()
    conn.rollback()


# ---------- unblock_task + budget ----------

def test_f4_unblock_exhausted_without_new_cap_rejected(conn):
    """blocked с spent >= cap и без p_new_budget_cap -> CT001."""
    cur = conn.cursor()
    t = _mk_leased_task(cur, spent=1000, cap=1000, state="blocked")
    with pytest.raises(psycopg2.DatabaseError) as e:
        cur.execute("select unblock_task(%s,'operator')", (t,))
    assert "CT001" in str(e.value) or "budget" in str(e.value).lower()
    conn.rollback()


def test_f4_unblock_with_new_cap_raises_budget(conn):
    """blocked с исчерпанным бюджетом + p_new_budget_cap > spent -> pending."""
    cur = conn.cursor()
    t = _mk_leased_task(cur, spent=1000, cap=1000, state="blocked")
    cur.execute("select unblock_task(%s,'operator',2000)", (t,))
    assert cur.fetchone()[0] in ("pending", "ready")
    cur.execute("select state, budget_cap, attempts, rework from tasks where id=%s", (t,))
    state, cap, attempts, rework = cur.fetchone()
    # promote_ready() в конце unblock_task может сразу поднять pending -> ready
    assert state in ("pending", "ready"), f"F4: state={state!r}"
    assert cap == 2000, f"F4: budget_cap не поднят ({cap})"
    assert attempts == 0 and rework == 0
    conn.rollback()


def test_f4_unblock_new_cap_not_above_spent_rejected(conn):
    """p_new_budget_cap <= spent_tokens -> CT001 (бессмысленно)."""
    cur = conn.cursor()
    t = _mk_leased_task(cur, spent=1000, cap=1000, state="blocked")
    with pytest.raises(psycopg2.DatabaseError) as e:
        cur.execute("select unblock_task(%s,'operator',500)", (t,))
    assert "CT001" in str(e.value) or "budget" in str(e.value).lower()
    conn.rollback()


def test_f4_unblock_non_blocked_rejected(conn):
    """unblock_task не для blocked -> GR009."""
    cur = conn.cursor()
    t = _mk_leased_task(cur, state="pending")
    with pytest.raises(psycopg2.DatabaseError) as e:
        cur.execute("select unblock_task(%s,'operator',2000)", (t,))
    assert "GR009" in str(e.value) or "not_blocked" in str(e.value).lower()
    conn.rollback()


def test_f4_unblock_ok_when_budget_fine(conn):
    """blocked, но бюджет не исчерпан: unblock без явного cap работает."""
    cur = conn.cursor()
    t = _mk_leased_task(cur, spent=10, cap=1000, state="blocked")
    cur.execute("select unblock_task(%s,'operator')", (t,))
    assert cur.fetchone()[0] in ("pending", "ready")
    conn.rollback()
