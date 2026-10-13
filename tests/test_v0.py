"""Тесты инвариантов schema_v0.sql на реальном Postgres 16.

Запуск: PGDSN="dbname=v0test user=postgres host=localhost password=pw" pytest -q test_v0.py
Схема должна быть уже загружена в БД. Каждый тест начинает с пустых таблиц.
"""
import json
import os
import threading
import uuid

import psycopg2
import psycopg2.errors
import pytest
from psycopg2 import errors

DSN = os.environ.get("PGDSN", "dbname=v0test user=postgres host=localhost password=pw")


def conn():
    c = psycopg2.connect(DSN)
    c.autocommit = False
    return c


def code(exc):
    return exc.value.pgcode


@pytest.fixture
def db():
    c = conn()
    cur = c.cursor()
    cur.execute("truncate events, reviews, task_deps, artifacts, tasks, workers cascade")
    c.commit()
    yield c
    c.rollback()
    c.close()


def one(c, sql, *args):
    cur = c.cursor()
    cur.execute(sql, args)
    r = cur.fetchone()
    return r[0] if r and len(r) == 1 else r


def rows(c, sql, *args):
    cur = c.cursor()
    cur.execute(sql, args)
    return cur.fetchall()


def contract(needs=None, budget=None, checks=None):
    return json.dumps({
        "kind": "t",
        "needs": needs if needs is not None else {},
        "budget": budget if budget is not None else {"attempts": 3},
        "checks": checks if checks is not None else [{"type": "cmd", "run": "true"}],
    })


def new_task(c, root=None, ctr=None, prio=0, state=None, cap=1000):
    tid = str(uuid.uuid4())
    cur = c.cursor()
    cur.execute("select add_task(%s, %s, %s::jsonb, %s, %s)",
                (tid, root, ctr or contract(), cap, prio))
    if state:
        cur.execute("update tasks set state=%s where id=%s", (state, tid))
    c.commit()
    return tid


def worker(c, wid="w1", ram=16, coder=2, tags=()):
    cur = c.cursor()
    cur.execute("select register_worker(%s, %s::jsonb, %s)",
                (wid, json.dumps({"ram_gb": ram, "models": {"coder_class": coder}}), list(tags)))
    c.commit()


def ready(c, *ids):
    """pending -> ready через планировщик."""
    one(c, "select promote_ready()")
    c.commit()
    for i in ids:
        assert one(c, "select state from tasks where id=%s", i) == "ready"


def claim(c, wid="w1"):
    r = rows(c, "select * from claim_task(%s)", wid)
    c.commit()
    return r[0] if r else None


def expire(c, tid):
    cur = c.cursor()
    cur.execute("update tasks set lease_until = now() - interval '1 second' where id=%s", (tid,))
    c.commit()


# ---------------- DDL-инварианты ----------------
def test_state_check_rejects_garbage(db):
    t = new_task(db)
    with pytest.raises(errors.CheckViolation):
        db.cursor().execute("update tasks set state='banana' where id=%s", (t,))


def test_lease_invariant(db):
    t = new_task(db)
    with pytest.raises(errors.CheckViolation):
        db.cursor().execute("update tasks set state='leased' where id=%s", (t,))


def test_contract_requires_checks_and_numeric_needs(db):
    for bad in [{"checks": []}, {"kind": "x"},
                {"checks": [{"a": 1}], "needs": {"min_ram_gb": "lots"}},
                {"checks": [{"a": 1}], "budget": {"attempts": "x"}}]:
        with pytest.raises(psycopg2.Error) as e:
            db.cursor().execute("select add_task(%s,null,%s::jsonb,100,0)",
                                (str(uuid.uuid4()), json.dumps(bad)))
        assert code(e) == "CT001"
        db.rollback()


def test_worker_validation(db):
    with pytest.raises(psycopg2.Error) as e:
        db.cursor().execute("select register_worker('x', '{\"ram_gb\":\"abc\"}'::jsonb)")
    assert code(e) == "WK001"


# ---------------- claim ----------------
def test_claim_empty_needs_is_claimable(db):
    """Регрессия: задача без needs не должна отфильтровываться NULL-ом."""
    worker(db)
    t = new_task(db, ctr=contract(needs={}))
    ready(db, t)
    r = claim(db)
    assert r and str(r[0]) == t and r[2] == 1


def test_claim_hard_filters(db):
    worker(db, ram=16, coder=2)
    big = new_task(db, ctr=contract(needs={"min_ram_gb": 64}))
    smart = new_task(db, ctr=contract(needs={"coder_class": 3}))
    ok = new_task(db, ctr=contract(needs={"min_ram_gb": 8, "coder_class": 1}))
    ready(db, big, smart, ok)
    assert str(claim(db)[0]) == ok
    assert claim(db) is None
    assert one(db, "select state from tasks where id=%s", big) == "ready"


def test_claim_affinity_then_priority(db):
    worker(db, tags=["local"])
    plain = new_task(db, ctr=contract(needs={}), prio=9)
    tagged = new_task(db, ctr=contract(needs={"affinity_tags": ["local"]}), prio=0)
    ready(db, plain, tagged)
    assert str(claim(db)[0]) == tagged     # affinity сильнее priority (осознанное решение)
    assert str(claim(db)[0]) == plain


def test_claim_unknown_worker(db):
    with pytest.raises(psycopg2.Error) as e:
        db.cursor().execute("select * from claim_task('nobody')")
    assert code(e) == "WK002"


def test_claim_writes_event_atomically(db):
    worker(db)
    t = new_task(db)
    ready(db, t)
    claim(db)
    kinds = [r[0] for r in rows(db, "select kind from events where task_id=%s order by seq", t)]
    assert kinds == ["created", "promoted", "claimed"]
    assert one(db, "select verify_chain(%s)", t) is True


def test_concurrent_claims_never_double_assign(db):
    N, W = 24, 8
    for i in range(W):
        worker(db, f"w{i}")
    ids = [new_task(db) for _ in range(N)]
    ready(db, *ids)
    got, lock = [], threading.Lock()

    def run(wid):
        c = conn()
        while True:
            r = rows(c, "select * from claim_task(%s)", wid)
            c.commit()
            if not r:
                break
            with lock:
                got.append(str(r[0][0]))
        c.close()

    ts = [threading.Thread(target=run, args=(f"w{i}",)) for i in range(W)]
    [t.start() for t in ts]
    [t.join() for t in ts]
    assert len(got) == N and len(set(got)) == N


# ---------------- аренды и fencing ----------------
def test_lease_durations_must_be_positive(db):
    worker(db)
    t = new_task(db)
    ready(db, t)
    with pytest.raises(psycopg2.Error) as e:
        db.cursor().execute("select claim_task('w1', 0)")
    assert code(e) == "CT001"
    db.rollback()
    r = claim(db)
    with pytest.raises(psycopg2.Error) as e:
        db.cursor().execute("select heartbeat(%s,'w1',%s,0)", (t, r[2]))
    assert code(e) == "CT001"
    db.rollback()


def test_heartbeat_extends_and_cannot_revive(db):
    worker(db)
    t = new_task(db)
    ready(db, t)
    r = claim(db)
    before = one(db, "select lease_until from tasks where id=%s", t)
    assert one(db, "select heartbeat(%s,'w1',%s,600)", t, r[2]) is True
    db.commit()
    assert one(db, "select lease_until from tasks where id=%s", t) > before
    expire(db, t)
    assert one(db, "select heartbeat(%s,'w1',%s,600)", t, r[2]) is False   # зомби не оживает


def test_zombie_is_fenced_after_reclaim(db):
    worker(db, "w1")
    worker(db, "w2")
    t = new_task(db)
    ready(db, t)
    r1 = claim(db, "w1")
    expire(db, t)
    assert one(db, "select reclaim_expired()") == 1
    db.commit()
    assert one(db, "select state from tasks where id=%s", t) == "ready"
    with pytest.raises(psycopg2.Error) as e:
        db.cursor().execute("select save_artifact(%s,'w1',%s,'sha256:aa','/p')", (t, r1[2]))
    assert code(e) == "LS001"
    db.rollback()
    r2 = claim(db, "w2")
    assert r2[2] > r1[2]
    one(db, "select save_artifact(%s,'w2',%s,'sha256:bb','/p')", t, r2[2])
    assert one(db, "select complete_task(%s,'w2',%s,'sha256:bb',10)", t, r2[2]) == "review"
    db.commit()
    with pytest.raises(psycopg2.Error) as e:                       # старый epoch после всего
        db.cursor().execute("select complete_task(%s,'w1',%s,'sha256:aa',10)", (t, r1[2]))
    assert code(e) == "LS001"


def test_same_worker_old_epoch_is_fenced(db):
    """Единственный случай, где решает именно epoch: владелец и состояние совпадают,
    отличается только поколение аренды (воркер перезапустился и взял задачу снова)."""
    worker(db, "w1")
    t = new_task(db)
    ready(db, t)
    old = claim(db, "w1")
    expire(db, t)
    one(db, "select reclaim_expired()")
    db.commit()
    new = claim(db, "w1")
    assert new[2] > old[2]
    for sql in ["select save_artifact(%s,'w1',%s,'sha256:old','/p')",
                "select complete_task(%s,'w1',%s,'sha256:old',1)"]:
        with pytest.raises(psycopg2.Error) as e:
            db.cursor().execute(sql, (t, old[2]))
        assert code(e) == "LS001"
        db.rollback()
    assert one(db, "select heartbeat(%s,'w1',%s,60)", t, old[2]) is False
    assert one(db, "select heartbeat(%s,'w1',%s,60)", t, new[2]) is True


def test_complete_requires_existing_artifact(db):
    worker(db)
    t = new_task(db)
    ready(db, t)
    r = claim(db)
    with pytest.raises(psycopg2.Error) as e:
        db.cursor().execute("select complete_task(%s,'w1',%s,'sha256:none',1)", (t, r[2]))
    assert code(e) == "AR001"


def test_attempts_only_grow_on_reclaim_and_cap_blocks(db):
    worker(db)
    t = new_task(db, ctr=contract(budget={"attempts": 2}))
    ready(db, t)
    claim(db)
    assert one(db, "select attempts from tasks where id=%s", t) == 0   # claim не считает попытки
    expire(db, t)
    one(db, "select reclaim_expired()")
    db.commit()
    assert one(db, "select state, attempts from tasks where id=%s", t) == ("ready", 1)
    claim(db)
    expire(db, t)
    one(db, "select reclaim_expired()")
    db.commit()
    st, reason = rows(db, "select state, block_reason from tasks where id=%s", t)[0]
    assert (st, reason) == ("blocked", "max_attempts")


def test_budget_exhaustion_blocks_on_complete(db):
    worker(db)
    t = new_task(db, cap=100)
    ready(db, t)
    r = claim(db)
    one(db, "select save_artifact(%s,'w1',%s,'sha256:a','/p')", t, r[2])
    # Issue #20: complete_task при перерасходе даёт канонический 'budget_exhausted'
    # (Issue #3), а не 'blocked'. Ранее ожидание 'blocked' скрывало, что
    # roles_2b.sql перекрывал каноническую версию из schema_v0.sql.
    assert one(db, "select complete_task(%s,'w1',%s,'sha256:a',150)", t, r[2]) == "budget_exhausted"


def test_complete_rejects_negative_tokens(db):
    """P0-5: negative tokens must not refund/erase spent budget."""
    worker(db)
    t = new_task(db, cap=100)
    ready(db, t)
    r = claim(db)
    one(db, "select save_artifact(%s,'w1',%s,'sha256:a','/p')", t, r[2])
    with pytest.raises(psycopg2.Error) as e:
        db.cursor().execute("select complete_task(%s,'w1',%s,'sha256:a',-100)", (t, r[2]))
    assert code(e) == "CT001"
    db.rollback()
    # state and spent_tokens untouched
    st, spent = rows(db, "select state, spent_tokens from tasks where id=%s", t)[0]
    assert (st, spent) == ("leased", 0)


# ---------------- ревью ----------------
def to_review(db, t, wid="w1"):
    r = claim(db, wid)
    one(db, "select save_artifact(%s,%s,%s,'sha256:a','/p')", t, wid, r[2])
    one(db, "select complete_task(%s,%s,%s,'sha256:a',1)", t, wid, r[2])
    db.commit()


PASS = json.dumps([{"type": "cmd", "run": "true", "status": "pass", "exit_code": 0}])
FAIL = json.dumps([{"type": "cmd", "run": "true", "status": "fail", "exit_code": 1}])


def test_review_rules(db):
    worker(db)
    t = new_task(db)
    ready(db, t)
    to_review(db, t)
    for args, exp in [(("sha256:zzz", "rev", "approved", PASS), "RV001"),   # устаревший хэш
                      (("sha256:a", "w1", "approved", PASS), "RV002"),       # автор = рецензент
                      (("sha256:a", "rev", "approved", FAIL), "RV003")]:     # гейты не прошли
        with pytest.raises(psycopg2.Error) as e:
            db.cursor().execute("select submit_review(%s,%s,%s,%s,%s::jsonb)", (t, *args))
        assert code(e) == exp
        db.rollback()
    assert one(db, "select submit_review(%s,'sha256:a','rev','approved',%s::jsonb)", t, PASS) == "done"


def test_rework_does_not_touch_attempts_and_caps(db):
    worker(db)
    t = new_task(db, ctr=contract(budget={"attempts": 3, "max_rework": 1}))
    ready(db, t)
    to_review(db, t)
    assert one(db, "select submit_review(%s,'sha256:a','rev','changes_requested',%s::jsonb)", t, FAIL) == "ready"
    db.commit()
    assert rows(db, "select attempts, rework from tasks where id=%s", t)[0] == (0, 1)
    claim(db)                                        # повторная выдача не меняет attempts
    assert one(db, "select attempts from tasks where id=%s", t) == 0
    r = rows(db, "select lease_epoch from tasks where id=%s", t)[0][0]
    one(db, "select save_artifact(%s,'w1',%s,'sha256:b','/p')", t, r)
    one(db, "select complete_task(%s,'w1',%s,'sha256:b',1)", t, r)
    db.commit()
    assert one(db, "select submit_review(%s,'sha256:b','rev','changes_requested',%s::jsonb)", t, FAIL) == "blocked"


def test_done_promotes_dependents_and_promote_is_idempotent(db):
    worker(db)
    a = new_task(db)
    b = new_task(db, root=a)
    one(db, "select add_dep(%s,%s)", b, a)
    db.commit()
    ready(db, a)
    assert one(db, "select state from tasks where id=%s", b) == "pending"
    to_review(db, a)
    one(db, "select submit_review(%s,'sha256:a','rev','approved',%s::jsonb)", a, PASS)
    db.commit()
    assert one(db, "select state from tasks where id=%s", b) == "ready"
    assert one(db, "select promote_ready()") == 0
    assert rows(db, "select count(*) from events where task_id=%s and kind='promoted'", b)[0][0] == 1


# ---------------- граф ----------------
def chain(db, n, **budget):
    root = new_task(db, ctr=contract(budget={"attempts": 3, **budget}))
    ids = [root]
    for _ in range(n - 1):
        t = new_task(db, root=root)
        one(db, "select add_dep(%s,%s)", t, ids[-1])
        db.commit()
        ids.append(t)
    return ids


def test_cycle_rejected(db):
    ids = chain(db, 4)
    for a, b in [(ids[0], ids[3]), (ids[0], ids[0])]:
        with pytest.raises(psycopg2.Error) as e:
            db.cursor().execute("select add_dep(%s,%s)", (a, b))
        assert code(e) == "GR001"
        db.rollback()


def test_concurrent_opposite_edges_exactly_one_wins(db):
    root = new_task(db)
    a, b = new_task(db, root=root), new_task(db, root=root)
    barrier = threading.Barrier(2)
    res = []

    def run(x, y):
        c = conn()
        barrier.wait()
        try:
            one(c, "select add_dep(%s,%s)", x, y)
            c.commit()
            res.append("ok")
        except psycopg2.Error as e:
            c.rollback()
            res.append(e.pgcode)
        c.close()

    ts = [threading.Thread(target=run, args=(a, b)), threading.Thread(target=run, args=(b, a))]
    [t.start() for t in ts]
    [t.join() for t in ts]
    assert sorted(res) == ["GR001", "ok"]


def test_depth_fanout_and_task_limits(db):
    ids = chain(db, 3, max_depth=2)                  # 2 ребра: ок
    t = new_task(db, root=ids[0])
    with pytest.raises(psycopg2.Error) as e:
        db.cursor().execute("select add_dep(%s,%s)", (t, ids[2]))   # третье ребро
    assert code(e) == "GR002"
    db.rollback()

    root = new_task(db, ctr=contract(budget={"attempts": 3, "max_children": 2}))
    kids = [new_task(db, root=root) for _ in range(3)]
    one(db, "select add_dep(%s,%s)", kids[0], root)
    one(db, "select add_dep(%s,%s)", kids[1], root)
    db.commit()
    with pytest.raises(psycopg2.Error) as e:
        db.cursor().execute("select add_dep(%s,%s)", (kids[2], root))
    assert code(e) == "GR003"
    db.rollback()

    root = new_task(db, ctr=contract(budget={"attempts": 3, "max_tasks": 3}))
    new_task(db, root=root)
    new_task(db, root=root)
    with pytest.raises(psycopg2.Error) as e:
        db.cursor().execute("select add_task(%s,%s,%s::jsonb,10,0)", (str(uuid.uuid4()), root, contract()))
    assert code(e) == "GR004"


def test_cascade_blocks_subtree_and_fences_workers(db):
    worker(db)
    r, a, b = chain(db, 3)
    x = new_task(db, root=r)                          # независимая ветка
    cur = db.cursor()
    cur.execute("update tasks set state='done' where id in (%s,%s,%s)", (r, a, x))
    cur.execute("update tasks set state='leased', lease_owner='w1', lease_epoch=5, "
                "lease_until=now()+interval '1 hour' where id=%s", (b,))
    db.commit()
    assert one(db, "select reopen_task(%s,'op')", r) == 2
    db.commit()
    st = dict(rows(db, "select id::text, state from tasks"))
    assert (st[r], st[a], st[b], st[x]) == ("ready", "blocked", "blocked", "done")
    with pytest.raises(psycopg2.Error) as e:          # живой воркер ветки B отсечён
        db.cursor().execute("select save_artifact(%s,'w1',5,'sha256:q','/p')", (b,))
    assert code(e) == "LS001"
    db.rollback()
    assert one(db, "select unblock_task(%s,'op')", a) == "pending"


# ---------------- журнал ----------------
def test_event_idempotency(db):
    t = new_task(db)
    assert one(db, "select append_event(%s,'e1','claimed','x')", t) is True
    assert one(db, "select append_event(%s,'e1','claimed','x')", t) is False
    db.commit()
    assert rows(db, "select count(*) from events where task_id=%s and event_id='e1'", t)[0][0] == 1


def test_chain_has_no_fork_under_concurrency_even_for_first_event(db):
    tid = str(uuid.uuid4())
    cur = db.cursor()                                 # задача без единого события
    cur.execute("insert into tasks(id,root_id,contract,budget_cap) values(%s,%s,%s::jsonb,10)",
                (tid, tid, contract()))
    db.commit()
    barrier = threading.Barrier(8)

    def run(i):
        c = conn()
        barrier.wait()
        for k in range(5):
            one(c, "select append_event(%s,%s,'claimed','w')", tid, f"e{i}-{k}")
            c.commit()
        c.close()

    ts = [threading.Thread(target=run, args=(i,)) for i in range(8)]
    [t.start() for t in ts]
    [t.join() for t in ts]
    assert rows(db, "select count(*) from events where task_id=%s", tid)[0][0] == 40
    assert one(db, "select verify_chain(%s)", tid) is True


def test_tampering_breaks_chain(db):
    t = new_task(db)
    one(db, "select append_event(%s,'e1','claimed','x')", t)
    db.commit()
    assert one(db, "select verify_chain(%s)", t) is True
    db.cursor().execute("update events set payload='{\"x\":1}' where task_id=%s and event_id='e1'", (t,))
    db.commit()
    assert one(db, "select verify_chain(%s)", t) is False


def test_chain_detects_deleted_middle_event(db):
    """Волна 5.2: изоляция слоя «связь prev_hash» детектора цепочки.

    Существующий test_tampering_breaks_chain правит payload — его ловит второй
    слой (пересчёт hash). Слой «prev_hash следующего события указывает на
    предыдущее» не проверялся: SQL-мутация S5 (снять проверку связи) его
    переживала. Удаление СРЕДНЕГО события рвёт именно связь: хеши уцелевших
    событий остаются верными, но prev_hash третьего указывает на удалённое.
    """
    t = new_task(db)
    for eid in ("e1", "e2", "e3"):
        one(db, "select append_event(%s,%s,'claimed','x')", t, eid)
    db.commit()
    assert one(db, "select verify_chain(%s)", t) is True
    # Удаляем среднее событие: собственная hash каждого уцелевшего верна,
    # но связь e3.prev_hash -> e2 (удалён) нарушена.
    db.cursor().execute("delete from events where task_id=%s and event_id='e2'", (t,))
    db.commit()
    assert one(db, "select verify_chain(%s)", t) is False, (
        "удаление среднего события обязано ломать цепочку (слой prev_hash)"
    )


def test_chain_detects_corrupted_prev_hash(db):
    """Волна 5.2: изоляция слоя «e.prev_hash vs ожидаемый prev».

    Слой пересчёта hash ловит изменение payload/hash/удаление/порядок. Слой
    сравнения `e.prev_hash` с ожидаемым предыдущим хешем ловит СВОЙ класс:
    подмену только поля prev_hash при сохранённой (корректной) event_hash.
    SQL-мутация S5 (снять именно эту проверку) выживала без такого кейса. """
    t = new_task(db)
    for eid in ("e1", "e2"):
        one(db, "select append_event(%s,%s,'claimed','x')", t, eid)
    db.commit()
    assert one(db, "select verify_chain(%s)", t) is True
    # Портим ТОЛЬКО prev_hash второго события. Его собственная hash верна и
    # воспроизводится слоем 2, поэтому детектирует именно слой связи.
    db.cursor().execute(
        "update events set prev_hash='\\x00' where task_id=%s and event_id='e2'", (t,)
    )
    db.commit()
    assert one(db, "select verify_chain(%s)", t) is False, (
        "подмена prev_hash обязана ломать цепочку (слой связи)"
    )
