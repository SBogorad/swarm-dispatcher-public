#!/usr/bin/env python3
"""Лимит причины блокировки — миграция 017.

Живой урок `agent_memory#4` (2026-10-18, задача `76c2142c`, `code.verify`):
урок в базе имел длину ровно 655 символов и был оборван на полуслове
(«...ac):»). Арифметика точная: 155 символов префикса консолидатора плюс
ровно 500 символов причины. Причина резалась в `fail_task_state` до того,
как попадала в текст урока, — и самая полезная часть (`missing_input`
перечисляет, ЧЕГО не хватило) терялась.

Тест ловит обе половины:
  * статически — тело функций в живой БД содержит лимит 2000, а не 500;
  * поведенчески — причина длиной 900 символов доезжает до урока ЦЕЛИКОМ,
    включая хвост, который при лимите 500 исчезал.

Соединение:
  W4_ADMIN_DSN — DSN суперпользователя (postgres) на выделенной c2_-БД.

Запуск:
  W4_ADMIN_DSN="host=127.0.0.1 port=... user=postgres password=... dbname=c2_swarm" \
  .venv/bin/python -m pytest -q tests/test_agent_memory_017.py
"""
import json
import os
import re
import uuid

import psycopg2
import pytest

MIGRATION = "schema/migrations/017_block_reason_limit.sql"
ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

# Верхняя граница полезного текста: ровно CHECK на agent_memory.lesson из 015.
LESSON_MAX = 2000
# Лимит, который стоял до 017. Мутация «вернуть 500» обязана краснеть.
OLD_LIMIT = 500


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


def _contract(kind):
    return json.dumps(
        {"kind": kind, "description": "d", "checks": [{"type": "cmd", "run": "true"}]},
        ensure_ascii=False,
    )


def _mk_task(cur, tid, kind, state="ready", rework=0):
    cur.execute(
        "insert into tasks (id, root_id, state, contract, budget_cap, rework) "
        "values (%s, %s, %s, %s, %s, %s)",
        (tid, tid, state, _contract(kind), 100000, rework),
    )


# ---------- статические якоря ----------

def test_migration_file_exists():
    path = os.path.join(ROOT, MIGRATION)
    assert os.path.isfile(path), f"нет файла {MIGRATION}"
    text = open(path, encoding="utf-8").read()
    # Идемпотентность: повторный прогон не должен падать.
    assert "create or replace function" in text
    # Защита не теряется: create-or-replace без identity тихо снимал бы её.
    assert text.count("assert_worker_identity") >= 2, (
        "миграция пересоздаёт fail_task/fail_task_state, но не воспроизводит "
        "identity-проверку — create or replace снимет защиту"
    )


def test_canonical_schema_has_no_old_limit():
    """Канон и перекрытие согласованы: иначе drift guard поймает расхождение."""
    for rel in ("schema/schema_v0.sql", "step2b/roles_2b.sql"):
        text = open(os.path.join(ROOT, rel), encoding="utf-8").read()
        assert ", 500)" not in text, f"{rel} всё ещё режет причину на 500"
        assert ", 2000)" in text, f"{rel} не поднял лимит до 2000"


def test_live_function_bodies_use_new_limit(admin):
    """Побеждающее определение в БД — с лимитом 2000.

    Это и есть поведенческий якорь: `create or replace` из 017 обязан
    перекрыть тела из roles_2b. Если миграцию не применили (или откатили
    лимит обратно), `prosrc` покажет старое число.
    """
    cur = admin.cursor()
    for fn in ("fail_task", "fail_task_state"):
        cur.execute(
            "select prosrc from pg_proc p join pg_namespace n on n.oid = p.pronamespace "
            "where n.nspname = 'public' and p.proname = %s",
            (fn,),
        )
        rows = cur.fetchall()
        assert rows, f"нет функции {fn} в живой БД"
        src = rows[-1][0]
        assert f", {LESSON_MAX})" in src, f"{fn}: лимит не поднят до {LESSON_MAX}"
        assert f", {OLD_LIMIT})" not in src, f"{fn}: остался старый лимит {OLD_LIMIT}"


# ---------- поведение: длинная причина не теряет хвост ----------

def test_long_block_reason_reaches_lesson_intact(admin):
    """Причина 900 символов доезжает до урока целиком, вместе с хвостом.

    До 017 хвост за 500-м символом исчезал, и урок становился неприменимым:
    «заблокирована, причина: ...» без перечня нехватки.
    """
    cur = admin.cursor()
    tid = str(uuid.uuid4())
    _mk_task(cur, tid, "code.verify", state="review")

    head = "contract_error: missing_input: "
    tail = "НЕ ХВАТИЛО: crates/swarm-worker/src/prompt.rs:70 (правило handoff)"
    filler = "x" * (900 - len(head) - len(tail))
    reason = head + filler + tail
    assert len(reason) == 900
    assert len(reason) > OLD_LIMIT

    cur.execute(
        "update tasks set state = 'blocked', block_reason = %s where id = %s",
        (reason, tid),
    )
    cur.execute("select lesson from agent_memory where source_task_id = %s", (tid,))
    row = cur.fetchone()
    assert row, "урок провала не записан"
    lesson = row[0]
    assert len(lesson) <= LESSON_MAX, f"урок длиннее CHECK-границы: {len(lesson)}"
    assert tail in lesson, (
        "хвост причины потерян — блокировка снова режется по старому лимиту: "
        f"len={len(lesson)}"
    )


def test_lesson_stays_within_check_bound(admin):
    """Лимит 2000 не ломает CHECK(lesson <= 2000): консолидатор режет сам."""
    cur = admin.cursor()
    tid = str(uuid.uuid4())
    _mk_task(cur, tid, "code.feature", state="review")
    reason = "y" * 5000
    cur.execute(
        "update tasks set state = 'blocked', block_reason = %s where id = %s",
        (reason, tid),
    )
    cur.execute("select length(lesson) from agent_memory where source_task_id = %s", (tid,))
    row = cur.fetchone()
    assert row, "урок не записан при длинной причине"
    assert row[0] <= LESSON_MAX, f"вставка обошла CHECK: {row[0]}"
