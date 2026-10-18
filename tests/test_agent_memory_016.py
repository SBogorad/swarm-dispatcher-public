#!/usr/bin/env python3
"""Обратная связь в памяти роя — миграция 016.

Живой прогон 2026-10-17: `public.agent_memory` осталась ПУСТОЙ (0 строк), хотя
механизм 015 был на месте и его синтетические тесты были зелёными. Причина не
в триггере как таковом, а в том, ЧТО он мог записать:

  1. Условие `state = 'done' AND rework > 0` не выполнилось ни на одной задаче:
     `4a5307d9` и `bfd8d5dd` сгорели в `blocked (max_rework)`, `9482c71a` — в
     `blocked (contract_error: missing_input)`. Провал всех попыток — самый
     важный урок для следующего графа, и он терялся.
  2. Урок собирался из `checks[].id`, а ревьюер писал туда `cmd|true` от
     заглушки планировщика. Настоящая причина отказа (`detail` рубрики) в БД
     не попадала вовсе — оставалась только в stdout-логе.

Красный на схеме без 016 (нет колонки `outcome` → 42703).
Зелёный после 015 + 016.

Соединение:
  W4_ADMIN_DSN — DSN суперпользователя (postgres) на выделенной c2_-БД.

Запуск:
  W4_ADMIN_DSN="host=127.0.0.1 port=... user=postgres password=... dbname=c2_swarm" \
  .venv/bin/python -m pytest -q tests/test_agent_memory_016.py
"""
import json
import os
import re
import uuid

import psycopg2
import pytest

MIGRATION = "schema/migrations/016_agent_memory_feedback.sql"
ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def _dsn() -> str:
    return os.environ.get("W4_ADMIN_DSN", "")


def _parts():
    return dict(re.findall(r"(\w+)=(\S+)", _dsn()))


def connect():
    p = _parts()
    c = psycopg2.connect(
        host=p.get("host", "127.0.0.1"),
        port=int(p.get("port", "5432")),
        dbname=p.get("dbname", "swarm"),
        user=p.get("user", "postgres"),
        password=p.get("password", ""),
    )
    c.autocommit = True
    return c


@pytest.fixture
def admin():
    c = connect()
    c.cursor().execute(
        "truncate agent_memory, events, reviews, task_deps, artifacts, tasks, workers cascade"
    )
    yield c
    c.close()


def _sqlstate(conn, sql, params=None):
    try:
        conn.cursor().execute(sql, params)
        return None
    except psycopg2.Error as e:
        return e.pgcode


def _contract(kind):
    return json.dumps(
        {"kind": kind, "description": "d", "checks": [{"type": "cmd", "run": "true"}]},
        ensure_ascii=False,
    )


def _mk_task(cur, tid, kind, state="ready", lease_owner=None, epoch=0, rework=0):
    cur.execute(
        "insert into tasks (id, root_id, state, contract, budget_cap, rework, "
        "lease_owner, lease_epoch, lease_until) values (%s, %s, %s, %s, %s, %s, "
        "%s, %s, case when %s is null then null else now() + interval '1 hour' end)",
        (tid, tid, state, _contract(kind), 100000, rework, lease_owner, epoch, lease_owner),
    )


def _mk_artifact(cur, tid, ref, by="w1", epoch=0):
    cur.execute(
        "insert into artifacts (task_id, ref, created_by, lease_epoch, storage_path) "
        "values (%s, %s, %s, %s, %s)",
        (tid, ref, by, epoch, "/var/swarm/artifacts/" + ref.replace(":", "_")),
    )


def _mk_review(cur, tid, ref, verdict, checks):
    cur.execute(
        "insert into reviews (task_id, artifact_ref, reviewer, verdict, checks) "
        "values (%s, %s, %s, %s, %s::jsonb)",
        (tid, ref, "rev1", verdict, json.dumps(checks, ensure_ascii=False)),
    )


# ---------- статические якоря ----------


def test_migration_016_exists():
    assert os.path.isfile(os.path.join(ROOT, MIGRATION)), f"нет файла {MIGRATION}"


def test_015_kept_intact():
    """016 добавляет, а не переписывает историю: 015 остаётся на месте."""
    assert os.path.isfile(os.path.join(ROOT, "schema/migrations/015_agent_memory.sql"))


def test_outcome_column_and_constraint(admin):
    cur = admin.cursor()
    cur.execute(
        "select data_type from information_schema.columns "
        "where table_name = 'agent_memory' and column_name = 'outcome'"
    )
    row = cur.fetchone()
    assert row, "нет колонки outcome — миграция 016 не применена"
    assert row[0] == "text"

    # Допустимы ровно два вида урока.
    tid = str(uuid.uuid4())
    _mk_task(cur, tid, "code.feature")
    state = _sqlstate(
        admin,
        "insert into agent_memory (kind, source_task_id, rework_count, lesson, outcome) "
        "values ('code.feature', %s, 0, 'достаточно длинный урок', 'nonsense')",
        (tid,),
    )
    assert state == "23514", f"check-ограничение outcome не сработало: {state}"


def test_unique_index_per_task_and_outcome(admin):
    """Одна задача — не больше одного урока каждого вида."""
    cur = admin.cursor()
    tid = str(uuid.uuid4())
    _mk_task(cur, tid, "code.feature")
    for outcome in ("corrected", "blocked"):
        cur.execute(
            "insert into agent_memory (kind, source_task_id, rework_count, lesson, outcome) "
            "values ('code.feature', %s, 0, 'первый урок этого вида', %s)",
            (tid, outcome),
        )
    # Дубль того же вида запрещён индексом.
    state = _sqlstate(
        admin,
        "insert into agent_memory (kind, source_task_id, rework_count, lesson, outcome) "
        "values ('code.feature', %s, 0, 'дубль того же вида урока', 'blocked')",
        (tid,),
    )
    assert state == "23505", f"дубль (task, outcome) прошёл: {state}"


# ---------- урок провала: главный новый контур ----------


def test_blocked_task_writes_lesson(admin):
    """`blocked` — самый ценный урок, и раньше он терялся полностью."""
    cur = admin.cursor()
    tid = str(uuid.uuid4())
    _mk_task(cur, tid, "code.feature", state="review")
    _mk_artifact(cur, tid, "sha256:bbb")
    _mk_review(
        cur,
        tid,
        "sha256:bbb",
        "changes_requested",
        [
            {"id": "cmd|true", "type": "cmd", "run": "true", "status": "fail"},
            {
                "id": "rubric",
                "type": "rubric",
                "run": "review_rubric",
                "status": "fail",
                "detail": "[RUBRIC:TRAIL] evidence `ok` не является указателем",
            },
        ],
    )
    cur.execute(
        "update tasks set state = 'blocked', block_reason = 'max_rework' where id = %s",
        (tid,),
    )

    cur.execute(
        "select outcome, kind, lesson from agent_memory where source_task_id = %s", (tid,)
    )
    row = cur.fetchone()
    assert row, "урок провала не записан — контур `blocked` не работает"
    outcome, kind, lesson = row
    assert outcome == "blocked"
    assert kind == "code.feature"
    assert "max_rework" in lesson, f"причина блокировки не попала в урок: {lesson}"
    assert "RUBRIC:TRAIL" in lesson, f"detail рубрики не попал в урок: {lesson}"
    assert "cmd|true" not in lesson, f"в урок утекло мусорное id заглушки: {lesson}"


def test_blocked_lesson_includes_contract_error(admin):
    """`missing_input` — тоже урок: постановка была неисполнима."""
    cur = admin.cursor()
    tid = str(uuid.uuid4())
    _mk_task(cur, tid, "code.feature", state="review")
    _mk_artifact(cur, tid, "sha256:ccc")
    _mk_review(
        cur,
        tid,
        "sha256:ccc",
        "rejected",
        [
            {
                "id": "rubric",
                "type": "rubric",
                "run": "review_rubric",
                "status": "fail",
                "detail": "[RUBRIC:CONTRACT] контракт неисполнимо: missing_input",
            }
        ],
    )
    cur.execute(
        "update tasks set state = 'blocked', "
        "block_reason = 'contract_error: missing_input' where id = %s",
        (tid,),
    )
    cur.execute("select outcome, lesson from agent_memory where source_task_id = %s", (tid,))
    row = cur.fetchone()
    assert row and row[0] == "blocked"
    assert "missing_input" in row[1]


def test_blocked_lesson_is_idempotent(admin):
    """Повторный переход в blocked не плодит дубли."""
    cur = admin.cursor()
    tid = str(uuid.uuid4())
    _mk_task(cur, tid, "code.feature", state="review")
    _mk_artifact(cur, tid, "sha256:ddd")
    _mk_review(cur, tid, "sha256:ddd", "changes_requested", [])
    cur.execute("update tasks set state = 'blocked', block_reason = 'max_rework' where id = %s", (tid,))
    cur.execute("update tasks set state = 'ready' where id = %s", (tid,))
    cur.execute("update tasks set state = 'blocked', block_reason = 'max_rework' where id = %s", (tid,))
    cur.execute("select count(*) from agent_memory where source_task_id = %s", (tid,))
    assert cur.fetchone()[0] == 1


# ---------- урок преодоления: теперь с реальным detail ----------


def test_corrected_lesson_uses_rubric_detail(admin):
    """Урок преодоления строится из `detail`, а не из id заглушки."""
    cur = admin.cursor()
    tid = str(uuid.uuid4())
    _mk_task(cur, tid, "code.architect", state="review")
    _mk_artifact(cur, tid, "sha256:eee")
    _mk_review(
        cur,
        tid,
        "sha256:eee",
        "changes_requested",
        [
            {
                "id": "rubric",
                "type": "rubric",
                "run": "review_rubric",
                "status": "fail",
                "detail": "[RUBRIC:TRAIL] нет секции `## Handoff`",
            }
        ],
    )
    cur.execute("update tasks set rework = 1 where id = %s", (tid,))
    cur.execute("update tasks set state = 'done', result_ref = 'sha256:eee' where id = %s", (tid,))

    cur.execute(
        "select outcome, lesson from agent_memory where source_task_id = %s", (tid,)
    )
    row = cur.fetchone()
    assert row, "урок преодоления не записан"
    outcome, lesson = row
    assert outcome == "corrected"
    assert "## Handoff" in lesson, f"detail рубрики не попал в урок: {lesson}"


def test_both_outcomes_coexist_and_are_recalled(admin):
    """Урок провала и урок преодоления — разные события одной задачи.

    И оба должны доезжать до следующего графа через `task_recall_memory`.
    """
    cur = admin.cursor()
    tid = str(uuid.uuid4())
    _mk_task(cur, tid, "code.feature", state="review")
    _mk_artifact(cur, tid, "sha256:fff")
    _mk_review(
        cur,
        tid,
        "sha256:fff",
        "changes_requested",
        [
            {
                "id": "rubric",
                "type": "rubric",
                "run": "review_rubric",
                "status": "fail",
                "detail": "[RUBRIC:UNSLOP] artifact contains unfinished stub marker",
            }
        ],
    )
    # Первый провал — blocked.
    cur.execute("update tasks set state = 'blocked', block_reason = 'max_rework' where id = %s", (tid,))
    # Человек переоткрыл и задача всё-таки прошла.
    cur.execute("update tasks set state = 'ready' where id = %s", (tid,))
    cur.execute("update tasks set rework = 2 where id = %s", (tid,))
    cur.execute("update tasks set state = 'done', result_ref = 'sha256:fff' where id = %s", (tid,))

    cur.execute(
        "select outcome from agent_memory where source_task_id = %s order by outcome", (tid,)
    )
    assert [r[0] for r in cur.fetchall()] == ["blocked", "corrected"]

    # Граф №2: другой root_id, тот же kind, живая аренда.
    t2 = str(uuid.uuid4())
    _mk_task(cur, t2, "code.feature", state="leased", lease_owner="w1", epoch=1)
    cur.execute("select lesson from task_recall_memory(%s, %s, %s)", (t2, "w1", 1))
    lessons = [r[0] for r in cur.fetchall()]
    assert any("RUBRIC:UNSLOP" in lesson for lesson in lessons), lessons
    assert any("заблокирована" in lesson for lesson in lessons), lessons
