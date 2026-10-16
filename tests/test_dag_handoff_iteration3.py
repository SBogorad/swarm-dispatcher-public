#!/usr/bin/env python3
"""Итерация 3 (DAG handoff) — арбитр RPC `task_parent_artifacts`.

Дефект (нашёл разбор pstack-навыка `handoff`):
    Задачи графа связаны `task_deps(task_id=t2, dep_id=t1)`, но воркер, взявший
    `t2`, НЕ видит результат `t1`: `claim_task` возвращает только контракт
    своей задачи, а прямого SELECT к `tasks`/`artifacts` у сервисных ролей нет
    (least privilege). Каждая задача графа решает свою часть вслепую.

Фикс: SECURITY DEFINER RPC `task_parent_artifacts(task, worker, epoch)`,
которая отдаёт последние артефакты зависимостей — но ТОЛЬКО держателю живой
аренды дочерней задачи. Это не новая колонка `claim_task` (смена row type
требует DROP FUNCTION и перевыдачи грантов), а отдельная функция.

Красный на коде без RPC (42883 function does not exist → фейл, не skip).
Зелёный после миграции 013.

Соединение:
  W4_ADMIN_DSN — DSN суперпользователя (postgres) на выделенной c2_-БД.
  W4_W1_PW / W4_W2_PW — пароли per-worker ролей (identity-проверка RPC).

Запуск:
  W4_ADMIN_DSN="host=127.0.0.1 port=... user=postgres password=... dbname=c2_swarm" \
  W4_W1_PW=... W4_W2_PW=... \
  .venv/bin/python -m pytest -q tests/test_dag_handoff_iteration3.py
"""
import json
import os
import re
import uuid

import psycopg2
import pytest

# Миграция 013 — предмет проверки присутствия в применяемой цепочке.
MIGRATION = "schema/migrations/013_task_parent_artifacts.sql"
ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def _dsn() -> str:
    return os.environ.get("W4_ADMIN_DSN", "")


def _parts():
    return dict(re.findall(r"(\w+)=(\S+)", _dsn()))


def connect(user, password):
    p = _parts()
    c = psycopg2.connect(
        host=p.get("host", "127.0.0.1"),
        port=int(p.get("port", "5432")),
        dbname=p.get("dbname", "swarm"),
        user=user,
        password=password,
    )
    c.autocommit = True
    return c


def _sqlstate(conn, sql, params=None):
    try:
        conn.cursor().execute(sql, params)
        return None
    except psycopg2.Error as e:
        return e.pgcode


@pytest.fixture
def admin():
    c = psycopg2.connect(_dsn())
    c.autocommit = True
    c.cursor().execute("truncate events, reviews, task_deps, artifacts, tasks, workers cascade")
    yield c
    c.close()


def _mk_worker(cur, wid):
    cur.execute(
        "insert into workers (id, capabilities) values (%s, %s) "
        "on conflict (id) do update set last_seen = now()",
        (wid, '{"ram_gb":16}'),
    )


def _contract(kind):
    """Контракт задачи — через json.dumps: jsonb не принимает одинарные кавычки."""
    return json.dumps(
        {"kind": kind, "description": "d", "checks": [{"type": "cmd", "run": "true"}]},
        ensure_ascii=False,
    )


def _mk_task(cur, tid, state, kind, lease_owner=None, epoch=0):
    cur.execute(
        "insert into tasks (id, root_id, state, contract, budget_cap, lease_owner, "
        "lease_epoch, lease_until) values (%s, %s, %s, %s, %s, %s, %s, "
        "case when %s is null then null else now() + interval '1 hour' end)",
        (
            tid,
            tid,
            state,
            _contract(kind),
            100000,
            lease_owner,
            epoch,
            lease_owner,
        ),
    )


def _mk_artifact(cur, tid, ref, by="w1", epoch=0):
    cur.execute(
        "insert into artifacts (task_id, ref, created_by, lease_epoch, storage_path) "
        "values (%s, %s, %s, %s, %s)",
        (tid, ref, by, epoch, "/var/swarm/artifacts/" + ref.replace(":", "_")),
    )


# ---------- статический якорь: файл миграции присутствует ----------

def test_iteration3_migration_file_exists():
    """Без файла миграции RPC не поднимется ни в harness, ни в проде."""
    path = os.path.join(ROOT, MIGRATION)
    assert os.path.isfile(path), f"нет файла миграции: {MIGRATION}"
    body = open(path, encoding="utf-8").read().lower()
    assert "security definer" in body, "RPC обязана быть security definer"
    assert "set search_path = public, pg_temp" in body, "RPC без пина search_path"
    assert "assert_worker_identity" in body, "RPC обязана проверять identity воркера"
    assert "assert_lease" in body, "RPC обязана проверять ЖИВУЮ аренду задачи"


# ---------- присутствие в каталоге ----------

def test_iteration3_rpc_in_catalog(admin):
    cur = admin.cursor()
    cur.execute(
        "select prosecdef, provolatile, proowner::regrole::text from pg_proc "
        "where proname = 'task_parent_artifacts'"
    )
    row = cur.fetchone()
    assert row is not None, "RPC task_parent_artifacts не найдена в каталоге"
    secdef, _, owner = row
    assert secdef is True, "RPC должна быть SECURITY DEFINER"
    assert owner == "swarm_owner", f"владелец RPC — {owner}, а не swarm_owner"


def test_iteration3_grant_limited_to_worker(admin):
    """Право EXECUTE — у swarm_worker; у public его нет (deny-by-default)."""
    cur = admin.cursor()
    cur.execute(
        "select has_function_privilege('public', "
        "'task_parent_artifacts(uuid,text,bigint)', 'execute')"
    )
    assert cur.fetchone()[0] is False, "public не должен иметь EXECUTE"
    cur.execute(
        "select has_function_privilege('swarm_worker', "
        "'task_parent_artifacts(uuid,text,bigint)', 'execute')"
    )
    assert cur.fetchone()[0] is True, "swarm_worker обязан иметь EXECUTE"


# ---------- поведение ----------

def test_iteration3_returns_parent_artifacts(admin):
    """Счастливый путь: арендатор t2 видит артефакты t1 с kind родителя."""
    t1, t2 = str(uuid.uuid4()), str(uuid.uuid4())
    cur = admin.cursor()
    _mk_worker(cur, "w1")
    # t1 — уже завершённый родитель (done), его артефакт и есть handoff.
    _mk_task(cur, t1, "done", "code.investigate")
    _mk_artifact(cur, t1, "sha256:deadbeef")
    _mk_task(cur, t2, "leased", "code.feature", lease_owner="w1", epoch=3)
    cur.execute("insert into task_deps (task_id, dep_id) values (%s, %s)", (t2, t1))

    w = connect("swarm_worker_w1", os.environ.get("W4_W1_PW", ""))
    cur2 = w.cursor()
    cur2.execute("select parent_task_id, parent_kind, artifact_ref, artifact_sha256 "
                 "from task_parent_artifacts(%s, 'w1', 3)", (t2,))
    rows = cur2.fetchall()
    w.close()

    assert len(rows) == 1, f"ожидался 1 артефакт родителя, получено {len(rows)}"
    pid, kind, ref, sha = rows[0]
    assert str(pid) == str(t1)
    assert kind == "code.investigate", "kind родителя должен быть прокинут"
    assert ref == "sha256:deadbeef", "artifact_ref должен идти целиком"
    assert sha == "deadbeef", "artifact_sha256 — hex без префикса sha256:"


def test_iteration3_requires_live_lease(admin):
    """Без живой аренды (эпоха не совпала) — LS001, а не «пустой результат»."""
    t1, t2 = str(uuid.uuid4()), str(uuid.uuid4())
    cur = admin.cursor()
    _mk_worker(cur, "w1")
    _mk_task(cur, t1, "done", "code.investigate")
    _mk_artifact(cur, t1, "sha256:deadbeef")
    _mk_task(cur, t2, "leased", "code.feature", lease_owner="w1", epoch=3)
    cur.execute("insert into task_deps (task_id, dep_id) values (%s, %s)", (t2, t1))

    w = connect("swarm_worker_w1", os.environ.get("W4_W1_PW", ""))
    code = _sqlstate(w, "select * from task_parent_artifacts(%s, 'w1', 2)", (t2,))
    w.close()
    assert code == "LS001", f"чужая/старая эпоха должна давать LS001, получено {code!r}"


def test_iteration3_requires_worker_identity(admin):
    """w1 по чужому имени w2 — WK003 (identity), а не чтение данных."""
    t2 = str(uuid.uuid4())
    cur = admin.cursor()
    _mk_task(cur, t2, "leased", "code.feature", lease_owner="w2", epoch=1)

    w = connect("swarm_worker_w1", os.environ.get("W4_W1_PW", ""))
    code = _sqlstate(w, "select * from task_parent_artifacts(%s, 'w2', 1)", (t2,))
    w.close()
    assert code == "WK003", f"подмена воркера должна давать WK003, получено {code!r}"


def test_iteration3_empty_for_dependency_without_artifacts(admin):
    """Зависимость без артефакта не ломает вызов: просто нет строк."""
    t1, t2 = str(uuid.uuid4()), str(uuid.uuid4())
    cur = admin.cursor()
    _mk_worker(cur, "w1")
    _mk_task(cur, t1, "ready", "code.investigate")
    _mk_task(cur, t2, "leased", "code.feature", lease_owner="w1", epoch=1)
    cur.execute("insert into task_deps (task_id, dep_id) values (%s, %s)", (t2, t1))

    w = connect("swarm_worker_w1", os.environ.get("W4_W1_PW", ""))
    cur2 = w.cursor()
    cur2.execute("select * from task_parent_artifacts(%s, 'w1', 1)", (t2,))
    rows = cur2.fetchall()
    w.close()
    assert rows == [], "без артефактов у зависимости результат пуст, не ошибка"


def test_iteration3_legacy_contract_kind_fallback(admin):
    """Legacy-контракт без kind → 'task' (coalesce), а не NULL."""
    t1, t2 = str(uuid.uuid4()), str(uuid.uuid4())
    cur = admin.cursor()
    _mk_worker(cur, "w1")
    cur.execute(
        "insert into tasks (id, root_id, state, contract, budget_cap) "
        "values (%s, %s, 'done', %s, 100000)",
        (t1, t1, json.dumps({"description": "legacy без kind"}, ensure_ascii=False)),
    )
    _mk_artifact(cur, t1, "sha256:cafe")
    _mk_task(cur, t2, "leased", "code.feature", lease_owner="w1", epoch=1)
    cur.execute("insert into task_deps (task_id, dep_id) values (%s, %s)", (t2, t1))

    w = connect("swarm_worker_w1", os.environ.get("W4_W1_PW", ""))
    cur2 = w.cursor()
    cur2.execute("select parent_kind from task_parent_artifacts(%s, 'w1', 1)", (t2,))
    kind = cur2.fetchone()[0]
    w.close()
    assert kind == "task", f"legacy-контракт должен давать 'task', получено {kind!r}"
