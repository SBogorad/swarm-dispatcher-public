"""Тесты идентификации воркера (roles_2b.sql). Полная версия.

Запуск (БД с schema_v0.sql + roles.sql + roles_2b.sql):
  ROLES_DB=swarm ROLES_HOST=127.0.0.1 ADMIN_PW=pw \
  SWARM_PW_W1=pw_w1 SWARM_PW_W2=pw_w2 \
  SWARM_PW_W_1=pw_w_1 SWARM_PW_GPU_1=pw_gpu_1 \
  pytest -q step2b/test_roles_2b.py

Правило: 42501 — недостаточно прав (роль); WK003 — несоответствие
session_user и p_worker. Другие доменные ошибки (GRxxx, LS001) —
доступ есть, дошли до логики функции.
"""
import json
import os
import uuid

import psycopg2
import pytest

DB = os.environ.get("ROLES_DB", "swarm")
HOST = os.environ.get("ROLES_HOST", "127.0.0.1")
PORT = int(os.environ.get("ROLES_PORT", "5432"))
ADMIN_PW = os.environ.get("ADMIN_PW", "pw")
DENIED = "42501"
MISMATCH = "WK003"
CONTRACT = json.dumps({"kind": "t", "checks": [{"type": "cmd"}], "budget": {"attempts": 3}})


def connect(user, pw):
    c = psycopg2.connect(host=HOST, port=PORT, dbname=DB, user=user, password=pw)
    c.autocommit = True
    return c


def as_role(role):
    if role.startswith("swarm_worker_"):
        suffix = role.removeprefix("swarm_worker_")
        env_name = "SWARM_PW_" + suffix.upper()
    else:
        suffix = role.split("_", 1)[1]
        env_name = "SWARM_PW_" + suffix.upper()
    return connect(role, os.environ.get(env_name, "pw_" + suffix))


def attempt(conn, sql):
    """None если выполнено; иначе SQLSTATE."""
    try:
        conn.cursor().execute(sql)
        return None
    except psycopg2.Error as e:
        return e.pgcode


@pytest.fixture
def admin():
    c = connect("postgres", ADMIN_PW)
    c.cursor().execute("truncate events, reviews, task_deps, artifacts, tasks, workers cascade")
    yield c
    c.close()


def _prepare_task_for(admin, worker_id):
    """Хелпер: регистрирует воркера, создаёт задачу, промоутит, клеймит."""
    admin.cursor().execute("select register_worker(%s, '{\"ram_gb\":16}'::jsonb)", (worker_id,))
    t = str(uuid.uuid4())
    admin.cursor().execute("select add_task(%s, null, %s::jsonb, 100, 0)", (t, CONTRACT))
    admin.cursor().execute("select promote_ready()")
    admin.cursor().execute("select claim_task(%s, 60)", (worker_id,))
    return t


# ---------- Позитивные: своя роль, своё имя ----------

def test_01_w1_register_self(admin):
    w = as_role("swarm_worker_w1")
    assert attempt(w, "select register_worker('w1', '{\"ram_gb\":16}'::jsonb)") is None


def test_02_w1_claim_self(admin):
    admin.cursor().execute("select register_worker('w1', '{\"ram_gb\":16}'::jsonb)")
    t = str(uuid.uuid4())
    admin.cursor().execute("select add_task(%s, null, %s::jsonb, 100, 0)", (t, CONTRACT))
    admin.cursor().execute("select promote_ready()")
    w = as_role("swarm_worker_w1")
    assert attempt(w, "select claim_task('w1', 60)") is None


def test_03_w1_heartbeat_self(admin):
    t = _prepare_task_for(admin, "w1")
    w = as_role("swarm_worker_w1")
    assert attempt(w, f"select heartbeat('{t}', 'w1', 1, 60)") is None


def test_04_w1_save_artifact_self(admin):
    t = _prepare_task_for(admin, "w1")
    w = as_role("swarm_worker_w1")
    assert attempt(w, f"select save_artifact('{t}', 'w1', 1, 'sha256:a', '/p')") is None


def test_05_w1_complete_self(admin):
    t = _prepare_task_for(admin, "w1")
    admin.cursor().execute(f"select save_artifact('{t}', 'w1', 1, 'sha256:a', '/p')")
    w = as_role("swarm_worker_w1")
    assert attempt(w, f"select complete_task('{t}', 'w1', 1, 'sha256:a', 1)") is None


# ---------- Негативные: чужая роль, чужое имя (WK003) ----------

def test_06_w1_cannot_register_w2(admin):
    w = as_role("swarm_worker_w1")
    assert attempt(w, "select register_worker('w2', '{\"ram_gb\":16}'::jsonb)") == MISMATCH


def test_07_w1_cannot_claim_w2(admin):
    admin.cursor().execute("select register_worker('w2', '{\"ram_gb\":16}'::jsonb)")
    t = str(uuid.uuid4())
    admin.cursor().execute("select add_task(%s, null, %s::jsonb, 100, 0)", (t, CONTRACT))
    admin.cursor().execute("select promote_ready()")
    w = as_role("swarm_worker_w1")
    assert attempt(w, "select claim_task('w2', 60)") == MISMATCH


def test_08_w1_cannot_heartbeat_w2(admin):
    t = _prepare_task_for(admin, "w2")
    w = as_role("swarm_worker_w1")
    assert attempt(w, f"select heartbeat('{t}', 'w2', 1, 60)") == MISMATCH


def test_09_w1_cannot_save_artifact_as_w2(admin):
    t = _prepare_task_for(admin, "w2")
    w = as_role("swarm_worker_w1")
    assert attempt(w, f"select save_artifact('{t}', 'w2', 1, 'sha256:a', '/p')") == MISMATCH


def test_10_w1_cannot_complete_as_w2(admin):
    t = _prepare_task_for(admin, "w2")
    w = as_role("swarm_worker_w1")
    assert attempt(w, f"select complete_task('{t}', 'w2', 1, 'sha256:a', 1)") == MISMATCH


# ---------- Нормализация ----------

def test_11_hyphen_normalization(admin):
    """worker_id 'w-1' нормализуется в 'w_1' → роль swarm_worker_w_1.
    Роль создана отдельно в roles_2b.sql, чтобы тест был позитивным."""
    admin.cursor().execute("select register_worker('w-1', '{\"ram_gb\":16}'::jsonb)")
    t = str(uuid.uuid4())
    admin.cursor().execute("select add_task(%s, null, %s::jsonb, 100, 0)", (t, CONTRACT))
    admin.cursor().execute("select promote_ready()")
    w = as_role("swarm_worker_w_1")
    assert attempt(w, "select claim_task('w-1', 60)") is None


def test_11b_hyphen_wrong_role_rejected(admin):
    """Негативный контроль: swarm_worker_w1 не может claim_task('w-1'),
    потому что нормализация даёт 'w_1' → ожидается swarm_worker_w_1."""
    admin.cursor().execute("select register_worker('w-1', '{\"ram_gb\":16}'::jsonb)")
    t = str(uuid.uuid4())
    admin.cursor().execute("select add_task(%s, null, %s::jsonb, 100, 0)", (t, CONTRACT))
    admin.cursor().execute("select promote_ready()")
    w = as_role("swarm_worker_w1")
    assert attempt(w, "select claim_task('w-1', 60)") == MISMATCH


def test_12_case_and_hyphen_normalization(admin):
    """'GPU-1' → 'gpu_1' → swarm_worker_gpu_1. Проверяет и lower(), и regexp_replace()."""
    admin.cursor().execute("select register_worker('GPU-1', '{\"ram_gb\":16}'::jsonb)")
    t = str(uuid.uuid4())
    admin.cursor().execute("select add_task(%s, null, %s::jsonb, 100, 0)", (t, CONTRACT))
    admin.cursor().execute("select promote_ready()")
    w = as_role("swarm_worker_gpu_1")
    assert attempt(w, "select claim_task('GPU-1', 60)") is None


# ---------- Другие роли не затронуты ----------

def test_13_cli_role_unaffected(admin):
    c = as_role("swarm_cli")
    assert attempt(c, f"select add_task('{uuid.uuid4()}', null, '{CONTRACT}'::jsonb, 100, 0)") is None


def test_14_planner_role_unaffected(admin):
    p = as_role("swarm_planner")
    assert attempt(p, f"select add_task('{uuid.uuid4()}', null, '{CONTRACT}'::jsonb, 100, 0)") is None


def test_15_watchdog_role_unaffected(admin):
    wd = as_role("swarm_watchdog")
    assert attempt(wd, "select reclaim_expired()") is None


def test_16_common_worker_role_cannot_login_or_call(admin):
    """Общая роль -- NOLOGIN и не может быть worker identity."""
    cur = admin.cursor()
    cur.execute("select rolcanlogin from pg_roles where rolname = 'swarm_worker'")
    assert cur.fetchone()[0] is False
    cur.execute("select has_function_privilege('swarm_worker', 'claim_task(text, integer)', 'execute')")
    assert cur.fetchone()[0] is True
    with pytest.raises(psycopg2.Error) as exc:
        connect("swarm_worker", os.environ.get("SWARM_PW_WORKER", "pw_worker"))
    assert "is not permitted to log in" in str(exc.value)


# ---------- Каталог ----------

def test_17_catalog_identity_functions(admin):
    cur = admin.cursor()
    cur.execute("""select proname, prosecdef, proconfig, r.rolsuper
                   from pg_proc p join pg_roles r on r.oid = p.proowner
                   where proname in ('normalize_worker_id', 'assert_worker_identity')""")
    rows = cur.fetchall()
    assert len(rows) == 2
    for name, secdef, cfg, is_super in rows:
        assert secdef, f"{name}: не SECURITY DEFINER"
        assert cfg and "search_path=public, pg_temp" in cfg, f"{name}: search_path не закреплён"
        assert not is_super, f"{name}: владелец суперпользователь"


def test_18_assert_worker_identity_not_granted_to_public(admin):
    """assert_worker_identity — внутренняя, не должна быть доступна никакой роли."""
    cur = admin.cursor()
    cur.execute("""
        select has_function_privilege('public', 'assert_worker_identity(text)', 'execute')
    """)
    assert cur.fetchone()[0] is False

    cur.execute("""
        select has_function_privilege('swarm_worker_w1', 'assert_worker_identity(text)', 'execute')
    """)
    assert cur.fetchone()[0] is False

    cur.execute("""
        select has_function_privilege('swarm_worker', 'assert_worker_identity(text)', 'execute')
    """)
    assert cur.fetchone()[0] is False

    cur.execute("select has_function_privilege('public', 'normalize_worker_id(text)', 'execute')")
    assert cur.fetchone()[0] is False


# ---------- Defense-in-depth: SET ROLE на чужую роль запрещён ----------

ALL_WORKER_ROLES = [
    "swarm_worker_w1",
    "swarm_worker_w2",
    "swarm_worker_w_1",
    "swarm_worker_gpu_1",
]


def test_19_no_set_option_on_shared_worker_role(admin):
    """G3: PG16 по умолчанию ставит set_option=true при grant.

    roles_2b.sql явно делает REVOKE SET OPTION FOR swarm_worker FROM
    каждой per-worker роли. Проверяем по каталогу pg_auth_members, что
    set_option == false — иначе компрометация одной роли позволила бы
    SET ROLE на общую worker-роль.
    """
    cur = admin.cursor()
    for role in ALL_WORKER_ROLES:
        cur.execute(
            """
            select am.set_option
              from pg_auth_members am
              join pg_roles m on m.oid = am.member
              join pg_roles r on r.oid = am.roleid
             where m.rolname = %s and r.rolname = 'swarm_worker'
            """,
            (role,),
        )
        row = cur.fetchone()
        assert row is not None, f"{role}: нет membership на swarm_worker"
        assert row[0] is False, f"{role}: set_option на swarm_worker должен быть false (G3)"


def test_20_no_cross_worker_set_option(admin):
    """G3: ни одна worker-роль не имеет SET на любую другую worker-роль."""
    cur = admin.cursor()
    for member in ALL_WORKER_ROLES:
        others = [r for r in ALL_WORKER_ROLES if r != member]
        for target in others:
            cur.execute(
                """
                select coalesce(max(case when am.set_option then 1 else 0 end), 0)
                  from pg_auth_members am
                  join pg_roles m on m.oid = am.member
                  join pg_roles r on r.oid = am.roleid
                 where m.rolname = %s and r.rolname = %s
                """,
                (member, target),
            )
            assert cur.fetchone()[0] == 0, f"{member}: set_option на {target} должен отсутствовать"


def test_21_set_role_spoofing_is_denied(admin):
    """G3 end-to-end: SET ROLE на чужую worker-роль реально отбит Postgres.

    Без set_option попытка SET ROLE завершается ошибкой прав (42501),
    а не только перехватывается session_user-проверкой внутри функции.
    """
    w = as_role("swarm_worker_w1")
    assert attempt(w, "set role swarm_worker_w2") == DENIED
    assert attempt(w, "set role swarm_worker") == DENIED
