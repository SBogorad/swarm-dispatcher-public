"""Drift guard: identity-проверка воркера не теряется при перекрытии функций.

Зачем (ночная смена 2026-10-17, FIND-NEW-02). В проекте есть доктрина F3:
воркерская обёртка обязана вызывать `assert_worker_identity(p_worker)` —
deny-by-default, `session_user` должен совпасть с ожидаемой per-worker ролью.
Проверка `assert_lease(p_task, p_worker, p_epoch)` этого НЕ заменяет: она
сверяет `lease_owner` с ПЕРЕДАННЫМ p_worker, то есть доверяет вызывающему.

Опасность в том, что функции определяются НЕСКОЛЬКО РАЗ, и побеждает последнее
определение (порядок применения — как в `docker-entrypoint-initdb.d`):

  1. `schema/schema_v0.sql`            — канон (identity нет);
  2. `step2/roles.sql`                 — владельцы/права, тел не меняет;
  3. `step2b/roles_2b.sql`             — перекрывает 7 воркерских обёрток,
     добавляя identity (F3, FIND-NEW-01);
  4. `step2/roles_patch_*.sql`         — патчи грантов (`record_tool_call`);
  5. `schema/migrations/NNN_*.sql`     — применяются ПОСЛЕ всех и перекрывают
     тела ещё раз.

Последний пункт — источник двух реальных дефектов:

  * **тихая потеря защиты.** Миграция 011 пересоздаёт `claim_task` и
    `complete_task`; её собственный комментарий требует воспроизвести identity
    («иначе create-or-replace тихо снимает защиту»). Проверено зондом на
    disposable-БД: если строку identity из 011 убрать, ВЕСЬ набор арбитров
    остаётся зелёным — слепой зон ровно того класса, что уже дважды кусал
    (S8 для fail_task, S10 для handoff). Правило A ниже закрывает это
    статически, поведенческие арбитры — в `test_worker_identity_f3.py`.
  * **недоделанное перекрытие.** Миграция 012 (`task_rework_feedback`)
    воспроизвела только `assert_lease`, поэтому `swarm_worker_w1` с
    (task_id, epoch) живой аренды w2 читал диагностику ревью чужой задачи
    (rework, verdict, reviewer, artifact_ref, checks). Правило B ловит это без
    БД; фикс — миграция 014, арбитр — F3-тест.

Тест НЕ требует БД: чистый текстовый анализ SQL-файлов.

Запуск:
  .venv/bin/python -m pytest -q tests/test_migration_identity_preserved.py
"""
from __future__ import annotations

import re
from pathlib import Path

import pytest

REPO = Path(__file__).resolve().parents[1]

# Порядок применения = порядок в docker-entrypoint-initdb.d (compose) и в
# scripts/acceptance_harness.sh. `roles.sql` тел функций не меняет, но он часть
# цепочки — включён, чтобы «последнее определение» считалось честно.
FIXED_CHAIN = [
    "schema/schema_v0.sql",
    "step2/roles.sql",
    "step2b/roles_2b.sql",
]

FUNC_START = re.compile(
    r"create\s+(?:or\s+replace\s+)?function\s+(?:public\.)?(\w+)\s*\(", re.IGNORECASE
)
IDENTITY_CALL = re.compile(r"perform\s+assert_worker_identity\s*\(", re.IGNORECASE)
LEASE_CALL = re.compile(r"perform\s+assert_lease\s*\(", re.IGNORECASE)
GRANT_STMT = re.compile(
    r"grant\s+execute\s+on\s+function\s+(.*?)\s+to\s+([^;]*);", re.IGNORECASE | re.DOTALL
)

# Роли, исполняющие воркерские обёртки. `swarm_worker` — общая роль-членство,
# per-worker роли (`swarm_worker_w1`, …) — login-identity.
WORKER_ROLES = {
    "swarm_worker",
    "swarm_worker_w1",
    "swarm_worker_w2",
    "swarm_worker_w_1",
    "swarm_worker_gpu_1",
}

# Имена параметров, которые несут идентичность воркера.
WORKER_PARAM_RE = re.compile(r"^(p_worker|p_id)$")


def _strip_comments(text: str) -> str:
    """Убрать строковые комментарии `-- ...`, чтобы пояснения не считались кодом."""
    return "\n".join(line.split("--", 1)[0] for line in text.splitlines())


def _chain_files() -> list[Path]:
    """Файлы цепочки применения: фиксированные + миграции по возрастанию номера."""
    files = [REPO / p for p in FIXED_CHAIN]
    files += sorted((REPO / "schema" / "migrations").glob("*.sql"))
    files += sorted((REPO / "step2").glob("roles_patch_*.sql"))
    # Патчи грантов применяются из 04-role-patches.sh (до миграций), но тел
    # функций, кроме record_tool_call, они не определяют — порядок между ними и
    # миграциями на результат не влияет. Ставим их перед миграциями честно.
    files = [f for f in files if f.is_file()]
    order = {str(REPO / p): i for i, p in enumerate(FIXED_CHAIN)}
    migs = sorted((REPO / "schema" / "migrations").glob("*.sql"))
    patches = sorted((REPO / "step2").glob("roles_patch_*.sql"))
    rest = [f for f in files if f not in migs and f not in patches]
    return rest + patches + migs


def _matching_paren(text: str, start: int) -> int:
    """Индекс `)`, закрывающей скобку, открытую в позиции `start` (-1 если нет)."""
    depth = 0
    for i, ch in enumerate(text[start:], start):
        if ch == "(":
            depth += 1
        elif ch == ")":
            depth -= 1
            if depth == 0:
                return i
    return -1


def _split_top_level(text: str) -> list[str]:
    """Разбить по запятым, не заглядывая внутрь скобок (типы аргументов)."""
    out, depth, cur = [], 0, []
    for ch in text:
        if ch == "(":
            depth += 1
        elif ch == ")":
            depth -= 1
        if ch == "," and depth == 0:
            out.append("".join(cur))
            cur = []
        else:
            cur.append(ch)
    out.append("".join(cur))
    return [c.strip() for c in out if c.strip()]


def _params(signature: str) -> list[str]:
    """Имена параметров из сигнатуры `(a t, b t default ...)`."""
    start = signature.find("(")
    end = _matching_paren(signature, start) if start != -1 else -1
    if start == -1 or end == -1:
        return []
    out = []
    for chunk in _split_top_level(signature[start + 1 : end]):
        name = chunk.split()[0] if chunk.split() else ""
        if name:
            out.append(name)
    return out


def _definitions(text: str) -> dict[str, tuple[str, list[str]]]:
    """{имя: (тело, параметры)} для всех определений в тексте."""
    out: dict[str, tuple[str, list[str]]] = {}
    for m in FUNC_START.finditer(text):
        end = text.find("$$;", m.end())
        end = len(text) if end == -1 else end + len("$$;")
        paren = text.find("(", m.start())
        close = _matching_paren(text, paren) if paren != -1 else -1
        signature = text[paren:close + 1] if close != -1 else ""
        out[m.group(1)] = (text[m.start() : end], _params(signature))
    return out


def _grants_to_worker_roles(text: str) -> set[str]:
    """Имена функций, которым выдан EXECUTE любой из воркерских ролей."""
    granted: set[str] = set()
    for m in GRANT_STMT.finditer(text):
        roles = {r.strip() for r in m.group(2).split(",") if r.strip()}
        if not (roles & WORKER_ROLES):
            continue
        for item in _split_top_level(m.group(1)):
            name = item.split("(")[0].strip().split(".")[-1]
            if name:
                granted.add(name)
    return granted


@pytest.fixture(scope="module")
def chain():
    """Эффективная цепочка: порядок файлов, все определения, все гранты."""
    files = _chain_files()
    assert files, "цепочка применения пуста — тест проверял бы пустое множество"
    defs: dict[str, list[tuple[str, str, list[str]]]] = {}
    granted: set[str] = set()
    for f in files:
        text = _strip_comments(f.read_text(encoding="utf-8"))
        for name, (body, params) in _definitions(text).items():
            defs.setdefault(name, []).append((f.name, body, params))
        granted |= _grants_to_worker_roles(text)
    return defs, granted, files


def _effective(defs, name):
    """Последнее (побеждающее) определение функции."""
    return defs[name][-1]


def test_inventory_is_meaningful(chain):
    """Якорь: в цепочке реально есть перекрытия и identity-вызовы.

    Без этого регрессия регулярки дала бы «зелёный» тест, проверяющий пустое
    множество (тот же приём, что в test_known_limitations_refs.py).
    """
    defs, granted, files = chain
    assert len(defs) >= 25, f"подозрительно мало функций в цепочке: {len(defs)}"
    assert len(files) >= 12, f"подозрительно мало файлов в цепочке: {len(files)}"
    overridden = {n for n, d in defs.items() if len(d) > 1}
    assert len(overridden) >= 5, f"перекрытий не найдено: {sorted(overridden)}"
    with_identity = {
        n for n in defs if IDENTITY_CALL.search(_effective(defs, n)[1])
    }
    assert len(with_identity) >= 8, (
        "identity-вызовов в цепочке подозрительно мало: " f"{sorted(with_identity)}"
    )
    assert granted & with_identity, "грантов воркерским ролям не найдено"


def test_identity_survives_later_overrides(chain):
    """Правило A: если определение имело identity — последнее обязано иметь тоже.

    Именно это нарушение делает миграция, пересоздающая функцию «забыв» строку:
    `create or replace` тихо снимает защиту, а тесты остаются зелёными.
    """
    defs, _, _ = chain
    lost = []
    for name, defs_list in sorted(defs.items()):
        if not any(IDENTITY_CALL.search(body) for _, body, _ in defs_list):
            continue
        if not IDENTITY_CALL.search(_effective(defs, name)[1]):
            where = " → ".join(f for f, _, _ in defs_list)
            lost.append(f"{name}: {where}")
    assert not lost, (
        "identity-проверка потеряна в последнем определении "
        "(перекрывающая миграция обязана её воспроизвести):\n  " + "\n  ".join(lost)
    )


def test_worker_wrappers_check_identity(chain):
    """Правило B: обёртка воркера с арендой обязана проверять identity.

    Критерий узкий и проверяемый: функция выдана воркерской роли, принимает
    параметр-идентичность (`p_worker`/`p_id`) и вызывает `assert_lease`.
    `assert_lease` сверяет аренду с переданным p_worker, поэтому без
    `assert_worker_identity` чужой воркер проходит проверку, зная только
    (task_id, epoch). Ровно так и был пропущен `task_rework_feedback` в 012.
    """
    defs, granted, _ = chain
    bad = []
    for name in sorted(granted):
        if name not in defs:
            continue
        _, body, params = _effective(defs, name)
        if not LEASE_CALL.search(body):
            continue
        if not any(WORKER_PARAM_RE.match(p) for p in params):
            continue
        if not IDENTITY_CALL.search(body):
            bad.append(f"{name}({', '.join(params)}) — {_effective(defs, name)[0]}")
    assert not bad, (
        "воркерская обёртка проверяет аренду, но не identity "
        "(assert_lease доверяет переданному p_worker):\n  " + "\n  ".join(bad)
    )


def test_identity_call_precedes_lease(chain):
    """Порядок: identity обязана стоять ДО assert_lease.

    Иначе чужой воркер получает LS001 («аренды нет») вместо WK003
    («ты не тот воркер»), и различить «не моя задача» от «аренда истекла»
    по коду ошибки невозможно — это и был симптом FIND-NEW-01.
    """
    defs, granted, _ = chain
    wrong = []
    for name in sorted(granted):
        if name not in defs:
            continue
        _, body, _ = _effective(defs, name)
        i = IDENTITY_CALL.search(body)
        l = LEASE_CALL.search(body)
        if i and l and i.start() > l.start():
            wrong.append(name)
    assert not wrong, (
        "assert_worker_identity стоит ПОСЛЕ assert_lease — чужой воркер получит "
        f"LS001 вместо WK003: {wrong}"
    )
