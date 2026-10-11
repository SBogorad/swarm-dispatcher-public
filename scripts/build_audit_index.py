#!/usr/bin/env python3
"""Индекс аудит-пакета: читает три тома XML и строит сводный MD + checksums.txt.

Только чтение. Выходы:
  docs/audit/AUDIT_PACKAGE_INDEX_v0.4.md
  docs/audit/CHECKSUMS_v0.4.txt
"""
from __future__ import annotations

import hashlib
import xml.etree.ElementTree as ET
from datetime import datetime, timezone
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
AUD = REPO / "docs" / "audit"
VOLS = ["CODE", "INFRA", "DOCS_CORE", "DOCS"]
OUT_MD = AUD / "AUDIT_PACKAGE_INDEX_v0.4.md"
OUT_SUM = AUD / "CHECKSUMS_v0.4.txt"


def digest(p: Path, algo: str) -> str:
    h = hashlib.new(algo)
    with p.open("rb") as fh:
        for c in iter(lambda: fh.read(65536), b""):
            h.update(c)
    return h.hexdigest()


def main() -> int:
    now = datetime.now(timezone.utc).astimezone().isoformat(timespec="seconds")
    data = []
    for v in VOLS:
        p = AUD / f"PROJECT_{v}_BUNDLE_v0.4.xml"
        r = ET.parse(p).getroot()
        idx = list(r.find("manifest/index"))
        total = int(r.find("manifest/files").get("total_bytes", "0"))
        data.append({
            "vol": v, "path": p, "title": r.findtext("title", v),
            "entries": idx, "src_bytes": total,
            "xml_bytes": p.stat().st_size,
            "sha256": digest(p, "sha256"), "md5": digest(p, "md5"),
            "lines": sum(int(e.get("lines", "0")) for e in idx),
        })

    grand_files = sum(len(d["entries"]) for d in data)
    grand_src = sum(d["src_bytes"] for d in data)
    grand_xml = sum(d["xml_bytes"] for d in data)

    L = []
    L.append("# Аудит-пакет проекта `swarm-dispatcher` — индекс")
    L.append("")
    L.append(f"**Сгенерировано:** {now}")
    L.append("**Генератор:** `scripts/build_project_bundle.py` + `scripts/build_audit_index.py`")
    L.append("**Схема:** `swarm-dispatcher.audit-bundle.v2`")
    L.append("")
    L.append("Предназначено для передачи **внешнему аудитору (LLM)**: полный исходный код, "
             "инфраструктура и документация в машиночитаемых XML-томах. Каждый файл — с "
             "полным текстом и двумя хешами (`sha256`, `md5`). Секреты исключены.")
    L.append("")
    L.append("## Состав пакета")
    L.append("")
    L.append("| Том | Содержание | Файлов | Строк | Исходники, байт | XML, байт |")
    L.append("|-----|------------|-------:|------:|----------------:|----------:|")
    for d in data:
        L.append(f"| `{d['vol']}` | {d['title']} | {len(d['entries'])} | {d['lines']} | "
                 f"{d['src_bytes']} | {d['xml_bytes']} |")
    L.append(f"| **Итого** | | **{grand_files}** | | **{grand_src}** | **{grand_xml}** |")
    L.append("")
    L.append("## Хеши томов (для передачи аудитору)")
    L.append("")
    for d in data:
        L.append(f"### {d['vol']} — `{d['path'].name}`")
        L.append("")
        L.append(f"- Размер: {d['xml_bytes']} байт")
        L.append(f"- `sha256`: `{d['sha256']}`")
        L.append(f"- `md5`: `{d['md5']}`")
        L.append("")
    L.append("## Исключено сознательно")
    L.append("")
    L.append("- `.env`, `.env.bak.*` — **секреты** (пароли ролей, ключи LLM).")
    L.append("- `target/`, `.venv/`, `__pycache__/`, `.pytest_cache/` — сборка/кэш.")
    L.append("- `.docx`, `.png` и прочие бинарники.")
    L.append("- `docs/history/inbound/*` — переписка (не код; при необходимости — отдельно).")
    L.append("")
    L.append("Вместо `.env` включён `.env.example` (структура без значений).")
    L.append("")
    L.append("## Манифест по томам")
    L.append("")
    for d in data:
        L.append(f"### {d['vol']} ({len(d['entries'])} файлов)")
        L.append("")
        L.append("| # | Путь | Язык | Байт | Строк | sha256 |")
        L.append("|--:|------|------|-----:|------:|--------|")
        for i, e in enumerate(d["entries"], 1):
            L.append(f"| {i} | `{e.get('path')}` | {e.get('lang')} | {e.get('bytes')} | "
                     f"{e.get('lines')} | `{e.get('sha256')}` |")
        L.append("")
    L.append("## Как читать")
    L.append("")
    L.append("```bash")
    L.append("# целостность")
    L.append("sha256sum PROJECT_CODE_BUNDLE_v0.4.xml PROJECT_INFRA_BUNDLE_v0.4.xml PROJECT_DOCS_BUNDLE_v0.4.xml")
    L.append("")
    L.append("# список файлов в томе")
    L.append("python3 - <<'PY'")
    L.append("import xml.etree.ElementTree as ET")
    L.append("r = ET.parse('PROJECT_CODE_BUNDLE_v0.4.xml').getroot()")
    L.append("for e in r.find('manifest/index'):")
    L.append("    print(e.get('path'), e.get('bytes'), e.get('sha256')[:16])")
    L.append("PY")
    L.append("```")
    L.append("")

    OUT_MD.write_text("\n".join(L), encoding="utf-8")

    sums = []
    for d in data:
        sums.append(f"{d['sha256']}  {d['path'].name}")
    OUT_SUM.write_text("\n".join(sums) + "\n", encoding="utf-8")

    print(f"OK {OUT_MD}  bytes={OUT_MD.stat().st_size}")
    print(f"OK {OUT_SUM}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
