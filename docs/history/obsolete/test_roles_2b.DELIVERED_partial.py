"""Тесты идентификации воркера (roles_2b.sql).

Запуск (БД с schema_v0.sql + roles.sql + roles_2b.sql):
  ROLES_DB=swarm ROLES_HOST=127.0.0.1 ADMIN_PW=pw \
  SWARM_PW_W1=pw_w1 SWARM_PW_W2=pw_w2 \
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
U = "'00000000-0000-0000-0000-000000000001'"
CONTRACT = json.dumps({"kind": "t", "checks": [{"type": "cmd"}], "budget": {"attempts": 3}})


def connect(user, pw):
    c = psycopg2.connect(host=HOST, port=PORT, dbname=DB, user=user, password=pw)
    c.autocommit = True
    return c


def as_role(role):
    suffix = role.split("_", 1)[1]
    return connect(role, os.environ.get("SWARM_PW_" + suffix.upper(), "pw_" + suffix))


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


# ---------- Позитивные: своя роль, своё имя ----------

def test_w1_register_self(admin):
    w = as_role("swarm_worker_w1")
    assert attempt(w, "select register_worker('w1', '{\"ram_gb\":16}'::jsonb)") is None


def test_w1_claim_self(admin):
    admin.cursor().execute("select register_worker('w1', '{\"ram_gb\":16}'::jsonb)")
    t = str(uuid.uuid4())
    admin.cursor().execute("select add_task(%s, null, %s::jsonb, 100, 0)", (t, CONTRACT))
    admin.cursor().execute("select promote_ready()")
    w = as_role("swarm_worker_w1")
    assert attempt(w, "select claim_task('w1', 60)") is None


def test_w1_heartbeat_self(admin):
    admin.cursor().execute("select register_worker('w1', '{\"ram_gb\":16}'::jsonb)")
    t = str(uuid.uuid4())
    admin.cursor().execute("select add_task(%s, null, %s::jsonb, 100, 0)", (t, CONTRACT))
    admin.cursor().execute("select promote_ready()")
    admin.cursor().execute("select claim_task('w1', 60)")
    w = as_role("swarm_worker_w1")
    # epoch после claim = 1
    assert attempt(w, f"select heartbeat('{t}', 'w1', 1, 60)") is None


def test_w1_save_artifact_self(admin):
    admin.cursor().execute("select register_worker('w1', '{\"ram_gb\":16}'::jsonb)")
    t = str(uuid.uuid4())
    admin.cursor().execute("select add_task(%s, null, %s::jsonb, 100, 0)", (t, CONTRACT))
    admin.cursor().execute("select promote_ready()")
    admin.cursor().execute("select claim_task('w1', 60)")
    w = as_role("swarm_worker_w1")
    assert attempt(w, f"select save_artifact('{t}', 'w1', 1, 'sha256:a', '/p')") is None


def test_w1_complete_self(admin):
    admin.cursor().execute("select register_worker('w1', '{\"ram_gb\":16}'::jsonb)")
    t = str(uuid.uuid4())
    admin.cursor().execute("select add_task(%s, null, %s::jsonb, 100, 0)", (t, CONTRACT))
