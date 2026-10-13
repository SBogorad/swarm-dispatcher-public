"""Drift guard: дублирование тел функций schema_v0.sql ↔ step2b/roles_2b.sql.

Техдолг (ночная смена 2026-10-14): `step2b/roles_2b.sql` переопределяет через
`create or replace` пять функций воркера, уже определённых в
`schema/schema_v0.sql`:

  register_worker, claim_task, heartbeat, save_artifact, complete_task

Это ровно тот класс дефекта, что уже один раз уронил прод: `submit_review`
определялась дважды (schema_v0.sql + step2/roles_patch_submit_review.sql), патч
перекрывал каноническое тело, и фиксы в схеме были мёртвым кодом
(см. docs/KNOWN_LIMITATIONS.md §5.7, F16г). `complete_task` в roles_2b.sql
однажды возвращал `'blocked'` вместо `'budget_exhausted'` — тоже перекрытие
(см. tests/test_v0.py::test_budget_exhaustion_blocks_on_complete).

Сейчас перекрытие **осознанное и корректное**: roles_2b добавляет ровно одну
строку — `perform assert_worker_identity(...)` (deny-by-default identity воркера,
F3). Тела во всём остальном идентичны каноническим.

Этот тест — детектор расхождений (drift guard), а не запрет перекрытия:
  * перекрываемая функция должна входить в разрешённый список;
  * её тело после вычищения комментариев/пробелов и единственной разрешённой
    строки identity-проверки обязано совпадать с каноническим.

Если кто-то поправит тело в schema_v0.sql и забудет про roles_2b.sql (или
наоборот) — тест краснеет и указывает, что именно разошлось.

Тест НЕ требует БД: это чистый текстовый анализ. Запуск:
  .venv/bin/python -m pytest -q tests/test_no_duplicate_function_bodies.py
"""
import re
from pathlib import Path

import pytest

REPO = Path(__file__).resolve().parents[1]
SCHEMA = REPO / "schema" / "schema_v0.sql"
ROLES_2B = REPO / "step2b" / "roles_2b.sql"

# Функции, которые roles_2b.sql намеренно перекрывает, добавляя identity-проверку.
ALLOWED_OVERRIDES = {
    "register_worker",
    "claim_task",
    "heartbeat",
    "save_artifact",
    "complete_task",
}

# Единственная разрешённая дополнительная строка в перекрытом теле.
IDENTITY_LINE = "perform assert_worker_identity("

_FUNC_START = re.compile(r"create\s+(?:or\s+replace\s+)?function\s+(\w+)\s*\(", re.IGNORECASE)


def _extract_functions(text: str) -> dict[str, str]:
    """Вернуть {имя: тело} для всех `create [or replace] function` в тексте.

    Тело — от начала определения до закрывающего `$$;`.
    """
    out: dict[str, str] = {}
    for m in _FUNC_START.finditer(text):
        name = m.group(1)
        end = text.find("$$;", m.end())
        if end == -1:
            end = len(text)
        else:
            end += len("$$;")
        out[name] = text[m.start():end]
    return out


# Identity-вызов с любым именем параметра: `perform assert_worker_identity(<arg>);`
_IDENTITY_CALL = re.compile(r"perform\s+assert_worker_identity\s*\([^)]*\)\s*;")


def _normalize(body: str) -> str:
    """Токенизированное представление: без комментариев, все пробелы → один.

    Схлопываем переносы строк, чтобы косметическая переупаковка параметров не
    считалась расхождением — сравниваем семантику тела, а не верстку.
    """
    body = re.sub(r"--.*", "", body)
    body = re.sub(r"\s+", " ", body)
    return body.strip()


def _canonicalize_canon(body: str) -> str:
    """Канон: `create function` → `create or replace function` (форма перекрытия)."""
    return re.sub(r"create\s+function", "create or replace function", body, count=1)


def _without_identity(body: str) -> str:
    """Убрать разрешённую identity-строку (её в каноне нет) и схлопнуть пробелы."""
    return re.sub(r"\s+", " ", _IDENTITY_CALL.sub(" ", body)).strip()


@pytest.fixture(scope="module")
def bodies():
    schema = _extract_functions(SCHEMA.read_text(encoding="utf-8"))
    roles = _extract_functions(ROLES_2B.read_text(encoding="utf-8"))
    return schema, roles


def test_sources_exist():
    assert SCHEMA.is_file(), f"нет {SCHEMA}"
    assert ROLES_2B.is_file(), f"нет {ROLES_2B}"


def test_roles_2b_only_overrides_allowed_functions(bodies):
    """roles_2b.sql не должен тихо перекрывать функции вне разрешённого списка.

    Новые функции (нет в schema_v0.sql, напр. normalize_worker_id /
    assert_worker_identity) — это не перекрытие, они допустимы. Проверяем только
    функции, определённые в ОБОИХ файлах.
    """
    schema, roles = bodies
    unexpected = sorted((set(roles) & set(schema)) - ALLOWED_OVERRIDES)
    assert not unexpected, (
        "roles_2b.sql перекрывает функции вне разрешённого списка: "
        f"{unexpected}. Либо это осознанное решение (тогда расширьте "
        "ALLOWED_OVERRIDES и объясните в комментарии), либо убрать перекрытие."
    )


@pytest.mark.parametrize("name", sorted(ALLOWED_OVERRIDES))
def test_override_body_matches_schema(bodies, name):
    """Тело перекрытия = каноническое тело + ровно identity-строка."""
    schema, roles = bodies
    assert name in schema, f"{name} отсутствует в schema_v0.sql (канон потерян)"
    assert name in roles, f"{name} не перекрыта в roles_2b.sql"

    canon = _canonicalize_canon(_normalize(schema[name]))
    override = _without_identity(_normalize(roles[name]))

    if canon != override:
        import difflib

        diff = "\n".join(
            difflib.unified_diff(
                canon.split(" "), override.split(" "),
                "schema_v0.sql", "roles_2b.sql", lineterm="",
            )
        )
        pytest.fail(
            f"{name}: тело в roles_2b.sql разошлось с каноном из schema_v0.sql "
            f"(кроме разрешённой identity-строки):\n{diff}"
        )


def test_override_adds_identity_check(bodies):
    """Перекрытие обязано добавлять именно identity-проверку, а не что-то иное."""
    _, roles = bodies
    for name in ALLOWED_OVERRIDES:
        assert IDENTITY_LINE in roles[name], (
            f"{name}: перекрытие в roles_2b.sql не содержит identity-проверки "
            f"({IDENTITY_LINE}...) — либо защита потеряна, либо тест устарел"
        )
