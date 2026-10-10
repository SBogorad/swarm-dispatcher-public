#!/usr/bin/env python3
"""Тест-арбитр волны 4 — F15: validate_contract должен требовать форму checks[].

Дефект: validate_contract проверял только непустоту checks (array), но НЕ форму
элементов. Принимались ["pytest",42], [{"run":"cargo test"}], [{"type":""}],
[{"type":123}] — задача без непустого строкового type затем вечно висит
(её нельзя одобрить: RV003).

Фикс (эталон reference_fix_wave4.sql, блок F15):
  каждый элемент checks — объект с непустым строковым type;
  id (если задан) — непустая строка.

ТРИАДА:
  RED  pre-fix : malformed checks принимаются (CT001 не летит) => падает
  GREEN fixed  : CT001 на каждый malformed; валидные (с id и без) проходят
  RED  after revert: снова принимаются => падает

Гейт BLOCKED (нет PG) — в tests/conftest.py.
Запуск:
  W4_ADMIN_DSN="host=... user=postgres password=... dbname=c2_swarm" \
  .venv/bin/python -m pytest -q tests/test_contract_form_f15.py
"""
import json
import os

import psycopg2
import pytest


def _dsn() -> str:
    return os.environ.get("W4_ADMIN_DSN", "")


@pytest.fixture()
def conn():
    c = psycopg2.connect(_dsn())
    c.autocommit = False
    yield c
    c.rollback()
    c.close()


def _contract(checks):
    return json.dumps(
        {"kind": "t", "checks": checks, "budget": {"attempts": 3, "max_rework": 3}}
    )


@pytest.mark.parametrize(
    "bad_checks",
    [
        ["pytest", 42],              # не объекты
        [{"run": "cargo test"}],     # нет type
        [{"type": ""}],              # пустой type
        [{"type": "   "}],           # пробельный type
        [{"type": 123}],             # type не строка
        [{"type": "cmd", "id": ""}],  # id задан, но пустой
        [{"type": "cmd", "id": 7}],   # id не строка
    ],
)
def test_f15_rejects_malformed_checks(conn, bad_checks):
    """validate_contract обязан отвергнуть каждый malformed элемент с CT001."""
    cur = conn.cursor()
    with pytest.raises(psycopg2.DatabaseError) as e:
        cur.execute("select validate_contract(%s::jsonb)", (_contract(bad_checks),))
    assert "contract_invalid" in str(e.value), (
        f"F15: ожидался contract_invalid, получено: {e.value}"
    )
    conn.rollback()


@pytest.mark.parametrize(
    "good_checks",
    [
        [{"type": "cmd", "run": "cargo test"}],          # без id (формат планировщика)
        [{"type": "cmd", "id": "unit", "run": "cargo test"}],  # с id
        [{"type": "cmd"}, {"type": "lint", "id": "l1"}],  # несколько, смешанно
    ],
)
def test_f15_accepts_valid_checks(conn, good_checks):
    """Регрессия: валидные checks (в т.ч. без id, как у планировщика) проходят."""
    cur = conn.cursor()
    cur.execute("select validate_contract(%s::jsonb)", (_contract(good_checks),))
    conn.rollback()
