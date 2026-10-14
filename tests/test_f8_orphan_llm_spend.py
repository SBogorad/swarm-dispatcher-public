"""F8 — бюджет обязан ограничивать РЕАЛЬНЫЕ расходы, а не только `complete`.

Дефект (аудит v0.4, docs/audit/AUDIT_REPORT_swarm_dispatcher_v0_4.md:121;
docs/KNOWN_LIMITATIONS.md §5.10 — F8):
  `spent_tokens` рос ТОЛЬКО в `complete_task`. Если воркер получил ответ LLM
  (деньги потрачены, строка в `llm_calls` записана), но упал/потерял аренду ДО
  `complete`, расход:
    * учтён в `llm_calls` (cost ledger — Issue #2),
    * НЕ учтён в `tasks.spent_tokens` (бюджет задачи).
  Значит `budget_cap` не ограничивал РЕАЛЬНЫЙ расход: цикл «взяли задачу,
  оплатили вызов, потеряли аренду, `reclaim_expired` вернул в ready» можно
  крутить, пока `attempts < budget.attempts`, и каждый оборот тратит деньги
  мимо бюджета.

Этот файл — АРБИТР ИНВАРИАНТА ЗАЩИТЫ БЮДЖЕТА (не описание дефекта). Он
КРАСНЕЕТ на логике без фикса и ЗЕЛЕНЕЕТ на фиксе (проверено `git stash` тела
фикса — см. отчёт задачи). Проверяемые инварианты:

  I1. Синхронный учёт: вставка в `llm_calls` немедленно (в той же транзакции)
      увеличивает `tasks.spent_tokens`. Оборванная до `complete` аренда уже
      видна бюджету.
  I2. Единый источник истины: `spent_tokens` не расходится с суммой ledger
      (нет двойного счёта при `complete_task`).
  I3. Реклейм: если воркер оплатил вызов и `spent_tokens >= budget_cap`,
      `reclaim_expired` переводит задачу сразу в `budget_exhausted`, а не в
      `ready` — повторный оборот невозможен.
  I4. Ограниченность: цикл оборванных аренд не копит расход безгранично —
      бюджет рано или поздно останавливает выдачу.
  I5. Двойной гейт по дереву: `root_budget_cap` на корне останавливает выдачу
      задач дерева, даже когда у конкретной задачи локальный бюджет цел.

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

def _has_root_cap_column(c) -> bool:
    """Есть ли в схеме колонка root_budget_cap (F8-фикс применён?).

    Нужно, чтобы этот же файл-арбитр можно было прогнать против СТАРОЙ схемы
    (без фикса) и увидеть красноту по инварианту бюджета, а не ошибку вызова
    несуществующей сигнатуры.
    """
    return _one(
        c,
        "select count(*) from information_schema.columns "
        "where table_name = 'tasks' and column_name = 'root_budget_cap'",
    ) == 1

def _new_task(c, cap=1000, root=None, root_cap=None):
    tid = str(uuid.uuid4())
    cur = c.cursor()
    if _has_root_cap_column(c):
        cur.execute(
            "select add_task(%s, %s, %s::jsonb, %s, %s, %s)",
            (tid, root, CONTRACT, cap, 0, root_cap),
        )
    else:
        # Старая схема: add_task без root-бюджета. Тесты I1–I4 не нуждаются в нём.
        cur.execute(
            "select add_task(%s, %s, %s::jsonb, %s, %s)",
            (tid, root, CONTRACT, cap, 0),
        )
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

def _claim(c, wid="w1"):
    cur = c.cursor()
    cur.execute("select out_task_id, out_epoch from claim_task(%s, 1)", (wid,))
    row = cur.fetchone()
    c.commit()
    return row

def _record_llm_call(c, task_id, worker_id, epoch, tokens_in=1000, tokens_out=500, cost=0.001):
    """Прямая запись в ledger — имитирует уже случившийся ответ LLM (Issue #2).

    Расход в ledger — это ФАКТ (деньги потрачены). Инвариант F8: этот же факт
    обязан немедленно отразиться в tasks.spent_tokens (триггер синхронизации).
    """
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

def _expire(c, tid):
    c.cursor().execute(
        "update tasks set lease_until = now() - interval '1 second' where id = %s", (tid,)
    )
    c.commit()

def _ledger_tokens(c, tid):
    return _one(
        c,
        "select coalesce(sum(prompt_tokens + completion_tokens), 0) from llm_calls where task_id = %s",
        tid,
    )

# --------------------------------------------------------------------------- I1
def test_f8_orphan_spend_is_visible_to_budget(db):
    """Оборванная до complete аренда: расход учтён бюджетом СРАЗУ (не 0)."""
    _register_worker(db, "w1")
    tid = _new_task(db, cap=100_000)
    _promote_ready(db)

    got_tid, epoch = _claim(db, "w1")
    assert str(got_tid) == tid

    # LLM ответил — расход записан в ledger (воркер успел до падения).
    _record_llm_call(db, tid, "w1", epoch, tokens_in=1000, tokens_out=500)

    # И1: синхронный учёт — spent_tokens уже видит оборванный расход.
    spent = _one(db, "select spent_tokens from tasks where id = %s", (tid,))
    assert spent == 1500, (
        f"spent_tokens обязан учесть оборванный вызов сразу (ожидалось 1500, "
        f"получено {spent}) — иначе budget_cap не ограничивает реальный расход (F8)"
    )

    # Воркер упал: НЕ вызывает complete. Аренда истекает, сторож возвращает задачу.
    _expire(db, tid)
    reclaimed = _one(db, "select reclaim_expired()")
    assert reclaimed == 1, "сторож должен вернуть просроченную аренду"

    ledger_tokens = _ledger_tokens(db, tid)
    spent = _one(db, "select spent_tokens from tasks where id = %s", (tid,))

    # И2: единый источник истины — бюджет не расходится с ledger.
    assert ledger_tokens == 1500, "ledger содержит потраченные токены"
    assert spent == ledger_tokens, (
        f"spent_tokens ({spent}) обязан совпадать с фактическим расходом из "
        f"ledger ({ledger_tokens}) — расхождение и есть F8"
    )

# --------------------------------------------------------------------------- I2
def test_f8_detector_view_reports_no_divergence(db):
    """Инструмент детекции: view llm_costs_by_task не показывает расхождения."""
    _register_worker(db, "w1")
    tid = _new_task(db, cap=100_000)
    _promote_ready(db)
    _, epoch = _claim(db, "w1")
    _record_llm_call(db, tid, "w1", epoch, tokens_in=2000, tokens_out=0)

    _expire(db, tid)
    db.cursor().execute("select reclaim_expired()")
    db.commit()

    row = db.cursor()
    row.execute(
        "select llm_calls, total_tokens, spent_tokens from llm_costs_by_task where task_id = %s",
        (tid,),
    )
    calls, total_tokens, spent = row.fetchone()
    assert calls == 1, "ledger видит один вызов"
    assert total_tokens == 2000, "ledger видит потраченные токены"
    assert spent == total_tokens, (
        f"бюджет задачи обязан видеть тот же расход, что ledger "
        f"(spent={spent}, ledger={total_tokens})"
    )

# --------------------------------------------------------------------------- I3
def test_f8_reclaim_after_paid_call_goes_budget_exhausted(db):
    """Воркер оплатил вызов и упал при исчерпанном бюджете → budget_exhausted."""
    _register_worker(db, "w1")
    # cap=1000; один оборванный вызов тратит 1500 > cap.
    tid = _new_task(db, cap=1000)
    _promote_ready(db)
    _, epoch = _claim(db, "w1")
    _record_llm_call(db, tid, "w1", epoch, tokens_in=1000, tokens_out=500)

    _expire(db, tid)
    reclaimed = _one(db, "select reclaim_expired()")
    assert reclaimed == 1

    state = _one(db, "select state from tasks where id = %s", (tid,))
    assert state == "budget_exhausted", (
        f"оплаченная и оборванная аренда при исчерпанном бюджете обязана уйти в "
        f"budget_exhausted, а не в ready (получено {state!r}) — иначе цикл "
        f"повторных оборотов тратит деньги мимо бюджета (F8)"
    )

    # И3: повторная выдача невозможна.
    assert _claim(db, "w1") is None, "исчерпанная по бюджету задача не выдаётся"

# --------------------------------------------------------------------------- I4
def test_f8_repeated_orphans_are_bounded_by_budget(db):
    """Цикл оборванных аренд ограничен бюджетом, а не только attempts."""
    _register_worker(db, "w1")
    cap = 2500
    tid = _new_task(db, cap=cap)
    _promote_ready(db)

    rounds = 0
    for _ in range(10):
        row = _claim(db, "w1")
        if row is None:
            break
        _, epoch = row
        _record_llm_call(db, tid, "w1", epoch, tokens_in=1000, tokens_out=0)
        _expire(db, tid)
        db.cursor().execute("select reclaim_expired()")
        db.commit()
        rounds += 1
        if _one(db, "select state from tasks where id = %s", (tid,)) == "budget_exhausted":
            break

    spent = _one(db, "select spent_tokens from tasks where id = %s", (tid,))
    state = _one(db, "select state from tasks where id = %s", (tid,))
    ledger_tokens = _ledger_tokens(db, tid)

    assert state == "budget_exhausted", (
        f"цикл оборванных аренд обязан упереться в budget_exhausted, получено "
        f"{state!r} (rounds={rounds}) — бюджет не ограничивает реальный расход (F8)"
    )
    # Расход ограничен бюджетом с точностью до одного вызова (последний вызов,
    # перешагнувший cap, оплачен — но после него выдача прекращена).
    assert spent <= cap + 1000, f"расход не ограничен бюджетом: spent={spent}, cap={cap}"
    assert spent == ledger_tokens, "бюджет и ledger не должны расходиться"
    assert _claim(db, "w1") is None, "после исчерпания бюджета задача не выдаётся"

# --------------------------------------------------------------------------- I5
def test_f8_root_budget_cap_stops_tree(db):
    """root_budget_cap на корне останавливает выдачу задач дерева."""
    _register_worker(db, "w1")
    root = _new_task(db, cap=100_000, root_cap=2000)
    child = _new_task(db, cap=100_000, root=root)
    _promote_ready(db)

    # Потомок наследует root_budget_cap корня (денормализация триггером).
    child_rbc = _one(db, "select root_budget_cap from tasks where id = %s", (child,))
    assert child_rbc == 2000, "потомок обязан унаследовать root_budget_cap корня"

    # Тратим бюджет дерева на корне: 2500 > 2000.
    _, epoch = _claim(db, "w1")
    _record_llm_call(db, root, "w1", epoch, tokens_in=2500, tokens_out=0)

    tree_spent = _one(
        db, "select coalesce(sum(spent_tokens), 0) from tasks where root_id = %s", (root,)
    )
    assert tree_spent >= 2000, "суммарный расход дерева превысил root_budget_cap"

    # И5: локальный бюджет потомка цел (0 < 100000), но дерево исчерпано —
    # выдача обязана остановиться.
    claimed = []
    while True:
        row = _claim(db, "w1")
        if row is None:
            break
        claimed.append(str(row[0]))
    assert child not in claimed, (
        f"задача дерева с исчерпанным root_budget_cap не должна выдаваться "
        f"(выданы: {claimed}) — гейт по дереву не работает (F8)"
    )
