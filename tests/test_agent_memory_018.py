#!/usr/bin/env python3
"""SQL-дедупликация памяти роя — миграция 018.

Дефект (слепое пятно PR #45):
    `memory_section` в `crates/swarm-worker/src/prompt.rs` дедуплицирует уроки
    по смысловому отпечатку, но ПОСЛЕ выборки. Сама выборка живёт в SQL:

        order by m.created_at desc, m.id desc
        limit 5;

    Если пять свежих уроков описывают одну причину отказа (разные задачи,
    разные формулировки), SQL отдаёт все пять, Rust схлопывает их в один, и
    воркер получает 1 урок вместо 5. Уникальные уроки, лежащие глубже,
    не доходят никогда.

Почему дубли НЕ побайтовые:
    в 016 стоит `uq_agent_memory_task_outcome (source_task_id, outcome)`,
    поэтому две строки одной задачи с одним исходом невозможны. Дубли
    возникают по смыслу — разные задачи, одна причина разными словами.

Красный на коде без миграции (42883 на отсутствующей
`agent_memory_fingerprint`), зелёный после 018. Не `skip`.

Соединение:
  W4_ADMIN_DSN — DSN суперпользователя (postgres) на выделенной c2_-БД.

Запуск:
  W4_ADMIN_DSN="host=127.0.0.1 port=... user=postgres password=... dbname=c2_swarm" \
  .venv/bin/python -m pytest -q tests/test_agent_memory_018.py
"""
import json
import os
import re
import uuid

import psycopg2
import pytest

MIGRATION = "schema/migrations/018_recall_memory_dedup.sql"
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


def _mk_lesson(cur, source_task_id, lesson, kind="code.feature", age_hours=0):
    """Урок прошлой задачи. `age_hours` > 0 сдвигает его в прошлое."""
    cur.execute(
        "insert into agent_memory (kind, source_task_id, rework_count, lesson, created_at) "
        "values (%s, %s, %s, %s, now() - make_interval(hours => %s))",
        (kind, source_task_id, 1, lesson, age_hours),
    )


def _past_task(cur, kind="code.feature"):
    """Задача-источник урока: своя задача исключается из выдачи."""
    tid = str(uuid.uuid4())
    _mk_task(cur, tid, kind, state="done")
    return tid


# ---------- статический якорь ----------


def test_migration_file_exists():
    path = os.path.join(ROOT, MIGRATION)
    assert os.path.isfile(path), f"нет файла миграции: {MIGRATION}"
    body = open(path, encoding="utf-8").read().lower()
    # Вспомогательная функция отпечатка и пересоздание шлюза.
    assert "agent_memory_fingerprint" in body
    assert "create or replace function public.task_recall_memory" in body
    # Защита обязана быть воспроизведена дословно: create-or-replace тихо
    # снимает её, если строку забыть (класс 011/012).
    assert "assert_worker_identity" in body
    assert "assert_lease" in body
    assert "m.source_task_id <> p_task" in body
    # Дедупликация ДО ограничения — предмет миграции.
    assert "distinct on" in body
    assert "limit 25" in body
    # Права не должны съехать.
    assert "grant execute on function public.task_recall_memory" in body
    assert "to swarm_worker" in body
    # Инвариант каталога (step2/test_roles.py::test_catalog_invariants):
    # КАЖДАЯ функция в public обязана быть SECURITY DEFINER с закреплённым
    # search_path. Первый прогон гейта покраснел именно на этом.
    assert "security definer" in body
    assert "set search_path = public, pg_temp" in body


def test_fingerprint_is_immutable(admin):
    """`distinct on` и планировщик опираются на режим immutable."""
    cur = admin.cursor()
    cur.execute(
        "select p.provolatile from pg_proc p "
        "where p.proname = 'agent_memory_fingerprint'"
    )
    row = cur.fetchone()
    assert row, "нет функции agent_memory_fingerprint (миграция не применена?)"
    assert row[0] == "i", f"agent_memory_fingerprint не immutable: {row[0]}"


def test_fingerprint_satisfies_catalog_invariant(admin):
    """Функция в `public` обязана быть SECURITY DEFINER с закреплённым search_path.

    Это требование `step2/test_roles.py::test_catalog_invariants`: обычная
    invoker-функция в `public` роняет гейт (проверено на первом прогоне).
    """
    cur = admin.cursor()
    cur.execute(
        "select p.prosecdef, p.proconfig, r.rolsuper "
        "  from pg_proc p join pg_roles r on r.oid = p.proowner "
        " where p.proname = 'agent_memory_fingerprint'"
    )
    row = cur.fetchone()
    assert row, "нет функции agent_memory_fingerprint"
    secdef, cfg, is_super = row
    assert secdef, "agent_memory_fingerprint обязана быть SECURITY DEFINER"
    assert cfg and "search_path=public, pg_temp" in cfg, f"search_path не закреплён: {cfg}"
    assert not is_super, "владелец функции — суперпользователь"


# ---------- отпечаток ----------


def test_fingerprint_ignores_case_and_spacing(admin):
    """Регистр и лишние пробелы не делают урок другим — включая кириллицу."""
    cur = admin.cursor()
    cur.execute(
        "select public.agent_memory_fingerprint(%s) = public.agent_memory_fingerprint(%s)",
        (
            "Виток 4: Патч не применён. Причина: patch failed",
            "виток 4:   патч  не применён.   причина: patch failed",
        ),
    )
    assert cur.fetchone()[0] is True


def test_fingerprint_ignores_reason_tail(admin):
    """Хвост причины за пределами 80 символов не различает уроки.

    Именно там расходятся формулировки одного сбоя: номера строк, имена файлов.
    """
    cur = admin.cursor()
    head = "Виток 4: патч роя не применяется. Причина: "
    long = (
        "ханк объявляет old=6 new=22, фактически old=8 new=28, "
        "файл crates/swarm-worker/src/"
    )
    assert len(long) > 80, "хвост должен выходить за 80 символов"
    cur.execute(
        "select public.agent_memory_fingerprint(%s) = public.agent_memory_fingerprint(%s)",
        (head + long + "model_selector.rs", head + long + "worker.rs"),
    )
    assert cur.fetchone()[0] is True


def test_fingerprint_keeps_distinct_reasons(admin):
    """Разные причины одного вида работ остаются разными уроками."""
    cur = admin.cursor()
    cur.execute(
        "select public.agent_memory_fingerprint(%s) = public.agent_memory_fingerprint(%s)",
        (
            "Виток 4: тест красный. Причина: connection refused",
            "Виток 4: тест красный. Причина: assertion failed",
        ),
    )
    assert cur.fetchone()[0] is False


# ---------- главный кейс ----------


def test_five_duplicates_do_not_hide_unique_lesson(admin):
    """Пять свежих дублей одной причины не заслоняют шестой уникальный урок.

    До 018: SQL отдаёт пять дублей, Rust схлопывает их в один — воркер
    получает 1 урок, уникальный не доходит никогда.
    После 018: два смысла, дубль представлен одной строкой.
    """
    cur = admin.cursor()
    t2 = str(uuid.uuid4())
    _mk_task(cur, t2, "code.feature", state="leased", lease_owner="w1")

    # Пять разных задач приносят ОДНУ причину: расходятся регистр, пробелы и
    # хвост после `причина:` за пределами 80 символов. Это и есть смысловые
    # дубли: побайтово строки разные, `unique (source_task_id, outcome)` их не
    # ловит, потому что задачи разные.
    head = "Виток 4: патч роя не применяется. Причина: "
    tail = "ханк объявляет old=6 new=22, фактически old=8 new=28, файл crates/swarm-worker/src/"
    assert len(tail) > 80, "хвост должен выходить за 80 символов"
    variants = [
        head + tail + "model_selector.rs",
        head + tail + "worker.rs",
        head.upper() + tail + "prompt.rs",
        "Виток 4:   патч роя   не применяется.   Причина: " + tail + "lib.rs",
        "виток 4: патч роя не применяется. причина: " + tail + "dispatcher.rs",
    ]
    for i, variant in enumerate(variants):
        _mk_lesson(cur, _past_task(cur), variant, age_hours=i)

    # Уникальный урок лежит ГЛУБЖЕ — старше всех дублей.
    _mk_lesson(
        cur,
        _past_task(cur),
        "Заполняй Decision Trail до сдачи",
        age_hours=100,
    )

    cur.execute("select lesson from task_recall_memory(%s, %s, %s)", (t2, "w1", 1))
    recalled = [r[0] for r in cur.fetchall()]

    assert len(recalled) == 2, f"ожидалось два смысла, получено {len(recalled)}: {recalled}"
    assert any("Decision Trail" in r for r in recalled), (
        f"уникальный урок не дошёл до воркера: {recalled}"
    )
    assert sum("не применяется" in r.lower() for r in recalled) == 1, (
        f"дубль представлен более чем одной строкой: {recalled}"
    )


def test_most_recent_lesson_wins_its_group(admin):
    """Из группы выживает самый свежий урок, и он же первый в выдаче."""
    cur = admin.cursor()
    t2 = str(uuid.uuid4())
    _mk_task(cur, t2, "code.feature", state="leased", lease_owner="w1")

    # Одна и та же причина: расходятся только регистр и пробелы, то есть
    # ровно то, что отпечаток обязан игнорировать. Текст ДО `Причина:` должен
    # совпадать — иначе это честно разные уроки, и схлопывать их нельзя.
    older = "виток 4: патч роя не применён. причина: patch failed"
    fresher = "Виток 4: Патч роя не применён. Причина: patch failed"
    _mk_lesson(cur, _past_task(cur), older, age_hours=50)
    _mk_lesson(cur, _past_task(cur), fresher, age_hours=1)

    cur.execute("select lesson from task_recall_memory(%s, %s, %s)", (t2, "w1", 1))
    recalled = [r[0] for r in cur.fetchall()]
    assert len(recalled) == 1, f"группа не схлопнулась: {recalled}"
    assert recalled[0] == fresher, f"выжил не самый свежий урок: {recalled}"


def test_recall_excludes_own_task(admin):
    """Своя задача исключена: её урок ещё не выучен."""
    cur = admin.cursor()
    t2 = str(uuid.uuid4())
    _mk_task(cur, t2, "code.feature", state="leased", lease_owner="w1")
    cur.execute(
        "insert into agent_memory (kind, source_task_id, rework_count, lesson) "
        "values (%s, %s, %s, %s)",
        ("code.feature", t2, 1, "Свой собственный урок не должен вернуться"),
    )
    cur.execute("select lesson from task_recall_memory(%s, %s, %s)", (t2, "w1", 1))
    assert cur.fetchall() == []


def test_recall_requires_live_lease(admin):
    """Без живой аренды шлюз обязан отказать (LS001), а не отдать память роя.

    Мутация S15 снимает `assert_lease` — этот кейс обязан покраснеть.
    """
    cur = admin.cursor()
    t1 = str(uuid.uuid4())
    _mk_task(cur, t1, "code.feature")  # state=ready, аренды нет
    state = _sqlstate(admin, "select * from task_recall_memory(%s, %s, %s)", (t1, "w1", 1))
    assert state == "LS001", f"ожидался lease_lost, получено {state}"


def test_recall_returns_at_most_five_distinct(admin):
    """Потолок 5 сохраняется, но теперь на РАЗНЫХ смыслах."""
    cur = admin.cursor()
    t2 = str(uuid.uuid4())
    _mk_task(cur, t2, "code.feature", state="leased", lease_owner="w1")

    # Восемь уникальных смыслов: выдано должно быть ровно 5.
    for i in range(8):
        _mk_lesson(cur, _past_task(cur), f"Уникальный урок номер {i} про причину {i}", age_hours=i)

    cur.execute("select lesson from task_recall_memory(%s, %s, %s)", (t2, "w1", 1))
    recalled = [r[0] for r in cur.fetchall()]
    assert len(recalled) == 5, f"ожидалось 5 уроков, получено {len(recalled)}"
    assert len(set(recalled)) == 5, f"в выдаче есть дубли: {recalled}"
