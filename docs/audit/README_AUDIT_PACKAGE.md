# Audit package: `swarm-dispatcher` v0.4 / Аудит-пакет: `swarm-dispatcher` v0.4

**README — RU/EN.** Both languages are provided for the whole package.
**README — RU/EN.** Обе версии приведены для всего пакета.

---

## EN

### What this is

A self-contained, machine-readable snapshot of the `swarm-dispatcher` repository
for an **independent external auditor** (an LLM), together with a brief describing
what to look for. Content is shipped as XML volumes; every file carries a full
text body and two hashes (`sha256`, `md5`); secrets are excluded.

### Files in this package

| File | Language | Purpose |
|------|----------|---------|
| `AUDITOR_BRIEF_v0.4.md` | RU | The brief: scope, what to check, answer format |
| `AUDITOR_BRIEF_v0.4.en.md` | EN | The same brief, in English |
| `PROJECT_CODE_BUNDLE_v0.4.xml` | — | Rust: 6 crates (src, tests, fixtures, Cargo) |
| `PROJECT_INFRA_BUNDLE_v0.4.xml` | — | SQL (schema/roles/patches), init scripts, Docker/compose, config |
| `PROJECT_DOCS_CORE_BUNDLE_v0.4.xml` | RU | Core docs (specs, architecture, limitations, reports) |
| `PROJECT_DOCS_BUNDLE_v0.4.xml` | RU | Core docs + methodology, decision history, correspondence |
| `SCRIPTS_BUNDLE_v0.4.xml` | — | All repository scripts (SQL/bash/py) |
| `AUDIT_PACKAGE_INDEX_v0.4.md` | RU | Index of the pack with per-volume hashes |
| `AUDIT_BUNDLE_REPORT_v0.4.md` | RU | Report on the scripts bundle |
| `CHECKSUMS_v0.4.txt` | — | `sha256` per volume |

### Revision under audit

- Branch: `feature/v0.6-distributed-worker-mvp`
- HEAD (`git rev-parse`): recorded in each volume as `<git head="…"/>`
  and in `AUDIT_PACKAGE_INDEX_v0.4.md`.
- The bundle head **must equal** the git HEAD you were given; if not, the package
  is stale — ask for a rebuild.

### How to start

1. Read `AUDITOR_BRIEF_v0.4.en.md` (or the RU original).
2. Verify integrity: `sha256sum -c CHECKSUMS_v0.4.txt`.
3. Follow the recommended reading order in the brief (§2).

### Language note

The brief is available in **Russian and English**. The source code, SQL, and
commit messages are in English; the project documentation (in the volumes) is
primarily Russian. Findings may be reported in either language.

---

## RU

### Что это

Самодостаточный машиночитаемый снимок репозитория `swarm-dispatcher` для
**внешнего независимого аудитора** (LLM), вместе с брифом о том, что проверять.
Содержимое — XML-тома; каждый файл с полным текстом и двумя хешами
(`sha256`, `md5`); секреты исключены.

### Состав пакета

| Файл | Язык | Назначение |
|------|------|------------|
| `AUDITOR_BRIEF_v0.4.md` | RU | Бриф: рамки, что проверять, формат ответа |
| `AUDITOR_BRIEF_v0.4.en.md` | EN | Тот же бриф на английском |
| `PROJECT_CODE_BUNDLE_v0.4.xml` | — | Rust: 6 крейтов (src, тесты, фикстуры, Cargo) |
| `PROJECT_INFRA_BUNDLE_v0.4.xml` | — | SQL (схема/роли/патчи), init-скрипты, Docker/compose, конфиг |
| `PROJECT_DOCS_CORE_BUNDLE_v0.4.xml` | RU | Ключевая документация (ТЗ, архитектура, ограничения, отчёты) |
| `PROJECT_DOCS_BUNDLE_v0.4.xml` | RU | Ключевая документация + методология, история решений, переписка |
| `SCRIPTS_BUNDLE_v0.4.xml` | — | Все скрипты репозитория (SQL/bash/py) |
| `AUDIT_PACKAGE_INDEX_v0.4.md` | RU | Индекс пакета с хешами по томам |
| `AUDIT_BUNDLE_REPORT_v0.4.md` | RU | Отчёт по бандлу скриптов |
| `CHECKSUMS_v0.4.txt` | — | `sha256` по томам |

### Ревизия под аудитом

- Ветка: `feature/v0.6-distributed-worker-mvp`
- HEAD (`git rev-parse`): записан в каждом томе как `<git head="…"/>`
  и в `AUDIT_PACKAGE_INDEX_v0.4.md`.
- Head бандла **обязан совпадать** с выданным git HEAD; если нет — пакет устарел,
  запросите пересборку.

### Как начать

1. Прочитайте `AUDITOR_BRIEF_v0.4.md` (или английскую версию).
2. Проверьте целостность: `sha256sum -c CHECKSUMS_v0.4.txt`.
3. Идите по рекомендуемому порядку в брифе (§2).

### О языке

Бриф доступен **на русском и английском**. Исходный код, SQL и сообщения коммитов —
на английском; документация проекта (в томах) — преимущественно на русском.
Находки можно оформлять на любом из двух языков.
