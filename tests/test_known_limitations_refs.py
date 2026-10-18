"""Drift guard: ссылки `file:line` в `docs/KNOWN_LIMITATIONS.md` не должны гнить.

Зачем (2026-10-16, PR #3 `docs/limitations-refresh`): документ — «честный список
того, что не работает», и его ценность держится на том, что ссылки указывают на
реальный код. При аудите нашлись три класса гнили:

  * файл удалён/переименован, ссылка осталась (`docs/ROADMAP.md` →
    фактически `docs/optional/ROADMAP.md`; `04-role-patches.sh` → `03-…`);
  * строка уехала (`worker.rs:160` — там уже `Some("text/plain")`, а
    `thread::sleep` вообще убран; `parse.rs:104` — там список `CANONICAL_KINDS`,
    а `deterministic_root_id` на `:163`);
  * статус дефекта устарел (F9/F18 числились «ОТКРЫТО», хотя закрыты `5e27a4c`).

Тест ловит первые два класса механически. Третий — не механизируется: он требует
понимания, поэтому в `ANCHORS` лежит курируемый набор «эталонных» ссылок, каждая
из которых обязана указывать на строку с ожидаемым символом.

Тест НЕ требует БД: чистый текстовый анализ. Запуск:
  .venv/bin/python -m pytest -q tests/test_known_limitations_refs.py
"""
import re
from pathlib import Path

import pytest

REPO = Path(__file__).resolve().parents[1]
DOC = REPO / "docs" / "KNOWN_LIMITATIONS.md"

# Ссылки вида `path/to/file.ext:NNN` (полный путь + строка) и `path/to/file.ext`.
# Голые имена файлов (`allowlist.rs`, `main.rs`) намеренно НЕ проверяются: в
# прозе они контекстны и неоднозначны, а ложные срабатывания в drift guard хуже
# пропуска.
REF_RE = re.compile(r"`([A-Za-z0-9_./-]+/[A-Za-z0-9_.-]+\.(?:rs|sql|sh|py|toml|yml|md))(?::(\d+))?`")

# Полные пути, которых намеренно нет: упоминаются как удалённые/исторические.
ABSENT_ALLOWED = {
    # §5.7: файл-патч УДАЛЁН, это и есть суть закрытия F16г. Ссылка историческая.
    "step2/roles_patch_submit_review.sql",
}

# Курируемые анкеры: ссылка обязана встречаться в тексте документа дословно и
# указывать на строку, содержащую ожидаемый фрагмент. Если код сдвинется —
# тест краснеет и требует обновить документ, а не тихо разойтись.
ANCHORS = {
    "crates/swarm-worker/src/worker.rs:95": "fn sleep_interruptible",
    "crates/swarm-worker/src/worker.rs:608": 'format!("llm_error: {e}")',
    "crates/swarm-llm/src/lib.rs:408": "truncate(&text, 400)",
    "crates/swarm-llm/src/lib.rs:466": "truncate(&text, 400)",
    "crates/swarm-llm/src/error.rs:14": "Http { status: u16, body: String }",
    "crates/swarm-planner/src/parse.rs:168": "pub fn deterministic_root_id",
    "crates/swarm-worker/src/storage.rs:48": "Runtime::new()",
    "step2/roles.sql:91": "to swarm_reviewer",
}


def _doc_text() -> str:
    return DOC.read_text(encoding="utf-8")


def _refs(text: str):
    return {(m.group(1), m.group(2)) for m in REF_RE.finditer(text)}


def _line_count(path: Path) -> int:
    return sum(1 for _ in path.open(encoding="utf-8", errors="replace"))


def test_doc_exists_and_has_refs():
    """Якорь самого теста: документ на месте и в нём вообще есть ссылки.

    Без этого регрессия регулярки/переименование документа дали бы «зелёный»
    тест, проверяющий пустое множество.
    """
    assert DOC.is_file(), f"нет документа: {DOC}"
    refs = _refs(_doc_text())
    assert len(refs) >= 15, f"подозрительно мало ссылок: {len(refs)}"


def test_all_full_path_refs_exist():
    """Каждый `path/to/file.ext[:line]` из документа существует и в диапазоне."""
    bad = []
    for path, line in sorted(_refs(_doc_text()), key=lambda t: (t[0], t[1] or "")):
        if path in ABSENT_ALLOWED:
            continue
        f = REPO / path
        if not f.is_file():
            bad.append(f"{path} — файла нет")
            continue
        if line is not None:
            n = _line_count(f)
            if int(line) > n:
                bad.append(f"{path}:{line} — в файле только {n} строк")
    assert not bad, "гнилые ссылки в docs/KNOWN_LIMITATIONS.md:\n  " + "\n  ".join(bad)


@pytest.mark.parametrize("ref,token", sorted(ANCHORS.items()))
def test_anchor_points_at_expected_symbol(ref, token):
    """Курируемая ссылка встречается дословно и указывает на ожидаемый символ."""
    text = _doc_text()
    assert f"`{ref}`" in text, f"ссылка `{ref}` пропала из документа"
    path, line = ref.rsplit(":", 1)
    f = REPO / path
    assert f.is_file(), f"нет файла {path}"
    lines = f.read_text(encoding="utf-8").splitlines()
    assert int(line) <= len(lines), f"{ref}: строка за пределами файла"
    got = lines[int(line) - 1]
    assert token in got, f"{ref} указывает на {got!r}, ожидался фрагмент {token!r}"


def test_absent_allowlist_is_honest():
    """Allowlist не должен разрастаться в свалку: каждый путь реально отсутствует.

    Если файл вернут — запись обязана уйти, иначе она маскирует настоящую
    гниль под «историческую».
    """
    for path in ABSENT_ALLOWED:
        assert not (REPO / path).is_file(), (
            f"{path} существует — уберите его из ABSENT_ALLOWED"
        )
