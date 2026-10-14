"""F3 — identification closed by default.

Defect (pre-fix): assert_worker_identity only guarded when
`session_user LIKE 'swarm_worker_%'`. The shared role `swarm_worker`
does NOT match that pattern, yet it is the one role granted the wrappers
(register_worker / claim_task). So under `swarm_worker` the identity
check is a silent no-op — defense rested solely on `nologin`.

Fix (reference_fix_wave4.sql): deny-by-default. Bypass only when
session_user = 'swarm_worker_'||normalize_worker_id(p_worker), or the
session is a superuser. Everything else -> WK003.

assert_worker_identity is intentionally NOT granted to any role; it is
observed only through wrappers. The positive control therefore drives a
wrapper (register_worker), not the assert directly.

Connection contract matches the other wave-4 arbiters:
  W4_ADMIN_DSN   full DSN for the admin (postgres) connection
  W4_W1_PW       password of swarm_worker_w1
  W4_W2_PW       password of swarm_worker_w2
Pre-condition gate (no PG => BLOCKED) lives in tests/conftest.py.

Запуск:
  W4_ADMIN_DSN="host=127.0.0.1 port=... user=postgres password=... dbname=c2_swarm" \
  W4_W1_PW=... W4_W2_PW=... \
  .venv/bin/python -m pytest -q tests/test_worker_identity_f3.py
"""
import os
import re

import psycopg2
import pytest

MISMATCH = "WK003"


def _dsn() -> str:
    return os.environ.get("W4_ADMIN_DSN", "")


def _parts():
    """Parse key=value DSN into a dict."""
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


# ---------- structural anchor: deny-by-default body ----------

def _strip_sql_comments(body: str) -> str:
    """Убирает строковые комментарии (-- ... до конца строки), чтобы
    структурный анкер не ловил упоминание паттерна в пояснении."""
    out = []
    for line in body.splitlines():
        out.append(line.split("--", 1)[0])
    return "\n".join(out)


def test_f3_body_is_deny_by_default(admin):
    """The assert body must not be gated on LIKE 'swarm_worker_%'.

    Pre-fix the whole check sits inside
    `if session_user LIKE 'swarm_worker_%'`, which is false for the shared
    role swarm_worker — the guard is skipped entirely (bypass).

    Комментарии (-- ...) исключаем: F3-объяснение может упоминать старый
    LIKE-гейт как историю, и это не должно считаться гейтом в коде.
    """
    cur = admin.cursor()
    cur.execute("select prosrc from pg_proc where proname = 'assert_worker_identity'")
    body = cur.fetchone()[0]
    low = _strip_sql_comments(body).lower()
    assert "like 'swarm_worker_%'" not in low, (
        "F3: assert_worker_identity is still gated on LIKE 'swarm_worker_%' — "
        "under the shared role the check is a no-op (bypass)"
    )
    # deny-by-default must raise WK003 and must consult rolsuper for the bypass
    assert "wk003" in low or "worker_identity_mismatch" in low, "F3: assert must raise WK003"
    assert "rolsuper" in low, "F3: the only bypass must be an explicit rolsuper lookup"


# ---------- behavioural ----------

def test_f3_superuser_bypass(admin):
    """Superuser session may pass any p_worker (explicit rolsuper bypass)."""
    assert _sqlstate(admin, "select assert_worker_identity('w1')") is None, (
        "F3: superuser must bypass the check"
    )


def test_f3_owner_not_whitelisted(admin):
    """The object owner (swarm_owner) must not be whitelisted by name."""
    cur = admin.cursor()
    cur.execute("select prosrc from pg_proc where proname = 'assert_worker_identity'")
    body = cur.fetchone()[0].lower()
    assert "swarm_owner" not in body, "F3: assert must not whitelist swarm_owner by name"


# ---------- positive control: legit worker still works, via wrapper ----------

def test_f3_legit_worker_wrapper_passes(admin):
    """swarm_worker_w1 registering itself succeeds (identity matches)."""
    w = connect("swarm_worker_w1", os.environ.get("W4_W1_PW", ""))
    code = _sqlstate(w, "select register_worker('w1', '{\"ram_gb\":16}'::jsonb)")
    assert code is None, f"F3: legit w1/register_worker('w1') must succeed, got {code!r}"
    w.close()


def test_f3_wrong_identity_via_wrapper_rejected(admin):
    """swarm_worker_w1 registering as 'w2' must be WK003 (identity mismatch)."""
    w = connect("swarm_worker_w1", os.environ.get("W4_W1_PW", ""))
    code = _sqlstate(w, "select register_worker('w2', '{\"ram_gb\":16}'::jsonb)")
    assert code == MISMATCH, f"F3: w1 as 'w2' must be WK003, got {code!r}"
    w.close()

# ---------- FIND-NEW-01: identity in fail_task / fail_task_state ----------
#
# Регрессия (второй проход аудита, снимок bc0c168): roles_2b.sql перекрывал
# identity-проверкой только 5 функций воркера + record_tool_call. fail_task и
# fail_task_state шли под SECURITY DEFINER БЕЗ `assert_worker_identity(p_worker)`,
# хотя GRANT EXECUTE был выдан всем per-worker ролям. Следствие: swarm_worker_w1,
# зная (task_id, epoch) задачи w2, вызывал fail_task('<task_w2>','w2',epoch_w2,...)
# → сбрасывал чужую живую аренду (задача → blocked) и писал в хэш-цепочку events
# запись с actor='w2' (подделка журнала).
#
# Анкер: identity-проверка обязана стоять ПЕРВОЙ содержательной строкой тела,
# ДО assert_lease, поэтому чужой воркер получает именно WK003.

import uuid as _uuid

def _mk_leased_task(admin, worker="w2", epoch=1):
    """Задача в состоянии leased, арендованная `worker` с живым сроком."""
    t = str(_uuid.uuid4())
    admin.cursor().execute(
        """insert into tasks(id, root_id, state, contract, priority, attempts,
                             budget_cap, spent_tokens, lease_epoch,
                             lease_owner, lease_until)
           values (%s,%s,'leased',%s::jsonb,0,1,1000,0,%s,%s,now()+interval '5 min')""",
        (t, t, '{"kind":"k","checks":[{"type":"cmd"}]}', epoch, worker),
    )
    return t

def test_f3_fail_task_foreign_worker_rejected(admin):
    """swarm_worker_w1 calling fail_task for w2's lease must be WK003.

    Без identity-проверки вызов доходил до assert_lease, где lease_owner='w2'
    совпадал с переданным p_worker='w2' → LS001 не срабатывал, и чужую аренду
    можно было сбросить. С проверкой — WK003 ещё до assert_lease.
    """
    t = _mk_leased_task(admin, worker="w2", epoch=1)
    w = connect("swarm_worker_w1", os.environ.get("W4_W1_PW", ""))
    code = _sqlstate(w, "select fail_task(%s,'w2',1,null,'sabotage')", (t,))
    assert code == MISMATCH, (
        f"F3/FIND-NEW-01: w1 calling fail_task for w2 must be WK003, got {code!r}"
    )
    w.close()

    # Задача w2 не тронута: чужая аренда не сброшена.
    cur = admin.cursor()
    cur.execute("select state, lease_owner from tasks where id = %s", (t,))
    state, owner = cur.fetchone()
    assert (state, owner) == ("leased", "w2"), (
        f"F3/FIND-NEW-01: чужую аренду сбросили (state={state!r}, owner={owner!r})"
    )

def test_f3_fail_task_state_foreign_worker_rejected(admin):
    """swarm_worker_w1 calling fail_task_state for w2's lease must be WK003."""
    t = _mk_leased_task(admin, worker="w2", epoch=1)
    w = connect("swarm_worker_w1", os.environ.get("W4_W1_PW", ""))
    code = _sqlstate(
        w, "select fail_task_state(%s,'w2',1,null,'sabotage','blocked')", (t,)
    )
    assert code == MISMATCH, (
        f"F3/FIND-NEW-01: w1 calling fail_task_state for w2 must be WK003, got {code!r}"
    )
    w.close()

    cur = admin.cursor()
    cur.execute("select state, lease_owner from tasks where id = %s", (t,))
    state, owner = cur.fetchone()
    assert (state, owner) == ("leased", "w2"), (
        f"F3/FIND-NEW-01: чужую аренду сбросили (state={state!r}, owner={owner!r})"
    )

def test_f3_fail_task_own_identity_passes(admin):
    """swarm_worker_w1 failing ITS OWN lease must pass identity (LS001/OK, not WK003).

    Положительный контроль: без него мутация «убрать всю проверку целиком»
    осталась бы незамеченной, а мы бы не отличили «identity работает» от
    «вызов всегда падает по другой причине».
    """
    t = _mk_leased_task(admin, worker="w1", epoch=1)
    w = connect("swarm_worker_w1", os.environ.get("W4_W1_PW", ""))
    code = _sqlstate(w, "select fail_task(%s,'w1',1,null,'boom')", (t,))
    assert code is None, (
        f"F3/FIND-NEW-01: w1 failing its own lease must succeed, got {code!r}"
    )
    w.close()

    cur = admin.cursor()
    cur.execute("select state from tasks where id = %s", (t,))
    assert cur.fetchone()[0] == "blocked"
