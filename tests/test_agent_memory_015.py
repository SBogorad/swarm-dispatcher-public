#!/usr/bin/env python3
"""Память роя (`reflect` + `correct` + `encode-lessons-in-structure`) — миграция 015.

Дефект (разбор pstack-навыков `reflect`/`correct`):
    У агента в CLI есть `MEMORY.md`: между сессиями он помнит, на чём обжёгся.
    У роя такой памяти нет. `012_claim_task_rework_feedback` и
    `013_task_parent_artifacts` живут ВНУТРИ одного графа: граф завершился —
    наступила амнезия, и следующий граф повторяет те же ошибки на ревью.
    Контейнер воркера `--read-only` без диска, своего `MEMORY.md` у него нет.

Фикс: таблица `public.agent_memory` + триггер `tasks_consolidate_memory`
(пишет урок при `done` после `rework > 0`) + RPC `task_recall_memory`
(отдаёт последние 5 уроков прошлых графов держателю живой аренды).

Красный на коде без миграции (42P01/42883 → фейл, не skip).
Зелёный после миграции 015.

Соединение:
  W4_ADMIN_DSN — DSN суперпользователя (postgres) на выделенной c2_-БД.

Запуск:
  W4_ADMIN_DSN="host=127.0.0.1 port=... user=postgres password=... dbname=c2_swarm" \
  .venv/bin/python -m pytest -q tests/test_agent_memory_015.py
"""
import json
import os
import re
import uuid

import psycopg2
import pytest

MIGRATION = "schema/migrations/015_agent_memory.sql"
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


def _mk_task(cur, tid, kind, state="ready", lease_owner=None, epoch=1, rework=0):
    cur.execute(
        "insert into tasks (id, root_id, state, contract, budget_cap, rework, "
        "lease_owner, lease_epoch, lease_until) values (%s, %s, %s, %s, %s, %s, "
        "%s, %s, case when %s is null then null else now() + interval '1 hour' end)",
        (tid, tid, state, _contract(kind), 100000, rework, lease_owner, epoch, lease_owner),
    )


def _mk_artifact(cur, tid, ref, by="w1", epoch=1):
    cur.execute(
        "insert into artifacts (task_id, ref, created_by, lease_epoch, storage_path) "
        "values (%s, %s, %s, %s, %s)",
        (tid, ref, by, epoch, "/var/swarm/artifacts/" + ref.replace(":", "_")),
    )


# ---------- статический якорь ----------

def test_agent_memory_migration_file_exists():
    path = os.path.join(ROOT, MIGRATION)
    assert os.path.isfile(path), f"нет файла миграции: {MIGRATION}"
    body = open(path, encoding="utf-8").read().lower()
    assert "create table if not exists public.agent_memory" in body
    assert "security definer" in body
    assert "set search_path = public, pg_temp" in body
    # Оба контура памяти: автоматический урок и шлюз вспоминания.
    assert "consolidate_task_memory" in body
    assert "task_recall_memory" in body
    # RPC вспоминания обязана проверять identity и ЖИВУЮ аренду.
    assert "assert_worker_identity" in body
    assert "assert_lease" in body
    # Прямой DML закрыт: только через SECURITY DEFINER.
    assert "revoke all on table public.agent_memory from public" in body


def test_agent_memory_not_numbered_014():
    """014 занята миграцией identity — два файла с одним номером разъехались бы."""
    assert os.path.isfile(os.path.join(ROOT, "schema/migrations/014_identity_in_override_migrations.sql"))


# ---------- каталог ----------

def test_agent_memory_rpc_in_catalog(admin):
    cur = admin.cursor()
    cur.execute(
        "select p.prosecdef, p.proconfig, p.proowner::regrole::text from pg_proc p "
        "where p.proname = 'task_recall_memory'"
    )
    row = cur.fetchone()
    assert row, "нет RPC task_recall_memory"
    secdef, cfg, owner = row
    assert secdef, "task_recall_memory обязана быть SECURITY DEFINER"
    assert cfg and "search_path=public, pg_temp" in cfg, f"search_path не закреплён: {cfg}"
    assert owner == "swarm_owner", f"владелец RPC: {owner}"


def test_agent_memory_trigger_present(admin):
    cur = admin.cursor()
    cur.execute(
        "select tgname from pg_trigger "
        "where tgrelid = 'public.tasks'::regclass and not tgisinternal"
    )
    names = {r[0] for r in cur.fetchall()}
    assert "tasks_consolidate_memory" in names, f"нет триггера: {names}"


def test_agent_memory_table_owner_and_no_public_dml(admin):
    cur = admin.cursor()
    cur.execute(
        "select pg_get_userbyid(relowner) from pg_class where relname = 'agent_memory'"
    )
    assert cur.fetchone()[0] == "swarm_owner"
    cur.execute(
        "select has_table_privilege('public', 'public.agent_memory', 'INSERT')"
    )
    assert cur.fetchone()[0] is False, "прямой INSERT для PUBLIC открыт"


# ---------- цикл самообучения ----------

def test_cycle_lesson_written_and_recalled_across_graphs(admin):
    """Граф №1 учится, граф №2 вспоминает — это и есть рекурсивная память."""
    cur = admin.cursor()
    t1 = str(uuid.uuid4())
    _mk_task(cur, t1, "code.feature")
    _mk_artifact(cur, t1, "sha256:aaa")
    cur.execute(
        "insert into reviews (task_id, artifact_ref, reviewer, verdict, checks) "
        "values (%s, %s, %s, %s, %s::jsonb)",
        (
            t1,
            "sha256:aaa",
            "rev1",
            "changes_requested",
            json.dumps(
                [
                    {"id": "cargo clippy", "type": "cmd", "run": "cargo clippy", "status": "fail"},
                    {"id": "cargo test", "type": "cmd", "run": "cargo test", "status": "pass"},
                ]
            ),
        ),
    )
    # Исправление со второй попытки: rework=1 и переход в done.
    cur.execute("update tasks set rework = 1 where id = %s", (t1,))
    cur.execute(
        "update tasks set state = 'done', result_ref = 'sha256:aaa' where id = %s", (t1,)
    )

    cur.execute("select kind, rework_count, lesson from agent_memory where source_task_id = %s", (t1,))
    row = cur.fetchone()
    assert row, "урок не записан триггером consolidate_task_memory"
    kind, rework_count, lesson = row
    assert kind == "code.feature"
    assert rework_count == 1
    assert "cargo clippy" in lesson, f"проваленная проверка не попала в урок: {lesson}"

    # Граф №2: другой root_id, тот же kind, живая аренда w1.
    t2 = str(uuid.uuid4())
    _mk_task(cur, t2, "code.feature", state="leased", lease_owner="w1", epoch=1)
    cur.execute("select lesson, kind from task_recall_memory(%s, %s, %s)", (t2, "w1", 1))
    recalled = cur.fetchall()
    assert recalled, "граф №2 не получил урок из графа №1"
    assert any("cargo clippy" in r[0] for r in recalled), recalled


def test_recall_requires_live_lease(admin):
    """Без живой аренды шлюз обязан отказать (LS001), а не отдать память роя."""
    cur = admin.cursor()
    t1 = str(uuid.uuid4())
    _mk_task(cur, t1, "code.feature")  # state=ready, аренды нет
    state = _sqlstate(admin, "select * from task_recall_memory(%s, %s, %s)", (t1, "w1", 1))
    assert state == "LS001", f"ожидался lease_lost, получено {state}"


def test_recall_excludes_own_task(admin):
    """Своя задача не вспоминает собственный урок: черновик — не знание."""
    cur = admin.cursor()
    t1 = str(uuid.uuid4())
    _mk_task(cur, t1, "code.feature", state="leased", lease_owner="w1", epoch=1)
    cur.execute(
        "insert into agent_memory (kind, source_task_id, rework_count, lesson) "
        "values (%s, %s, %s, %s)",
        ("code.feature", t1, 1, "урок этой же задачи, не должен вернуться"),
    )
    cur.execute("select count(*) from task_recall_memory(%s, %s, %s)", (t1, "w1", 1))
    assert cur.fetchone()[0] == 0


def test_recall_includes_wildcard_lessons(admin):
    """Урок `*` применим к любому виду работ."""
    cur = admin.cursor()
    t1 = str(uuid.uuid4())
    t2 = str(uuid.uuid4())
    _mk_task(cur, t1, "code.bugfix")
    cur.execute(
        "insert into agent_memory (kind, source_task_id, rework_count, lesson) "
        "values (%s, %s, %s, %s)",
        ("*", t1, 1, "Всегда заполняй Decision Trail до сдачи"),
    )
    _mk_task(cur, t2, "code.feature", state="leased", lease_owner="w1", epoch=1)
    cur.execute("select kind from task_recall_memory(%s, %s, %s)", (t2, "w1", 1))
    assert [r[0] for r in cur.fetchall()] == ["*"]


def test_lesson_written_once_per_task(admin):
    """reopen → done не должен плодить дубли уроков по одной задаче."""
    cur = admin.cursor()
    t1 = str(uuid.uuid4())
    _mk_task(cur, t1, "code.feature", rework=2)
    _mk_artifact(cur, t1, "sha256:aaa")
    cur.execute(
        "insert into reviews (task_id, artifact_ref, reviewer, verdict, checks) "
        "values (%s, %s, %s, %s, %s::jsonb)",
        (t1, "sha256:aaa", "rev1", "changes_requested",
         json.dumps([{"id": "x", "type": "cmd", "run": "x", "status": "fail"}])),
    )
    cur.execute("update tasks set state = 'done', result_ref = 'sha256:aaa' where id = %s", (t1,))
    cur.execute("update tasks set state = 'review' where id = %s", (t1,))
    cur.execute("update tasks set state = 'done' where id = %s", (t1,))
    cur.execute("select count(*) from agent_memory where source_task_id = %s", (t1,))
    assert cur.fetchone()[0] == 1
