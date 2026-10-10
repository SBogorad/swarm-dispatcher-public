#!/usr/bin/env python3
"""Собирает XML-бандл всех скриптов репозитория для аудита.

Только чтение. Секреты (.env) НЕ включаются — только .env.example.
Выход: docs/audit/SCRIPTS_BUNDLE_v0.4.xml
"""
from __future__ import annotations

import hashlib
import os
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path
from xml.sax.saxutils import escape, quoteattr

REPO = Path(__file__).resolve().parent.parent
OUT_DIR = REPO / "docs" / "audit"
OUT_XML = OUT_DIR / "SCRIPTS_BUNDLE_v0.4.xml"

# (путь, роль, язык)
FILES = [
    # --- скрипты инициализации ---
    ("scripts/02-roles.sh", "init", "bash"),
    ("scripts/03-roles-2b.sh", "init", "bash"),
    ("scripts/03-role-patches.sh", "init", "bash"),
    # --- схема и роли (SQL) ---
    ("schema/schema_v0.sql", "schema", "sql"),
    ("step2/roles.sql", "roles", "sql"),
    ("step2/roles_patch_planner_status.sql", "roles-patch", "sql"),
    ("step2/roles_patch_tool_call_audit.sql", "roles-patch", "sql"),
    ("step2b/roles_2b.sql", "roles", "sql"),
    ("step2b/roles_2b_patch_section3.sql", "roles-patch", "sql"),
    # --- контейнеризация ---
    ("Dockerfile", "container", "dockerfile"),
    ("sandbox/Dockerfile", "container", "dockerfile"),
    ("docker-compose.yml", "container", "yaml"),
    # --- тесты ---
    ("tests/test_v0.py", "test", "python"),
    ("step2/test_roles.py", "test", "python"),
    ("step2b/test_roles_2b.py", "test", "python"),
    # --- конфиг без секретов ---
    (".env.example", "config", "dotenv"),
    # --- вспомогательное ---
    ("rust_fixes/dispatcher.rs", "patch", "rust"),
]

FORBIDDEN = {".env", ".env.bak.1791466553"}


def sha256(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as fh:
        for chunk in iter(lambda: fh.read(65536), b""):
            h.update(chunk)
    return h.hexdigest()


def md5(path: Path) -> str:
    h = hashlib.md5()
    with path.open("rb") as fh:
        for chunk in iter(lambda: fh.read(65536), b""):
            h.update(chunk)
    return h.hexdigest()


def git_info() -> dict:
    def run(*args):
        try:
            return subprocess.run(
                ["git", *args], cwd=REPO, capture_output=True, text=True, timeout=10
            )
        except Exception:
            return None

    r = run("rev-parse", "HEAD")
    if r is None or r.returncode != 0:
        return {"is_git": "false", "note": "not a git repository"}
    return {
        "is_git": "true",
        "head": r.stdout.strip(),
        "branch": (run("rev-parse", "--abbrev-ref", "HEAD") or type("x", (), {"stdout": ""})).stdout.strip(),
        "dirty": "true" if (run("status", "--porcelain") and run("status", "--porcelain").stdout.strip()) else "false",
    }


def main() -> int:
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    now = datetime.now(timezone.utc).astimezone().isoformat(timespec="seconds")

    entries = []
    missing = []
    for rel, role, lang in FILES:
        p = REPO / rel
        if rel in FORBIDDEN or not p.is_file():
            missing.append(rel)
            continue
        raw = p.read_bytes()
        try:
            text = raw.decode("utf-8")
        except UnicodeDecodeError:
            text = raw.decode("utf-8", errors="replace")
        entries.append(
            {
                "path": rel,
                "role": role,
                "lang": lang,
                "bytes": len(raw),
                "lines": text.count("\n") + (0 if text.endswith("\n") else 1),
                "sha256": sha256(p),
                "md5": md5(p),
                "text": text,
            }
        )

    total_bytes = sum(e["bytes"] for e in entries)
    gi = git_info()

    out = []
    out.append('<?xml version="1.0" encoding="UTF-8"?>')
    out.append(
        '<audit-bundle schema="swarm-dispatcher.audit-bundle.v1" '
        f'generated={quoteattr(now)} generator={quoteattr("scripts/build_audit_bundle.py")}>'
    )
    out.append("  <project>")
    out.append(f"    <name>swarm-dispatcher</name>")
    out.append(f"    <root>{escape(str(REPO))}</root>")
    out.append(f"    <git is_git={quoteattr(gi['is_git'])}"
               + (f" head={quoteattr(gi.get('head',''))} branch={quoteattr(gi.get('branch',''))} dirty={quoteattr(gi.get('dirty',''))}/>" if gi["is_git"] == "true" else "")
               + ("" if gi["is_git"] == "true" else f"><note>{escape(gi.get('note',''))}</note></git>"))
    out.append("  </project>")
    out.append("  <manifest>")
    out.append(f"    <files count=\"{len(entries)}\" total_bytes=\"{total_bytes}\"/>")
    out.append("    <excluded>")
    out.append("      <file path=\".env\" reason=\"secrets\"/>")
    out.append("      <file path=\".env.bak.*\" reason=\"secrets\"/>")
    out.append("      <dir path=\"target/\" reason=\"build artifacts\"/>")
    out.append("      <dir path=\".venv/\" reason=\"virtualenv\"/>")
    out.append("    </excluded>")
    out.append("    <index>")
    for e in entries:
        out.append(
            f'      <entry path={quoteattr(e["path"])} role={quoteattr(e["role"])} '
            f'lang={quoteattr(e["lang"])} bytes="{e["bytes"]}" lines="{e["lines"]}" '
            f'sha256={quoteattr(e["sha256"])} md5={quoteattr(e["md5"])}/>'
        )
    out.append("    </index>")
    out.append("  </manifest>")
    out.append("  <files>")
    for e in entries:
        out.append(
            f'    <file path={quoteattr(e["path"])} role={quoteattr(e["role"])} '
            f'lang={quoteattr(e["lang"])} bytes="{e["bytes"]}" lines="{e["lines"]}" '
            f'sha256={quoteattr(e["sha256"])} md5={quoteattr(e["md5"])}>'
        )
        # CDATA, кроме случая когда текст содержит ]]>
        # Trailing blanks are stripped: embedded REPORT_*.md carry markdown
        # hard-breaks (two trailing spaces) which would otherwise land in the
        # bundle and trip `git diff --check` on any regenerated artifact.
        body = "\n".join(line.rstrip() for line in e["text"].rstrip("\n").split("\n"))
        if "]]>" in body:
            out.append("      <content encoding=\"xml-escaped\">")
            out.append(escape(body))
            out.append("      </content>")
        else:
            out.append("      <content><![CDATA[")
            out.append(body)
            out.append("]]></content>")
        out.append("    </file>")
    out.append("  </files>")
    out.append("</audit-bundle>")

    OUT_XML.write_text("\n".join(out) + "\n", encoding="utf-8")
    print(f"OK  {OUT_XML}")
    print(f"    files={len(entries)} total_bytes={total_bytes}")
    print(f"    md5={md5(OUT_XML)} sha256={sha256(OUT_XML)}")
    if missing:
        print(f"    MISSING: {', '.join(missing)}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
