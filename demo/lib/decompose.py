"""Декомпозиция цели в граф задач (DAG) для демонстрационного стенда.

Честно о природе: это **локальная rule-based декомпозиция**, а не LLM-планировщик.
Она кладёт граф в движок теми же публичными функциями, что и настоящий
`swarm-planner` (`add_task` + `add_dep`), поэтому всё интересное — зависимости,
промоушен `pending → ready`, порядок исполнения — делает именно движок.

Настоящий Rust-планировщик (`swarm-planner`) подключается опционально
(см. docs в demo/README.md): он требует доступного LLM-эндпоинта, которого в
изолированном контуре без модели нет. Тогда как fallback используется эта
детерминированная схема — ровно как допускает ТЗ («rule-based для
гарантированной демонстрации без интернета»).
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


def decompose(goal: str) -> list[dict[str, Any]]:
    """Цель → список подзадач трёхзвенного конвейера с линейными зависимостями.

    Возвращает список словарей: `{kind, description, checks, depends_on}`,
    где `depends_on` — индексы в этом же списке (0 — корень).
    """
    g = " ".join((goal or "").split())
    if not g:
        g = "задача без описания"

    # Стадии конвейера: анализ → исполнение → проверка/аудит.
    stages: list[tuple[str, str, str, list[dict[str, str]]]] = [
        (
            "analysis",
            "Анализ и план",
            f"Разобрать постановку, собрать контекст и составить план работ: {g}",
            [{"id": "analysis_present", "type": "manual", "run": "analysis_present"}],
        ),
        (
            "execution",
            "Исполнение",
            f"Выполнить основную работу по задаче и подготовить артефакт: {g}",
            [{"id": "execution_present", "type": "test", "run": "execution_present"}],
        ),
        (
            "review",
            "Проверка и аудит",
            f"Проверить результат, провести аудит и подтвердить приёмку: {g}",
            [{"id": "review_present", "type": "manual", "run": "review_present"}],
        ),
    ]

    tasks: list[dict[str, Any]] = []
    for i, (kind, title, desc, checks) in enumerate(stages):
        tasks.append({
            "kind": kind,
            "title": title,
            "description": desc,
            "checks": checks,
            "depends_on": [i - 1] if i > 0 else [],
        })
    return tasks


def build_contract(task: dict[str, Any], max_tasks: int = 50) -> dict[str, Any]:
    return _contract(task["kind"], task["description"], task["checks"], max_tasks)
