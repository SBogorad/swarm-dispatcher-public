"""F8 — расход, оборвавшийся до `complete`, не ограничен бюджетом задачи.

Дефект (аудит v0.4, docs/audit/AUDIT_REPORT_swarm_dispatcher_v0_4.md:121;
docs/KNOWN_LIMITATIONS.md §5.10 — ОТКРЫТО):
  `spent_tokens` растёт ТОЛЬКО в `complete_task` (schema_v0.sql:393). Если
  воркер получил ответ LLM (деньги потрачены, строка в `llm_calls` записана),
  но упал/потерял аренду ДО `complete`, расход:
    * учтён в `llm_calls` (cost ledger — Issue #2),
    * НЕ учтён в `tasks.spent_tokens` (бюджет задачи).
  Значит `budget_cap` задачи не ограничивает РЕАЛЬНЫЙ расход: цикл «взяли
  задачу, оплатили вызов, потеряли аренду, `reclaim_expired` вернул в ready»
  можно крутить, пока `attempts < budget.attempts`, и каждый оборот тратит
  деньги мимо бюджета.

Что проверяет этот файл (арбитр — запуск, а не мнение):
  1. Воспроизводит дефект на живой схеме: после оборванной аренды
     `llm_calls` содержит расход, а `spent_tokens` = 0 → расхождение
     `spent_tokens` vs фактический расход видно и измеримо.
  2. Доказывает, что инструмент ДЕТЕКТИРУЕТ дефект: view `llm_costs_by_task`
     показывает `total_tokens` (из ledger) при `spent_tokens` = 0.
  3. Проверяет, что дефект НЕ маскируется: сумма по ledger для задачи больше
     `spent_tokens` ровно на оборванный вызов.

Тест — на живой схеме (PGDSN суперпользователя), изолирован в disposable-БД
(c2_/v0/test_ префикс, как в tests/conftest.py). Гейт BLOCKED (нет PG) —
в conftest. Запуск:
  PGDSN="dbname=c2_swarm user=postgres host=127.0.0.1 port=NNNNN password=..." \
    .venv/bin/python -m pytest -q tests/test_f8_orphan_llm_spend.py
"""
from __future__ import annotations

import json
import os
import uuid

import psycopg2
import pytest

DSN = os.environ.get("PGDSN", "")


def _conn():
    c = psycopg2.connect(DSN)
    c.autocommit = False
    return c


def _db_is_disposable(dsn: str) -> bool:
    name = ""
    if dsn.startswith(("postgres://", "postgresql://")):
        name = dsn.split("/")[2].split("?")[0]
    else:
        for part in dsn.split():
            if part.startswith("dbname="):
                name = part.split("=", 1)[1]
    return name.startswith(("test_", "v0", "c2_"))


pytestmark = pytest.mark.skipif(
    not DSN or not _db_is_disposable(DSN),
    reason="F8 требует живой disposable-БД (PGDSN на c2_/v0/test_)",
)

CONTRACT = json.dumps(
    {"kind": "t", "budget": {"attempts": 3}, "checks": [{"type": "cmd", "run": "true"}]}
)


@pytest.fixture()
def db():
    c = _conn()
    cur = c.cursor()
    cur.execute(
        "truncate events, reviews, task_deps, artifacts, tasks, workers, llm_calls cascade"
    )
    c.commit()
    yield c
    c.rollback()
    c.close()


def _one(c, sql, *args):
    cur = c.cursor()
    cur.execute(sql, args)
    r = cur.fetchone()
    return r[0] if r and len(r) == 1 else r


def _new_task(c, cap=1000):
    tid = str(uuid.uuid4())
    cur = c.cursor()
    cur.execute("select add_task(%s, %s, %s::jsonb, %s, %s)", (tid, None, CONTRACT, cap, 0))
    c.commit()
    return tid


def _promote_ready(c):
    """add_task создаёт задачу в `pending`; в `ready` её переводит promote_ready().

    Без этого claim_task (выбирает только state='ready') не вернёт строку.
    Порядок как в проде: planner.add_task → scheduler.promote_ready.
    """
    cur = c.cursor()
    cur.execute("select promote_ready()")
    cur.fetchone()
    c.commit()


def _register_worker(c, wid="w1"):
    cur = c.cursor()
    cur.execute("select register_worker(%s, '{}'::jsonb, array[]::text[])", (wid,))
    c.commit()


def _record_llm_call(c, task_id, worker_id, epoch, tokens_in=1000, tokens_out=500, cost=0.001):
    """Прямая запись в ledger — имитирует уже случившийся ответ LLM (Issue #2)."""
    cur = c.cursor()
    cur.execute(
        "insert into llm_calls (task_id, worker_id, lease_epoch, model, provider, "
        "prompt_tokens, completion_tokens, cached_tokens, prompt_cost_usd, "
        "completion_cost_usd, total_cost_usd, started_at, completed_at, duration_ms, "
        "success, http_status) values (%s,%s,%s,'deepseek-v4-flash','relay.fast',"
        "%s,%s,0,%s,%s,%s, now(), now(), 100, true, 200)",
        (task_id, worker_id, epoch, tokens_in, tokens_out, cost, cost, cost * 2),
    )
    c.commit()


def test_f8_orphan_spend_invisible_to_budget(db):
    """Оборванная до complete аренда: деньги в ledger, но spent_tokens = 0."""
    _register_worker(db, "w1")
    tid = _new_task(db, cap=100_000)
    _promote_ready(db)

    # Воркер берёт задачу (аренда).
    row = db.cursor()
    row.execute("select out_task_id, out_epoch from claim_task('w1', 1)")
    got_tid, epoch = row.fetchone()
    assert str(got_tid) == tid
    db.commit()

    # LLM ответил — расход записан в ledger (воркер успел до падения).
    _record_llm_call(db, tid, "w1", epoch, tokens_in=1000, tokens_out=500)

    # Воркер упал: НЕ вызывает complete. Аренда истекает, сторож возвращает задачу.
    db.cursor().execute("update tasks set lease_until = now() - interval '1 second' where id = %s", (tid,))
    db.commit()
    reclaimed = _one(db, "select reclaim_expired()")
    assert reclaimed == 1, "сторож должен вернуть просроченную аренду"

    state = _one(db, "select state from tasks where id = %s", (tid,))
    spent = _one(db, "select spent_tokens from tasks where id = %s", (tid,))
    ledger_tokens = _one(
        db, "select coalesce(sum(prompt_tokens + completion_tokens), 0) from llm_calls where task_id = %s", (tid,)
    )

    # Дефект F8, зафиксирован как ФАКТ (не мнение):
    assert ledger_tokens == 1500, "ledger содержит потраченные токены"
    assert spent == 0, "spent_tokens НЕ отражает оборванный расход (дефект F8)"
    assert state == "ready", "задача вернулась в ready для повтора"


def test_f8_detector_view_surfaces_divergence(db):
    """Инструмент детекции: view llm_costs_by_task показывает расход при spent=0."""
    _register_worker(db, "w1")
    tid = _new_task(db, cap=100_000)
    _promote_ready(db)
    row = db.cursor()
    row.execute("select out_epoch from claim_task('w1', 1)")
    (epoch,) = row.fetchone()
    db.commit()
    _record_llm_call(db, tid, "w1", epoch, tokens_in=2000, tokens_out=0)

    # Оборвали аренду — расхода в бюджете нет.
    db.cursor().execute("update tasks set lease_until = now() - interval '1 second' where id = %s", (tid,))
    db.commit()
    db.cursor().execute("select reclaim_expired()")
    db.commit()

    row = db.cursor()
    row.execute(
        "select llm_calls, total_tokens, spent_tokens from llm_costs_by_task where task_id = %s", (tid,)
    )
    calls, total_tokens, spent = row.fetchone()
    assert calls == 1, "ledger видит один вызов"
    assert total_tokens == 2000, "ledger видит потраченные токены"
    assert spent == 0, "бюджет задачи их не видит — расхождение измеримо"


def test_f8_repeated_orphans_accumulate_unbounded(db):
    """Цикл оборванных аренд копит расход мимо бюджета (ядро F8)."""
    _register_worker(db, "w1")
    # budget.attempts=3 → максимум 3 оборота до blocked, каждый с оплатой.
    tid = _new_task(db, cap=100_000)
    _promote_ready(db)
    rounds = 0
    for _ in range(3):
        row = db.cursor()
        row.execute("select out_epoch from claim_task('w1', 1)")
        res = row.fetchone()
        db.commit()
        if res is None:
            break
        (epoch,) = res
        _record_llm_call(db, tid, "w1", epoch, tokens_in=1000, tokens_out=0)
        db.cursor().execute(
            "update tasks set lease_until = now() - interval '1 second' where id = %s", (tid,)
        )
        db.commit()
        db.cursor().execute("select reclaim_expired()")
        db.commit()
        rounds += 1

    ledger_tokens = _one(
        db, "select coalesce(sum(prompt_tokens + completion_tokens), 0) from llm_calls where task_id = %s", (tid,)
    )
    spent = _one(db, "select spent_tokens from tasks where id = %s", (tid,))
    state = _one(db, "select state from tasks where id = %s", (tid,))

    assert rounds >= 2, "дефект воспроизводится минимум дважды"
    assert ledger_tokens == rounds * 1000, "каждый оборот потратил токены"
    assert spent == 0, "бюджет по-прежнему ничего не видит"
    # Итог F8: реальные расходы накопились, а budget_cap задачи не сработал.
    assert ledger_tokens > spent, "факт расхода превышает учтённый бюджет"
