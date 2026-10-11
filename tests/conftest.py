"""Общий conftest для тестов волны 4.

PRE-CONDITION FAIL = BLOCKED, не skip (урок FIND-08, применённый к нашим тестам).

Pytest собирает хуки только из conftest.py/плагинов — не из тест-модулей.
Поэтому гейт «нет PG => BLOCKED» живёт здесь, а не в файле теста.

Волна 5.1: гейт сделан СЕЛЕКТИВНЫМ. Раньше он требовал W4_* для всего каталога
`tests/`, из-за чего любой прогон `pytest -q tests/test_v0.py` падал с BLOCKED
(UsageError на сборе) — хотя test_v0.py использует только PGDSN, который
acceptance-harness ставит сам. Это вскрылось штатным acceptance_harness.sh:
шаг «state machine (P0 #6/#7)» давал ERROR, а не реальный результат.

Теперь переменные W4_* требуются только для тест-файлов волны 4 (FIND-04,
F3/identity, F4, F2/F15). Базовый test_v0.py и всё остальное запускаются при
наличии только PGDSN/ROLES_*.
"""
import os

import pytest

_REQUIRED = ("W4_ADMIN_DSN", "W4_W1_PW", "W4_W2_PW")

# Файлы, которым действительно нужен live-DB с W4_*-окружением.
# Совпадение по имени узла (basename текущего собираемого модуля).
_WAVE4_ARBITER_FILES = (
    "test_find04_record_tool_call_identity.py",
    "test_worker_identity_f3.py",
    "test_fail_unblock_f4.py",
    "test_review_gate_wave4.py",
    "test_review_gate_f2.py",
    "test_contract_form_f15.py",
)


def _needs_wave4_db(path) -> bool:
    """Требует ли собираемый модуль W4_*-окружение."""
    name = os.path.basename(str(path))
    return name in _WAVE4_ARBITER_FILES


def pytest_collection_modifyitems(config, items):
    """Блокировать ТОЛЬКО тесты волны 4, если W4_* не выставлены.

    Важно: hard BLOCKED, а не skip (тестовая честность). Но — селективно,
    чтобы базовые сьюты (test_v0.py) не падали из-за чужих предусловий.
    """
    missing = [n for n in _REQUIRED if not os.environ.get(n)]
    if not missing:
        return
    blocked = [
        item for item in items if _needs_wave4_db(getattr(item, "fspath", ""))
    ]
    if not blocked:
        return
    reason = (
        "BLOCKED: PostgreSQL not provisioned; wave-4 PG arbiters require a live DB "
        "(missing: " + ", ".join(missing) + ")"
    )
    for item in blocked:
        item.add_marker(pytest.mark.skip(reason=reason))
    # Hard BLOCKED: не «молчаливый зелёный» — сообщаем громко.
    config.issue_config_time_warning(
        pytest.PytestConfigWarning(reason), stacklevel=2
    )
