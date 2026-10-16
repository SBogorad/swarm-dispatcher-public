"""Drift guard: ссылки `path[:line]` в ЖИВЫХ документах не должны гнить.

Зачем (2026-10-17, ночная смена, Q3). У `docs/KNOWN_LIMITATIONS.md` уже есть
свой страж (`tests/test_known_limitations_refs.py`, PR #3). Но полный скан
`docs/**` + корневых `*.md` показал, что тот же класс гнили живёт во всём
остальном корпусе: 59 битых ссылок. Реальные дефекты:

  * документы уехали в `docs/optional/`, а ссылки остались на `docs/X.md`
    (MANIFEST.md, RESUME.md, `docs/optional/*.md`, `docs/history/README.md`) — 32
    замены;
  * переименованный файл: `crates/swarm-core/tests/integration.rs` →
    `integration_verified.rs` (`docs/TZ_v0.2_step1_fixes.md`);
  * путь без префикса `crates/` (`docs/TZ_v0.3.1_planner_description.md`);
  * путь монтирования внутри контейнера вместо репозиторного
    (`docker-entrypoint-initdb.d/02-roles.sh` → `scripts/02-roles.sh`).

Остальные 25 — не дефекты, а ссылки на удалённое, запланированное или чужое:
исторические записи CHANGELOG об удалении файлов, удалённый Python-прототип
v0.1, план ROADMAP (файлов ещё нет по определению), чужой репозиторий в
`docs/research/`. Они перечислены в ALLOW с причиной, и тест требует, чтобы
каждая запись ALLOW была реально нужна: если ссылка перестала быть битой или
исчезла из документа — запись обязана уйти. Иначе allowlist незаметно
превращается в свалку, маскирующую настоящую гниль.

Скоуп. Проверяются «живые» документы: корневые `*.md` (кроме снимков
`REPORT_*.md`) и `docs/**/*.md` без `docs/history/**` и `docs/audit/**` — это
замороженные артефакты аудита и архив, их ссылки отражают состояние на момент
среза и переписыванию не подлежат. Исключение — `docs/history/README.md`: это
индекс архива, он живёт и обязан указывать на существующие документы.

Два класса ссылок:

* **В бэктиках** — `` `path/to/file.ext[:NNN]` `` — проверяются всегда.
* **Без бэктиков** — только если первый компонент пути совпадает с элементом
  корня репозитория (`docs/…`, `tests/…`, `crates/…`, `schema/…`). Такая
  ссылка однозначна: это путь от корня репо. А вот `worker/src/worker.rs` или
  `llm/src/lib.rs` — прозаичное сокращение внутри описания конкретного крейта;
  проверять его нельзя, давно и честно задокументированная причина — ложные
  срабатывания в drift guard хуже пропусков (см. docstring
  `tests/test_known_limitations_refs.py`). Второй класс появился после того, как
  скан нашёл гниль именно в нём: `README.md` (точка входа!) отправлял читателя
  в `docs/ROADMAP.md`, `docs/ERRORS.md`, `docs/BRIEF_FOR_AGENTS.md`,
  `docs/NATIVE_SETUP.md` — всё это уехало в `docs/optional/`, — а также в
  `docs/PLAIN_EXPLANATION.md`, которого не было никогда (проверено по всей
  истории: `git log --all -- docs/PLAIN_EXPLANATION.md` пуст).

Тест НЕ требует БД: чистый текстовый анализ.

Запуск:
  .venv/bin/python -m pytest -q tests/test_docs_link_drift.py
"""
from __future__ import annotations

import re
from pathlib import Path

import pytest

REPO = Path(__file__).resolve().parents[1]

# `path/to/file.ext` или `path/to/file.ext:NNN`.
# Абсолютные пути (`/tmp/...`) не проверяются: они по определению вне репозитория.
REF_RE = re.compile(
    r"`((?!/)[A-Za-z0-9_./-]+/[A-Za-z0-9_.-]+\.(?:rs|sql|sh|py|toml|yml|yaml|md|json|txt))(?::(\d+))?`"
)

# Ссылки БЕЗ бэктиков. Проверяются только пути от корня репозитория: первый
# компонент должен совпадать с элементом корня (`docs/`, `tests/`, `crates/`, …).
# Список берётся из файловой системы, а не хардкодится, — иначе он сам станет
# источником дрейфа. Не-корневые пути (`worker/src/worker.rs`) — прозаичные
# сокращения, см. docstring.
BARE_RE = re.compile(
    r"(?<![`/\w.-])((?:[A-Za-z0-9_-]+/)+[A-Za-z0-9_.-]+\.(?:rs|sql|sh|py|toml|yml|yaml|md|json|txt))(?::(\d+))?(?![`\w])"
)


def _top_level() -> set[str]:
    return {p.name for p in REPO.iterdir() if p.is_dir() and not p.name.startswith(".")}


TOP_LEVEL = _top_level()

# Законные битые ссылки БЕЗ бэктиков, первый компонент которых — корневой.
# Ключ — (документ, путь из ссылки), значение — причина.
ALLOW_BARE: dict[tuple[str, str], str] = {}

# Замороженные срезы: аудит-пакеты и архив. Ссылки внутри них — свидетельства
# состояния кода на дату среза, а не обещания; править их значит подделывать
# историю. Живой индекс архива — исключение, см. LIVE_EXTRA.
FROZEN_DIRS = ("docs/history/", "docs/audit/")
# Корневые отчёты — снимки прогонов (REPORT_v0.2_step1.md и т.п.).
FROZEN_ROOT_PREFIX = "REPORT_"
# Индексы, которые живут даже внутри замороженных каталогов.
LIVE_EXTRA = ("docs/history/README.md",)

# Ссылки, которые обязаны оставаться битыми, с причиной.
# Ключ — (документ, путь из ссылки). Строка со номером строки игнорируется:
# важно, что ссылка всё ещё битая и всё ещё нужна.
ALLOW: dict[tuple[str, str], str] = {
    # --- исторические записи об УДАЛЕНИИ (сам факт удаления и есть содержание) ---
    ("CHANGELOG.md", "schema/roles.sql"): "запись об удалении заглушки, PR-история",
    ("CHANGELOG.md", "step2b/roles_2b_patch_section3.sql"): "запись об удалении патча",
    ("CHANGELOG.md", "step2b/test_roles_2b.DELIVERED_partial.py"): "запись об удалении обрезанного файла",
    ("CHANGELOG.md", "worker/main.rs"): "запись о правке файла, впоследствии удалённого",
    ("CHANGELOG.md", "watchdog/main.rs"): "запись о правке файла, впоследствии удалённого",
    # --- та же запись, что в tests/test_known_limitations_refs.py (ABSENT_ALLOWED) ---
    ("docs/KNOWN_LIMITATIONS.md", "step2/roles_patch_submit_review.sql"): "§5.7: патч удалён — в этом суть закрытия F16г",
    # --- утраченные артефакты верификации ---
    ("docs/VERIFICATION_LOG.md", "docs/history/REPORT_v0_e2e.md"): "артефакт утрачен; в тексте помечено как отсутствующий",
    # --- удалённый Python-прототип v0.1 (документ описывает его как историю) ---
    ("docs/optional/NEXT_STEPS.md", "swarm/dispatcher.py"): "прототип v0.1 на Python удалён",
    ("docs/optional/NEXT_STEPS.md", "swarm/errors.py"): "прототип v0.1 на Python удалён",
    ("docs/optional/NEXT_STEPS.md", "swarm/types.py"): "прототип v0.1 на Python удалён",
    ("docs/optional/NEXT_STEPS.md", "swarm/watchdog.py"): "прототип v0.1 на Python удалён",
    ("docs/optional/NEXT_STEPS.md", "swarm/worker_stub.py"): "прототип v0.1 на Python удалён",
    ("docs/optional/NEXT_STEPS.md", "swarm/cli.py"): "прототип v0.1 на Python удалён",
    ("docs/optional/NEXT_STEPS.md", "sql/roles.sql"): "старый путь SQL-ролей (теперь step2/roles.sql)",
    ("docs/optional/NEXT_STEPS.md", "bench/RESULTS.md"): "файл результатов бенчмарка не версионируется",
    # --- план: ROADMAP перечисляет файлы, которых ещё нет по определению ---
    ("docs/optional/ROADMAP.md", "tests/test_planner.py"): "план: тесты Planner (Rust, не Python)",
    ("docs/optional/ROADMAP.md", "crates/swarm-worker/src/llm.rs"): "план: обёртка LLM",
    ("docs/optional/ROADMAP.md", "crates/swarm-worker/src/tools.rs"): "план: инструменты воркера",
    ("docs/optional/ROADMAP.md", "docs/TZ_v0.3_worker.md"): "план: ТЗ ещё не написано",
    ("docs/optional/ROADMAP.md", "docs/TZ_v0.4_web.md"): "план: ТЗ ещё не написано",
    ("docs/optional/ROADMAP.md", "crates/swarm-core/src/git.rs"): "план: обёртка Git",
    ("docs/optional/ROADMAP.md", "docs/TZ_v0.5_git.md"): "план: ТЗ ещё не написано",
    ("docs/optional/ROADMAP.md", "bench/RESULTS.md"): "план: файл результатов бенчмарка",
    # --- чужой репозиторий: разбор внешнего проекта, пути не наши ---
    ("docs/research/agent-orchestrator.md", "backend/internal/storage/sqlite/migrations/0001_init.sql"): "разбор внешнего проекта (Go/SQLite), путь не в этом репозитории",
}


def _live_docs() -> list[Path]:
    """Живые документы: корневые *.md без снимков + docs/** без архивов."""
    docs: list[Path] = []
    for path in sorted(REPO.glob("*.md")):
        if path.name.startswith(FROZEN_ROOT_PREFIX):
            continue
        docs.append(path)
    for path in sorted(REPO.glob("docs/**/*.md")):
        rel = path.relative_to(REPO).as_posix()
        if rel.startswith(FROZEN_DIRS):
            continue
        docs.append(path)
    for rel in LIVE_EXTRA:
        docs.append(REPO / rel)
    return docs


def _refs(doc: Path) -> set[tuple[str, str | None]]:
    text = doc.read_text(encoding="utf-8", errors="replace")
    return {(m.group(1), m.group(2)) for m in REF_RE.finditer(text)}


def _bare_refs(doc: Path) -> set[tuple[str, str | None]]:
    """Ссылки без бэктиков, у которых первый компонент — элемент корня репо."""
    text = doc.read_text(encoding="utf-8", errors="replace")
    out = set()
    for m in BARE_RE.finditer(text):
        path = m.group(1)
        if path.split("/", 1)[0] in TOP_LEVEL:
            out.add((path, m.group(2)))
    return out


def _line_count(path: Path) -> int:
    return sum(1 for _ in path.open(encoding="utf-8", errors="replace"))


def _broken() -> list[tuple[str, str, str | None, str]]:
    """Все битые ссылки живых документов: (документ, путь, строка, причина).

    Бэктик-ссылки и бесбэктик-ссылки от корня — один общий список: обе обещают
    читателю существующий файл.
    """
    broken: list[tuple[str, str, str | None, str]] = []
    for doc in _live_docs():
        rel_doc = doc.relative_to(REPO).as_posix()
        refs = _refs(doc) | _bare_refs(doc)
        for path, line in sorted(refs, key=lambda t: (t[0], t[1] or "")):
            target = REPO / path
            if not target.is_file():
                broken.append((rel_doc, path, line, "файла нет"))
            elif line is not None and int(line) > _line_count(target):
                broken.append((rel_doc, path, line, f"в файле только {_line_count(target)} строк"))
    return broken


ALL_ALLOWED = {**ALLOW, **ALLOW_BARE}


def test_scope_is_not_empty():
    """Якорь теста: скоуп реально набран и в нём есть проверяемые ссылки.

    Без этого опечатка в `docs/**`-глобе дала бы «зелёный» тест над пустым
    множеством — ровно тот класс дефекта, против которого страж и написан.
    """
    docs = _live_docs()
    assert len(docs) >= 30, f"подозрительно мало живых документов: {len(docs)}"
    refs = sum(len(_refs(d)) for d in docs)
    assert refs >= 150, f"подозрительно мало ссылок: {refs}"
    bare = sum(len(_bare_refs(d)) for d in docs)
    assert bare >= 40, f"подозрительно мало ссылок без бэктиков: {bare}"


def test_no_broken_refs_in_live_docs():
    """Каждая ссылка (в бэктиках или от корня) существует и попадает в диапазон."""
    unallowed = [
        f"{doc}: `{path}{':' + line if line else ''}` — {why}"
        for doc, path, line, why in _broken()
        if (doc, path) not in ALL_ALLOWED
    ]
    assert not unallowed, "гнилые ссылки в живых документах:\n  " + "\n  ".join(unallowed)


def test_allowlist_entries_are_still_needed():
    """Allowlist не должен разрастаться: каждая запись всё ещё битая и всё ещё в тексте.

    Если ссылка починена или исчезла — запись обязана уйти, иначе она тихо
    прикрывает будущую регрессию.
    """
    broken = {(doc, path) for doc, path, _line, _why in _broken()}
    refs_in_docs = set()
    for doc in _live_docs():
        rel = doc.relative_to(REPO).as_posix()
        for path, _line in _refs(doc) | _bare_refs(doc):
            refs_in_docs.add((rel, path))
    stale = []
    for key in sorted(ALL_ALLOWED):
        if key not in refs_in_docs:
            stale.append(f"{key[0]}: ссылка `{key[1]}` исчезла из документа")
        elif key not in broken:
            stale.append(f"{key[0]}: ссылка `{key[1]}` больше не битая — починилась")
    assert not stale, "устаревшие записи ALLOW:\n  " + "\n  ".join(stale)


@pytest.mark.parametrize("key", sorted(ALL_ALLOWED))
def test_allow_entry_has_reason(key: tuple[str, str]):
    """У каждой записи ALLOW есть непустая причина, объясняющая, почему битая ссылка законна."""
    assert ALL_ALLOWED[key].strip(), f"{key} без причины"
