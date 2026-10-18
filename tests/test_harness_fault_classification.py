"""Страж классификации сбоев контура самомодификации (RSI v2).

Зачем. `scripts/self_improve_loop.sh` пишет уроки в `public.agent_memory` —
это знание роя о СВОИХ ошибках. Если контур записывает туда сбой обвязки
(не поднялась тестовая БД, съеденный stdin, таймаут графа), рой получает
суеверный урок о том, чего не делал.

Живой случай: `docker compose exec -T` вычитывал stdin цикла, перебор
кандидатов обрывался на первом артефакте, и контур записал «ни один
одобренный артефакт не содержит применимого unified diff» — при том что
валидный патч лежал в третьем кандидате. Следующий виток учился на
галлюцинации контура.

Правило (Reflexion / ExpeL): урок пишется ТОЛЬКО при `AGENT_FAULT`.
Этот файл — статический страж правила: он не требует БД и ловит регрессию
на уровне текста скрипта.
"""

from __future__ import annotations

import re
from pathlib import Path

import pytest

LOOP = Path(__file__).resolve().parent.parent / "scripts" / "self_improve_loop.sh"


@pytest.fixture(scope="module")
def script() -> str:
    assert LOOP.is_file(), f"контур не найден: {LOOP}"
    return LOOP.read_text(encoding="utf-8")


def test_fault_classifiers_exist(script: str) -> None:
    """Обе категории объявлены и выставляются отдельными функциями."""
    assert re.search(r"^agent_fault\(\)\s*\{", script, re.M), "нет agent_fault()"
    assert re.search(r"^harness_fault\(\)\s*\{", script, re.M), "нет harness_fault()"
    assert "AGENT_FAULT" in script
    assert "HARNESS_FAULT" in script


def test_write_lesson_refuses_without_agent_fault(script: str) -> None:
    """Запись урока закрыта, пока сбой не признан ошибкой роя.

    Проверяется сам гейт внутри `write_lesson`: без него классификация
    осталась бы декоративной — функции есть, а урок пишется всё равно.
    """
    body = re.search(
        r"^write_lesson\(\)\s*\{(.*?)^\}", script, re.M | re.S
    )
    assert body, "нет функции write_lesson"
    inner = body.group(1)
    guard = re.search(
        r'if \[\[ "\$\{FAULT_CLASS:-\}" != "AGENT_FAULT" \]\]; then(.*?)return 0',
        inner,
        re.S,
    )
    assert guard, "write_lesson не проверяет FAULT_CLASS перед вставкой"
    assert "НЕ записан" in guard.group(1) or "НЕ записан" in inner


def test_every_lesson_site_is_classified(script: str) -> None:
    """У каждого вызова `write_lesson` рядом стоит классификация сбоя.

    Иначе новый шаг контура запишет урок «по привычке», и память снова
    начнёт копить суеверия. Окно в 25 строк покрывает комментарии и
    многострочные аргументы вызова.
    """
    lines = script.splitlines()
    sites = [i for i, l in enumerate(lines) if re.match(r"\s*write_lesson\s", l)]
    assert sites, "в контуре нет вызовов write_lesson — страж потерял цель"

    unclassified = []
    for idx in sites:
        window = "\n".join(lines[max(0, idx - 25) : idx + 1])
        if "agent_fault" not in window and "harness_fault" not in window:
            unclassified.append(idx + 1)

    assert not unclassified, (
        "вызовы write_lesson без классификации сбоя (строки): "
        f"{unclassified}. Урок пишется только при AGENT_FAULT."
    )


def test_preflight_runs_before_planner(script: str) -> None:
    """Pre-flight стоит до подачи постановки: грязное дерево не доходит до роя."""
    call = script.find("\npreflight\n")
    planner = script.find('log "B: виток')
    assert call != -1, "preflight объявлен, но не вызывается"
    assert planner != -1, "не найден старт цикла витка"
    assert call < planner, "preflight вызывается после подачи постановки"


def test_target_coverage_check_wired(script: str) -> None:
    """Проверка проводки подключена к пути применения патча.

    «Мёртвый остров»: рой добавил `choose_with_rework` и тест к нему, гейты
    зелёные, а боевой вызов остался старым. Функция без вызова в диффе —
    не выполненная задача.
    """
    assert re.search(r"^missing_targets\(\)\s*\{", script, re.M), "нет missing_targets()"
    assert 'MISSING="$(missing_targets "$PATCH_FILE")"' in script, (
        "missing_targets не вызывается на извлечённом патче"
    )
    # Проверка обязана идти ДО применения: иначе неподключённый патч уже в дереве.
    check = script.find('MISSING="$(missing_targets')
    apply = script.find('git apply --recount "$PATCH_FILE"')
    assert -1 < check < apply, "проводка проверяется после git apply"


def test_compact_output_helpers_used(script: str) -> None:
    """Длинные логи уходят в файл, в консоль — хвост и счётчики.

    Полные простыни `cargo test` в stdout координатора уже роняли сессию
    (`503 system memory overloaded` на 153k токенов).
    """
    assert re.search(r"^tail_log\(\)\s*\{", script, re.M), "нет tail_log()"
    assert re.search(r"^test_counts\(\)\s*\{", script, re.M), "нет test_counts()"
    assert script.count("tail_log /tmp/self-improve-") >= 2, (
        "tail_log не подключён к шагам тестов и гейта"
    )
