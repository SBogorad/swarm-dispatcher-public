"""Общий conftest для тестов волны 4.

PRE-CONDITION FAIL = BLOCKED, не skip (урок FIND-08, применённый к нашим тестам).

Pytest собирает хуки только из conftest.py/плагинов — не из тест-модулей.
Поэтому гейт «нет PG => BLOCKED» живёт здесь, а не в файле теста.
"""
import os

import pytest

_REQUIRED = ("W4_ADMIN_DSN", "W4_W1_PW", "W4_W2_PW")


def pytest_configure(config):
    missing = [n for n in _REQUIRED if not os.environ.get(n)]
    if missing:
        # Hard BLOCKED: pytest останавливается на сборе, exit code 4, статус ERROR,
        # а НЕ 'skipped' и НЕ молчаливый зелёный.
        raise pytest.UsageError(
            "BLOCKED: PostgreSQL not provisioned; wave-4 PG arbiters require a live DB "
            "(missing: " + ", ".join(missing) + ")"
        )
