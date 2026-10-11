"""Negative tests for the review gate (P0 #5, updated for F2/F15 wave 4).

Regression coverage for the review gate in `submit_review`:
  * the old length-only check accepted N fake `{"status":"pass"}` items with no
    matching type, bypassing `contract.checks`;
  * the later EXISTS-by-type check still allowed ONE generic
    `{"type":"cmd","status":"pass"}` to close TWO `cmd` requirements (F2).

Current rule (single source schema/schema_v0.sql): the comparison key is
`coalesce(id, type || '|' || run)`; for every key, the number of `pass` items in
the report must be >= the number of contract requirements with that key.
`type` is mandatory (F15); `id` is optional.

NOTE: step2/roles_patch_submit_review.sql was removed (F16г) — submit_review
lives ONLY in schema/schema_v0.sql now.

Run (DB with schema_v0.sql + roles.sql + roles_2b.sql, passwords pw_<role>):
  ROLES_DB=v0roles ROLES_HOST=localhost ADMIN_PW=pw pytest -q step2/test_review_gate.py
"""
import json
import os
import uuid

import psycopg2
import pytest

DB = os.environ.get("ROLES_DB", "v0roles")
HOST = os.environ.get("ROLES_HOST", "localhost")
PORT = int(os.environ.get("ROLES_PORT", "5432"))
ADMIN_PW = os.environ.get("ADMIN_PW", "pw")
RV003 = "RV003"

# Two required checks: a bypass must supply BOTH types to pass.
CONTRACT = json.dumps({
    "kind": "t",
    "checks": [{"type": "unit"}, {"type": "lint"}],
    "budget": {"attempts": 3, "max_rework": 3},
})


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


def attempt(conn, sql, params=None):
    """None if executed; otherwise SQLSTATE."""
    try:
        conn.cursor().execute(sql, params)
        return None
    except psycopg2.Error as e:
        return e.pgcode


@pytest.fixture
def admin():
    c = connect("postgres", ADMIN_PW)
    c.cursor().execute("truncate events, reviews, task_deps, artifacts, tasks, workers cascade")
    yield c
    c.close()


def _task_in_review(admin):
    """Drive a task to state='review' with a stored artifact; return (task_id, ref)."""
    pl = as_role("swarm_planner")
    wk = as_role("swarm_worker_w1")
    task = str(uuid.uuid4())
    c = pl.cursor()
    c.execute("select add_task(%s, null, %s::jsonb, 1000, 0)", (task, CONTRACT))
    c.execute("select promote_ready()")
    c = wk.cursor()
    c.execute("select register_worker('w1', '{\"ram_gb\":16}'::jsonb)")
    c.execute("select out_task_id, out_epoch from claim_task('w1', 60)")
    _, epoch = c.fetchone()
    ref = "sha256:gate"
    c.execute("select save_artifact(%s, 'w1', %s, %s, '/p')", (task, epoch, ref))
    c.execute("select complete_task(%s, 'w1', %s, %s, 1)", (task, epoch, ref))
    assert c.fetchone()[0] == "review"
    return task, ref


def test_length_only_bypass_is_rejected(admin):
    """The old attack: two pass items without type must NOT satisfy a 2-check contract."""
    task, ref = _task_in_review(admin)
    rv = as_role("swarm_reviewer")
    fake = json.dumps([{"status": "pass"}, {"status": "pass"}])
    assert attempt(rv,
                   "select submit_review(%s, %s, 'rev', 'approved', %s::jsonb)",
                   (task, ref, fake)) == RV003


def test_missing_one_required_type_is_rejected(admin):
    """Only one of the two required types present → still RV003."""
    task, ref = _task_in_review(admin)
    rv = as_role("swarm_reviewer")
    partial = json.dumps([{"type": "unit", "status": "pass"}])
    assert attempt(rv,
                   "select submit_review(%s, %s, 'rev', 'approved', %s::jsonb)",
                   (task, ref, partial)) == RV003


def test_failed_status_on_required_type_is_rejected(admin):
    """Matching types but one status != pass → RV003."""
    task, ref = _task_in_review(admin)
    rv = as_role("swarm_reviewer")
    mixed = json.dumps([
        {"type": "unit", "status": "pass"},
        {"type": "lint", "status": "fail"},
    ])
    assert attempt(rv,
                   "select submit_review(%s, %s, 'rev', 'approved', %s::jsonb)",
                   (task, ref, mixed)) == RV003


def test_full_matching_checks_are_accepted(admin):
    """All required types present with pass → approved, task done."""
    task, ref = _task_in_review(admin)
    rv = as_role("swarm_reviewer")
    good = json.dumps([
        {"type": "unit", "status": "pass"},
        {"type": "lint", "status": "pass"},
    ])
    c = rv.cursor()
    c.execute("select submit_review(%s, %s, 'rev', 'approved', %s::jsonb)",
              (task, ref, good))
    assert c.fetchone()[0] == "done"


def test_extra_passing_checks_are_allowed(admin):
    """Extra pass items beyond the contract must not break acceptance."""
    task, ref = _task_in_review(admin)
    rv = as_role("swarm_reviewer")
    extra = json.dumps([
        {"type": "unit", "status": "pass"},
        {"type": "lint", "status": "pass"},
        {"type": "smoke", "status": "pass"},
    ])
    c = rv.cursor()
    c.execute("select submit_review(%s, %s, 'rev', 'approved', %s::jsonb)",
              (task, ref, extra))
    assert c.fetchone()[0] == "done"
