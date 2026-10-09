#!/usr/bin/env python3
"""Генерирует MD-отчёт по XML-бандлу аудита из самого бандла (хеши гарантированно совпадают).

Только чтение. Выход: docs/audit/AUDIT_BUNDLE_REPORT_v0.4.md
"""
from __future__ import annotations

import hashlib
import sys
import xml.etree.ElementTree as ET
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
XML = REPO / "docs" / "audit" / "SCRIPTS_BUNDLE_v0.4.xml"
OUT = REPO / "docs" / "audit" / "AUDIT_BUNDLE_REPORT_v0.4.md"


def digest(p: Path, algo: str) -> str:
    h = hashlib.new(algo)
    with p.open("rb") as fh:
        for chunk in iter(lambda: fh.read(65536), b""):
            h.update(chunk)
    return h.hexdigest()


def main() -> int:
    if not XML.is_file():
        print(f"ERROR: {XML} not found; run build_audit_bundle.py first", file=sys.stderr)
        return 1

    root = ET.parse(XML).getroot()
    gen = root.get("generated", "?")
    manifest = root.find("manifest")
    files_el = manifest.find("files")
    index = manifest.find("index")

    entries = list(index.findall("entry"))
    total_bytes = int(files_el.get("total_bytes", "0"))
    total_lines = sum(int(e.get("lines", "0")) for e in entries)

    bundle_bytes = XML.stat().st_size
    bundle_sha = digest(XML, "sha256")
    bundle_md5 = digest(XML, "md5")

    by_role: dict[str, list] = {}
    for e in entries:
        by_role.setdefault(e.get("role", "?"), []).append(e)

    L: list[str] = []
    L.append("# Аудит-бандл скриптов — отчёт")
    L.append("")
    L.append("**Проект:** `swarm-dispatcher`  ")
    L.append("**Версия бандла:** v0.4  ")
    L.append(f"**Сгенерировано:** {gen}  ")
    L.append("**Генератор:** `scripts/build_audit_bundle.py`  ")
    L.append("**Артефакт:** `docs/audit/SCRIPTS_BUNDLE_v0.4.xml`")
    L.append("")
    L.append("---")
    L.append("")
    L.append("## 1. Назначение")
    L.append("")
    L.append(
        "Единый машиночитаемый снимок **всех скриптов** репозитория (инициализация БД, схема, "
        "роли, контейнеризация, тесты, конфиг без секретов) для независимого аудита. "
        "Каждый файл включён с полным текстом, размером, числом строк и двумя хешами "
        "(`sha256`, `md5`), что позволяет проверить целостность и воспроизводимость."
    )
    L.append("")
    L.append("## 2. Сводка")
    L.append("")
    L.append(f"- Файлов в бандле: **{len(entries)}**")
    L.append(f"- Суммарный объём исходников: **{total_bytes} байт** (~{total_bytes/1024:.1f} КиБ)")
    L.append(f"- Суммарно строк: **{total_lines}**")
    L.append(f"- Размер XML-бандла: **{bundle_bytes} байт**")
    L.append(f"- `sha256` бандла: `{bundle_sha}`")
    L.append(f"- `md5` бандла: `{bundle_md5}`")
    L.append("")
    L.append("## 3. Состав по ролям")
    L.append("")
    L.append("| Роль | Файлов | Байт |")
    L.append("|------|-------:|-----:|")
    role_names = {
        "init": "Инициализация БД (bash)",
        "schema": "Схема БД",
        "roles": "Роли и гранты",
        "roles-patch": "Патчи грантов",
        "container": "Контейнеризация",
        "test": "Тесты",
        "config": "Конфиг (без секретов)",
        "patch": "Вспомогательные патчи",
    }
    for role in sorted(by_role, key=lambda r: -sum(int(e.get("bytes", "0")) for e in by_role[r])):
        items = by_role[role]
        nbytes = sum(int(e.get("bytes", "0")) for e in items)
        L.append(f"| {role_names.get(role, role)} | {len(items)} | {nbytes} |")
    L.append("")
    L.append("## 4. Манифест файлов")
    L.append("")
    L.append("| # | Путь | Роль | Язык | Байт | Строк | sha256 | md5 |")
    L.append("|--:|------|------|------|-----:|------:|--------|-----|")
    for i, e in enumerate(entries, 1):
        L.append(
            f"| {i} | `{e.get('path')}` | {e.get('role')} | {e.get('lang')} | "
            f"{e.get('bytes')} | {e.get('lines')} | `{e.get('sha256')}` | `{e.get('md5')}` |"
        )
    L.append("")
    L.append("## 5. Исключения (сознательно не включены)")
    L.append("")
    L.append("- `.env` — **секреты** (пароли ролей, API-ключи LLM). В бандл не попадает.")
    L.append("- `.env.bak.*` — резервные копии с секретами.")
    L.append("- `target/` — артефакты сборки Rust.")
    L.append("- `.venv/`, `__pycache__/`, `.pytest_cache/` — окружение и кэш тестов.")
    L.append("")
    L.append("Вместо `.env` включён `.env.example` (структура без значений).")
    L.append("")
    L.append("## 6. Как проверить")
    L.append("")
    L.append("1. Целостность бандла:")
    L.append("")
    L.append("   ```bash")
    L.append("   sha256sum docs/audit/SCRIPTS_BUNDLE_v0.4.xml")
    L.append(f"   # ожидается: {bundle_sha}")
    L.append("   ```")
    L.append("")
    L.append("2. Извлечь файлы и сверить хеши (пример для одного файла):")
    L.append("")
    L.append("   ```bash")
    L.append("   python3 - <<'PY'")
    L.append("   import xml.etree.ElementTree as ET, hashlib")
    L.append("   r = ET.parse('docs/audit/SCRIPTS_BUNDLE_v0.4.xml').getroot()")
    L.append("   for f in r.find('files'):")
    L.append("       body = (f.find('content').text or '')")
    L.append("       h = hashlib.sha256(body.rstrip('\\n').encode()).hexdigest()")
    L.append("       print(f.get('path'), h == f.get('sha256'))")
    L.append("   PY")
    L.append("   ```")
    L.append("")
    L.append("3. Пересобрать с нуля и сравнить:")
    L.append("")
    L.append("   ```bash")
    L.append("   python3 scripts/build_audit_bundle.py")
    L.append("   md5sum docs/audit/SCRIPTS_BUNDLE_v0.4.xml")
    L.append("   ```")
    L.append("")
    L.append("## 7. Оговорка о хешах содержимого")
    L.append("")
    L.append(
        "Хеши в манифесте считаны с **исходных файлов на диске**. В `<content>` текст "
        "записан без завершающего перевода строки, поэтому `sha256` из манифеста не "
        "обязан совпасть с хешем извлечённого текста — для побайтовой сверки используйте "
        "сами файлы репозитория. Манифест и есть эталон."
    )
    L.append("")

    OUT.write_text("\n".join(L), encoding="utf-8")
    print(f"OK  {OUT}")
    print(f"    bytes={OUT.stat().st_size} md5={digest(OUT,'md5')} sha256={digest(OUT,'sha256')}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
