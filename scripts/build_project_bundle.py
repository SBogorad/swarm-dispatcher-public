#!/usr/bin/env python3
"""Собирает полный аудит-бандл проекта (весь код) в XML, по томам.

Тома:
  code  — Rust-исходники, тесты, Cargo-манифесты, фикстуры
  infra — схема/роли SQL, init-скрипты, Dockerfile/compose, конфиги без секретов
  docs  — документация проекта (docs/**, кроме писем inbound) + корневые отчёты

Только чтение. Секреты (.env*) и бинарники (.docx/.png) исключены.
Выход: docs/audit/PROJECT_<VOLUME>_BUNDLE_v0.4.xml
"""
from __future__ import annotations

import hashlib
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path
from xml.sax.saxutils import escape, quoteattr

REPO = Path(__file__).resolve().parent.parent
OUT_DIR = REPO / "docs" / "audit"

EXCL_DIRS = {"target", ".venv", "__pycache__", ".git", "node_modules",
             ".pytest_cache", ".mypy_cache"}
EXCL_FILES = {".env", ".env.bak.1791466553"}
BINARY_SUFFIX = {".docx", ".png", ".jpg", ".jpeg", ".gif", ".pdf", ".zip",
                 ".gz", ".tar", ".rlib", ".rmeta", ".o", ".so"}

# volume -> (описание, [пути-файлы] или корневые префиксы для рекурсии)
VOLUMES: dict[str, dict] = {
    "CODE": {
        "title": "Код проекта (Rust + тесты + манифесты)",
        "globs": ["crates/**/*.rs", "crates/**/Cargo.toml", "Cargo.toml",
                  "Cargo.lock", "crates/**/*.json"],
    },
    "INFRA": {
        "title": "Инфраструктура (SQL, init-скрипты, контейнеры, конфиг)",
        "globs": ["schema/*", "step2/*", "step2b/*", "scripts/*",
                  "docker-compose.yml", "Dockerfile", ".env.example",
                  ".dockerignore", ".gitignore", "LICENSE", "tests/*.py",
                  ".github/workflows/*"],
    },
    "DOCS": {
        "title": "Документация и отчёты (без писем history/inbound)",
        "globs": ["docs/**/*.md", "*.md", "docs/history/*.md"],
        "exclude_prefix": ["docs/history/inbound/", "docs/audit/"],
    },
    "DOCS_CORE": {
        "title": "Ключевая документация (ТЗ, архитектура, ограничения, отчёты)",
        "globs": ["docs/TZ_*.md", "docs/ARCHITECTURE.md",
                  "docs/KNOWN_LIMITATIONS.md", "docs/PROJECT_STATE.md",
                  "docs/METHODOLOGY.md", "docs/VERIFICATION_LOG.md",
                  "README.md", "CHANGELOG.md", "RESUME.md", "MANIFEST.md",
                  "REPORT_*.md", "docs/audit/AUDITOR_BRIEF_v0.4.md",
                  "docs/audit/AUDITOR_BRIEF_v0.4.en.md",
                  "docs/audit/README_AUDIT_PACKAGE.md"],
    },
}


def sha256(b: bytes) -> str:
    return hashlib.sha256(b).hexdigest()


def md5(b: bytes) -> str:
    return hashlib.md5(b).hexdigest()


def file_digest(p: Path, algo: str) -> str:
    h = hashlib.new(algo)
    with p.open("rb") as fh:
        for chunk in iter(lambda: fh.read(65536), b""):
            h.update(chunk)
    return h.hexdigest()


def git_info() -> dict:
    try:
        r = subprocess.run(["git", "rev-parse", "HEAD"], cwd=REPO,
                           capture_output=True, text=True, timeout=10)
        if r.returncode == 0:
            return {"is_git": "true", "head": r.stdout.strip()}
    except Exception:
        pass
    return {"is_git": "false", "note": "not a git repository"}


def collect(vol: dict) -> list[Path]:
    seen: set[Path] = set()
    ex_pfx = vol.get("exclude_prefix", [])
    for g in vol["globs"]:
        for p in sorted(REPO.glob(g)):
            if not p.is_file():
                continue
            if set(p.relative_to(REPO).parts) & EXCL_DIRS:
                continue
            if p.name in EXCL_FILES or p.suffix in BINARY_SUFFIX:
                continue
            rel = p.relative_to(REPO).as_posix()
            if any(rel.startswith(x) for x in ex_pfx):
                continue
            seen.add(p)
    return sorted(seen, key=lambda p: p.relative_to(REPO).as_posix())


def lang_of(p: Path) -> str:
    n = p.name
    if n == "Dockerfile":
        return "dockerfile"
    if n == "Cargo.toml" or n == "Cargo.lock":
        return "toml"
    if n.startswith(".env"):
        return "dotenv"
    return {".rs": "rust", ".sql": "sql", ".sh": "bash", ".py": "python",
            ".md": "markdown", ".yml": "yaml", ".yaml": "yaml",
            ".json": "json", ".toml": "toml"}.get(p.suffix, "text")


def build_volume(name: str, vol: dict) -> tuple[Path, int, int]:
    files = collect(vol)
    entries = []
    for p in files:
        raw = p.read_bytes()
        text = raw.decode("utf-8", errors="replace")
        entries.append({
            "path": p.relative_to(REPO).as_posix(), "lang": lang_of(p),
            "bytes": len(raw), "lines": text.count("\n") + (0 if text.endswith("\n") else 1),
            "sha256": sha256(raw), "md5": md5(raw), "text": text,
        })
    total = sum(e["bytes"] for e in entries)
    now = datetime.now(timezone.utc).astimezone().isoformat(timespec="seconds")
    gi = git_info()
    out = ['<?xml version="1.0" encoding="UTF-8"?>',
           f'<audit-volume schema="swarm-dispatcher.audit-bundle.v2" volume={quoteattr(name)} '
           f'generated={quoteattr(now)} generator="scripts/build_project_bundle.py">',
           f'  <title>{escape(vol["title"])}</title>',
           f'  <project>swarm-dispatcher</project>',
           f'  <root>{escape(str(REPO))}</root>',
           f'  <git is_git={quoteattr(gi["is_git"])}'
           + (f' head={quoteattr(gi.get("head",""))}/>' if gi["is_git"] == "true"
              else f'><note>{escape(gi.get("note",""))}</note></git>'),
           '  <manifest>',
           f'    <files count="{len(entries)}" total_bytes="{total}"/>',
           '    <excluded>',
           '      <file path=".env*" reason="secrets"/>',
           '      <dir path="target/" reason="build artifacts"/>',
           '      <dir path=".venv/" reason="virtualenv"/>',
           '      <file path="*.docx|*.png" reason="binary"/>',
           '    </excluded>',
           '    <index>']
    for e in entries:
        out.append(f'      <entry path={quoteattr(e["path"])} lang={quoteattr(e["lang"])} '
                   f'bytes="{e["bytes"]}" lines="{e["lines"]}" '
                   f'sha256={quoteattr(e["sha256"])} md5={quoteattr(e["md5"])}/>')
    out += ['    </index>', '  </manifest>', '  <files>']
    for e in entries:
        out.append(f'    <file path={quoteattr(e["path"])} lang={quoteattr(e["lang"])} '
                   f'bytes="{e["bytes"]}" lines="{e["lines"]}" '
                   f'sha256={quoteattr(e["sha256"])} md5={quoteattr(e["md5"])}>')
        if "]]>" in e["text"]:
            # XML-escaped body: strip trailing blanks per line for the same reason.
            body = "\n".join(line.rstrip() for line in e["text"].rstrip("\n").split("\n"))
            out += ['      <content encoding="xml-escaped">', escape(body),
                    '      </content>']
        else:
            # Strip trailing blanks: the embedded files (e.g. REPORT_*.md with
            # markdown hard-breaks) would otherwise put trailing whitespace into
            # the bundle, which trips `git diff --check` on any regenerated
            # artifact. The bundle is an audit snapshot of content, not of
            # insignificant line padding.
            body = "\n".join(line.rstrip() for line in e["text"].rstrip("\n").split("\n"))
            out += ['      <content><![CDATA[', body, ']]></content>']
        out.append('    </file>')
    out += ['  </files>', '</audit-volume>']

    dest = OUT_DIR / f"PROJECT_{name}_BUNDLE_v0.4.xml"
    dest.write_text("\n".join(out) + "\n", encoding="utf-8")
    return dest, len(entries), total


def main() -> int:
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    rows = []
    for name, vol in VOLUMES.items():
        dest, n, total = build_volume(name, vol)
        rows.append((name, dest, n, total, dest.stat().st_size,
                     file_digest(dest, "sha256"), file_digest(dest, "md5")))
        print(f"OK {name:6s} {dest.name}  files={n} bytes={total} "
              f"xml={dest.stat().st_size}")
    print()
    for name, dest, n, total, xb, s, m in rows:
        print(f"{name}: files={n} src_bytes={total} xml_bytes={xb}")
        print(f"   sha256={s}")
        print(f"   md5={m}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
