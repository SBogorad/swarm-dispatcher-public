"""Тесты разделения ролей (roles.sql) на реальном Postgres 16.

Запуск (БД с применёнными schema_v0.sql и roles.sql, пароли pw_<роль>):
  ROLES_DB=v0roles ROLES_HOST=localhost ADMIN_PW=pw pytest -q test_roles.py

Правило классификации: вызов «разрешён», если ошибка НЕ 42501 (insufficient_privilege).
Доменные ошибки функций (GR000, LS001 и т.п.) на пустых данных ожидаемы и значат,
что доступ есть. 42501 изнутри функции (нет прав на таблицу) тоже считается отказом.
"""
import json
import os
import uuid

import psycopg2
import pytest
from psycopg2 import errors

DB = os.environ.get("ROLES_DB", "v0roles")
HOST = os.environ.get("ROLES_HOST", "localhost")
PORT = int(os.environ.get("ROLES_PORT", "5432"))
ADMIN_PW = os.environ.get("ADMIN_PW", "pw")
ROLES = ["swarm_worker_w1", "swarm_watchdog", "swarm_planner",
         "swarm_reviewer", "swarm_operator", "swarm_cli"]
DENIED = "42501"


def connect(user, pw):
    c = psycopg2.connect(host=HOST, port=PORT, dbname=DB, user=user, password=pw)
    c.autocommit = True
    return c


def as_role(role):
    if role.startswith("swarm_worker_"):
        suffix = role.removeprefix("swarm_worker_")
    else:
        suffix = role.split("_", 1)[1]
    return connect(role, os.environ.get("SWARM_PW_" + suffix.upper(), "pw_" + suffix))


@pytest.fixture
def admin():
    c = connect("postgres", ADMIN_PW)
    c.cursor().execute("truncate events, reviews, task_deps, artifacts, tasks, workers cascade")
    yield c
    c.close()


def attempt(conn, sql):
    """None если выполнено; иначе SQLSTATE."""
    try:
        conn.cursor().execute(sql)
        return None
    except psycopg2.Error as e:
        return e.pgcode


U = "'00000000-0000-0000-0000-000000000001'"
CONTRACT = json.dumps({"kind": "t", "checks": [{"type": "cmd"}], "budget": {"attempts": 3}})

CALLS = {
    "claim_task":       "select * from claim_task('w1', 10)",
    "heartbeat":        f"select heartbeat({U}, 'w1', 1, 10)",
    "save_artifact":    f"select save_artifact({U}, 'w1', 1, 'sha256:a', '/p')",
    "complete_task":    f"select complete_task({U}, 'w1', 1, 'sha256:a', 1)",
    "register_worker":  "select register_worker('w-reg', '{\"ram_gb\":8}'::jsonb)",
    "reclaim_expired":  "select reclaim_expired()",
    "promote_ready":    "select promote_ready()",
    "add_task":         f"select add_task('{uuid.uuid4()}', null, '{CONTRACT}'::jsonb, 100, 0)",
    "add_dep":          f"select add_dep({U}, {U.replace('1', '2')})",
    "submit_review":    f"select submit_review({U}, 'sha256:a', 'rev', 'approved', '[]'::jsonb)",
    "verify_chain":     f"select verify_chain({U})",
    "reopen_task":      f"select reopen_task({U}, 'op')",
    "unblock_task":     f"select unblock_task({U}, 'op')",
    "task_status":      "select * from task_status(10)",
}
INTERNAL = {
    "append_event":      f"select append_event({U}, 'e', 'claimed', 'x')",
    "assert_lease":      f"select assert_lease({U}, 'w1', 1)",
    "lock_root":         f"select lock_root({U})",
    "validate_contract": f"select validate_contract('{CONTRACT}'::jsonb)",
    "event_hash":        f"select event_hash(null, {U}, 'e', 'claimed', 'x', '{{}}'::jsonb, now())",
    "c_num":             "select c_num('{}'::jsonb, '{a}', 0)",
}

ALLOWED = {
    "swarm_worker_w1": {"claim_task", "heartbeat", "save_artifact", "complete_task", "register_worker"},
    "swarm_watchdog": {"reclaim_expired", "promote_ready"},
    "swarm_planner":  {"add_task", "add_dep", "promote_ready", "task_status"},
    "swarm_reviewer": {"submit_review", "verify_chain"},
    "swarm_operator": {"reopen_task", "unblock_task", "verify_chain", "task_status"},
}
ALLOWED["swarm_cli"] = ALLOWED["swarm_planner"] | ALLOWED["swarm_reviewer"] | ALLOWED["swarm_operator"]


@pytest.mark.parametrize("role", ROLES)
def test_function_matrix(admin, role):
    """Каждая роль может ровно свои функции: ни больше, ни меньше."""
    conn = as_role(role)
    for fn, sql in CALLS.items():
        code = attempt(conn, sql)
        if fn in ALLOWED[role]:
            assert code != DENIED, f"{role} должна мочь {fn}, получила {code}"
        else:
            assert code == DENIED, f"{role} НЕ должна мочь {fn}, получила {code}"


@pytest.mark.parametrize("role", ROLES)
def test_internal_functions_closed_for_everyone(admin, role):
    conn = as_role(role)
    for fn, sql in INTERNAL.items():
        assert attempt(conn, sql) == DENIED, f"{role} вызвала внутреннюю {fn}"


@pytest.mark.parametrize("role", ROLES)
def test_no_direct_table_access(admin, role):
    conn = as_role(role)
    cols = {"tasks": "id", "events": "seq", "artifacts": "ref", "task_deps": "task_id",
            "workers": "id", "reviews": "id"}
    for t, col in cols.items():
        for sql in [f"select 1 from {t} limit 1", f"delete from {t}", f"truncate {t}",
                    f"update {t} set {col} = {col}"]:
            assert attempt(conn, sql) == DENIED, f"{role}: `{sql}` не должно выполняться"
    assert attempt(conn, "insert into events(task_id,event_id,kind,at,hash) "
                         f"values ({U},'x','claimed',now(),'\\x00')") == DENIED


@pytest.mark.parametrize("role", ROLES)
def test_no_ddl_in_public(admin, role):
    conn = as_role(role)
    assert attempt(conn, "create table public.x(i int)") == DENIED
    assert attempt(conn, "create function public.evil() returns int language sql as 'select 1'") == DENIED


def test_worker_cannot_approve_own_work(admin):
    """Главное следствие ролей: самоприёмка закрыта на уровне БД, а не только проверкой created_by."""
    assert attempt(as_role("swarm_worker_w1"),
                   f"select submit_review({U}, 'sha256:a', 'w1', 'approved', '[]'::jsonb)") == DENIED


def test_catalog_invariants(admin):
    cur = admin.cursor()
    cur.execute("""select p.proname, p.prosecdef, p.proconfig, r.rolsuper
                   from pg_proc p join pg_roles r on r.oid = p.proowner
                   where p.pronamespace = 'public'::regnamespace""")
    rows = cur.fetchall()
    assert len(rows) >= 19
    for name, secdef, cfg, is_super in rows:
        assert secdef, f"{name}: не SECURITY DEFINER"
        assert cfg and "search_path=public, pg_temp" in cfg, f"{name}: search_path не закреплён ({cfg})"
        assert not is_super, f"{name}: владелец суперпользователь"
    cur.execute("""select c.relname, r.rolsuper from pg_class c join pg_roles r on r.oid = c.relowner
                   where c.relnamespace = 'public'::regnamespace and c.relkind = 'r'""")
    for name, is_super in cur.fetchall():
        assert not is_super, f"таблица {name}: владелец суперпользователь"
    cur.execute("select rolname from pg_roles where rolname = any(%s) and (rolsuper or rolcreatedb or rolcreaterole)",
                (ROLES,))
    assert cur.fetchall() == []


def test_temp_table_cannot_hijack_security_definer(admin):
    """Временная таблица `tasks` у воркера не должна перекрыть настоящую (pg_temp стоит последним)."""
    cur = admin.cursor()
    cur.execute("select register_worker('w1', '{\"ram_gb\":16}'::jsonb)")
    t = str(uuid.uuid4())
    cur.execute("select add_task(%s, null, %s::jsonb, 100, 0)", (t, CONTRACT))
    cur.execute("select promote_ready()")
    w = as_role("swarm_worker_w1")
    wc = w.cursor()
    wc.execute("create temp table tasks (id uuid, state text)")
    wc.execute("create temp table workers (id text)")
    wc.execute("select out_task_id from claim_task('w1', 60)")
    assert [str(r[0]) for r in wc.fetchall()] == [t]


def test_full_workflow_uses_only_role_permissions(admin):
    """Реальный сценарий целиком, каждый шаг от своей роли, без суперпользователя."""
    pl, wk, rv, wd, op = (as_role(r) for r in
                          ["swarm_planner", "swarm_worker_w1", "swarm_reviewer", "swarm_watchdog", "swarm_operator"])
    root, child = str(uuid.uuid4()), str(uuid.uuid4())
    c = pl.cursor()
    c.execute("select add_task(%s, null, %s::jsonb, 1000, 0)", (root, CONTRACT))
    c.execute("select add_task(%s, %s, %s::jsonb, 1000, 0)", (child, root, CONTRACT))
    c.execute("select add_dep(%s, %s)", (child, root))
    c.execute("select promote_ready()")
    assert c.fetchone()[0] == 1

    c = wk.cursor()
    c.execute("select register_worker('w1', '{\"ram_gb\":16}'::jsonb)")
    c.execute("select out_task_id, out_epoch from claim_task('w1', 1)")
    tid, epoch = c.fetchone()
    assert str(tid) == root
    c.execute("select save_artifact(%s, 'w1', %s, 'sha256:a', '/p')", (root, epoch))
    admin.cursor().execute("update tasks set lease_until = now() - interval '1 second' where id = %s", (root,))
    # аренда истекла: воркер потерял право, сторож её возвращает
    assert attempt(wk, f"select complete_task('{root}', 'w1', {epoch}, 'sha256:a', 1)") == "LS001"
    c = wd.cursor()
    c.execute("select reclaim_expired()")
    assert c.fetchone()[0] == 1

    c = wk.cursor()
    c.execute("select out_task_id, out_epoch from claim_task('w1', 60)")
    tid, epoch = c.fetchone()
    c.execute("select save_artifact(%s, 'w1', %s, 'sha256:b', '/p')", (root, epoch))
    c.execute("select complete_task(%s, 'w1', %s, 'sha256:b', 1)", (root, epoch))
    assert c.fetchone()[0] == "review"

    c = rv.cursor()
    c.execute("select submit_review(%s, 'sha256:b', 'rev', 'approved', %s::jsonb)",
              (root, json.dumps([{"type": "cmd", "status": "pass"}])))
    assert c.fetchone()[0] == "done"
    c.execute("select verify_chain(%s)", (root,))
    assert c.fetchone()[0] is True

    c = op.cursor()
    c.execute("select reopen_task(%s, 'op')", (root,))
    assert c.fetchone()[0] == 1  # потомок после done корня стал ready и теперь заблокирован каскадом
    ac = admin.cursor()
    ac.execute("select state from tasks where id = %s", (child,))
    assert ac.fetchone()[0] == "blocked"


def test_cli_role_is_union_but_not_worker():
    conn = as_role("swarm_cli")
    assert attempt(conn, CALLS["claim_task"]) == DENIED
    assert attempt(conn, CALLS["heartbeat"]) == DENIED
    assert attempt(conn, CALLS["add_task"]) != DENIED
    assert attempt(conn, CALLS["submit_review"]) != DENIED


def test_task_status_shows_state_but_not_contract(admin):
    cur = admin.cursor()
    t = str(uuid.uuid4())
    cur.execute("select add_task(%s, null, %s::jsonb, 100, 0)", (t, CONTRACT))
    c = as_role("swarm_operator").cursor()
    c.execute("select * from task_status(5)")
    cols = [d[0] for d in c.description]
    rows = c.fetchall()
    assert len(rows) == 1 and str(rows[0][0]) == t
    assert "contract" not in cols and "result_ref" not in cols
    assert cols[:3] == ["id", "root_id", "state"]
