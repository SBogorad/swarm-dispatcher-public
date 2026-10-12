"""Декомпозиция цели в граф задач (DAG) для демонстрационного стенда.

Честно о природе: это **локальная rule-based декомпозиция** — резервный путь,
когда настоящий `swarm-planner` (LLM) недоступен. Она кладёт граф в движок
теми же публичными функциями, что и планировщик (`add_task` + `add_dep`),
поэтому зависимости и промоушен `pending → ready` считает именно движок.

Важное ограничение v0.5.x, учтённое здесь: исполнитель получает ТОЛЬКО свой
контракт и НЕ видит артефактов других задач. Поэтому каждая задача и каждая
проверка приёмки сформулированы самодостаточно — «проверить чужой результат»
такой задачей выразить нельзя.
"""
from __future__ import annotations

from typing import Any

# Реестр допустимых типов проверок (синхронизирован с движком: N5/F7).
# cmd | test | unit | lint | manual


def _contract(kind: str, description: str, checks: list[dict[str, str]],
              max_tasks: int = 50) -> dict[str, Any]:
    """Контракт задачи. Лимиты графа живут в контракте корня (как у планировщика)."""
    return {
        "description": description,
        "kind": kind,
        "checks": checks,
        "needs": {"min_ram_gb": 0, "coder_class": 0},
        "budget": {
            "attempts": 3,
            "max_rework": 2,
            "max_tasks": max_tasks,
            "max_depth": 10,
            "max_children": 20,
        },
    }


# Стадии конвейера: (kind, заголовок, шаблон описания, проверка).
# Описания самодостаточны: исполнитель не видит других задач.
_STAGES: list[tuple[str, str, str, str]] = [
    (
        "analysis",
        "Анализ и план",
        "Проанализируй постановку и составь план работ. Цель: {g}. "
        "Дай: 1) разбор цели и границ работ; 2) перечень ожидаемых результатов; "
        "3) этапы с критериями готовности; 4) допущения и риски.",
        "analysis_present",
    ),
    (
        "design",
        "Проработка решения",
        "Разработай решение по цели: {g}. "
        "Опиши состав решения по пунктам: сущности и их поля, интерфейсы и "
        "правила, порядок внедрения. Каждый пункт — с обоснованием.",
        "design_present",
    ),
    (
        "audit",
        "Аудит рисков",
        "Проведи аудит рисков по цели: {g}. "
        "Дай: 1) не менее восьми рисков; 2) для каждого — причина, следствие, "
        "мера снижения и остаточный риск; 3) условия допуска к запуску.",
        "audit_present",
    ),
]


def decompose(goal: str) -> list[dict[str, Any]]:
    """Цель → список самодостаточных подзадач с линейными зависимостями.

    Возвращает список словарей: `{kind, title, description, checks, depends_on}`,
    где `depends_on` — индексы в этом же списке (0 — корень).
    """
    g = " ".join((goal or "").split())
    if not g:
        g = "задача без описания"

    tasks: list[dict[str, Any]] = []
    for i, (kind, title, tmpl, check_id) in enumerate(_STAGES):
        tasks.append({
            "kind": kind,
            "title": title,
            "description": tmpl.format(g=g),
            "checks": [{"id": check_id, "type": "manual", "run": check_id}],
            "depends_on": [i - 1] if i > 0 else [],
        })
    return tasks


def build_contract(task: dict[str, Any], max_tasks: int = 50) -> dict[str, Any]:
    return _contract(task["kind"], task["description"], task["checks"], max_tasks)
