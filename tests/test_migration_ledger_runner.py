"""Реестр применённых миграций: initdb-путь против ЖИВОГО тома (§5.3).

Зачем (ночная смена 2026-10-17). `docs/KNOWN_LIMITATIONS.md` §5.3 фиксирует
остаточное ограничение: миграции применяются только initdb-скриптом
`scripts/05-migrations.sh`, а тот отрабатывает лишь на ПУСТОМ томе. На живом томе
новые миграции не догоняются — после мержа 014 фикс identity в перекрывающих
миграциях остался неприменённым на демо-томе (проверено на живом PG:
`task_rework_feedback` без `assert_worker_identity`). Отдельного раннера для
живого тома не было, только ручной `psql -f` по порядку.

Что появилось: `scripts/apply_migrations.sh` — раннер с реестром
`public.schema_migrations`, который применяет ТОЛЬКО отсутствующее, в отдельной
транзакции на файл (миграция + запись в реестр), держит advisory-lock, ловит
дрейф контрольных сумм и отказывается угадывать на унаследованном томе.

Тест состоит из двух частей:

  * СТАТИЧЕСКАЯ (без БД) — форма реестра у двух скриптов ОДНА (иначе initdb-путь
    и раннер разъедутся молча), раннер подключён в harness и в allowlist;
  * ПОВЕДЕНЧЕСКАЯ (нужна живая disposable-БД через PGDSN) — накат на живой том,
    no-op при повторном прогоне, отказ на унаследованном томе, явное
    «усыновление» реестра, дрейф, отказ на неинициализированной БД и, главное,
    упавшая миграция НЕ попадает в реестр (иначе реестр лжёт о состоянии схемы).

Граница честности: поведенчески проверяется РАННЕР; initdb-скрипт проверяется
только статически (форма реестра, наличие записи после применения файла). Гнать
`docker-entrypoint-initdb.d` из pytest значило бы тащить в тест подъём контейнера
— это делает acceptance-harness; здесь фиксируется то, что не должно разъехаться
молча.

Запуск:
  PGDSN="host=127.0.0.1 port=... user=postgres password=... dbname=c2_swarm" \
    .venv/bin/python -m pytest -q tests/test_migration_ledger_runner.py
"""
from __future__ import annotations

import hashlib
import os
import re
import shutil
import subprocess
from pathlib import Path

import psycopg2
import pytest

REPO = Path(__file__).resolve().parents[1]
RUNNER = REPO / "scripts" / "apply_migrations.sh"
INITDB = REPO / "scripts" / "05-migrations.sh"
HARNESS = REPO / "scripts" / "acceptance_harness.sh"
ALLOWLIST = REPO / "scripts" / "acceptance_allowlist.txt"
MIGRATIONS = REPO / "schema" / "migrations"
SCHEMA = REPO / "schema" / "schema_v0.sql"

PGDSN = os.environ.get("PGDSN", "")

LEDGER_TABLE_RE = re.compile(
    r"create table if not exists public\.schema_migrations\s*\((.*?)\);",
    re.IGNORECASE | re.DOTALL,
)

def _ledger_columns(text: str) -> list[str]:
    """Колонки реестра из DDL скрипта — нормализованные, по порядку."""
    m = LEDGER_TABLE_RE.search(text)
    if not m:
        return []
    return [" ".join(chunk.split()) for chunk in m.group(1).split(",") if chunk.strip()]

def _sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()

def _dbname(dsn: str) -> str:
    if dsn.startswith(("postgres://", "postgresql://")):
        return dsn.split("/")[3].split("?")[0]
    for part in dsn.split():
        if part.startswith("dbname="):
            return part.split("=", 1)[1]
    return ""

def _dsn_with_db(dsn: str, db: str) -> str:
    if dsn.startswith(("postgres://", "postgresql://")):
        head, _, _rest = dsn.partition("/")
        return f"{head}/{db}"
    return " ".join(
        f"dbname={db}" if part.startswith("dbname=") else part for part in dsn.split()
    )

def _disposable(dsn: str) -> bool:
    return _dbname(dsn).startswith(("c2_", "test_", "v0"))

# ---------------------------------------------------------------------------
# Статическая часть: реестр один на два пути применения.
# ---------------------------------------------------------------------------

def test_ledger_shape_is_identical_in_initdb_path_and_runner():
    """initdb-скрипт и раннер обязаны писать ОДИН реестр.

    Разъехавшись, они дадут худший из возможных исходов: раннер не увидит
    записей initdb и пойдёт накатывать 001 поверх существующей `llm_calls`
    (или наоборот — «применит» уже применённое).
    """
    runner_cols = _ledger_columns(RUNNER.read_text(encoding="utf-8"))
    initdb_cols = _ledger_columns(INITDB.read_text(encoding="utf-8"))
    assert runner_cols, "в scripts/apply_migrations.sh нет DDL public.schema_migrations"
    assert initdb_cols, "в scripts/05-migrations.sh нет DDL public.schema_migrations"
    assert runner_cols == initdb_cols, (
        "форма реестра разошлась:\n"
        f"  раннер: {runner_cols}\n  initdb: {initdb_cols}"
    )

def test_both_paths_own_the_ledger_by_swarm_owner():
    """Инвариант каталога: таблицы в public принадлежат не суперпользователю.

    Раннер и initdb ходят суперпользователем, поэтому владельца обязаны
    переводить на swarm_owner сами (step2/test_roles.py::test_catalog_invariants).
    """
    for path in (RUNNER, INITDB):
        text = path.read_text(encoding="utf-8")
        assert "owner to swarm_owner" in text, f"{path.name}: владелец реестра не закреплён"

def test_runner_documents_operational_modes_and_exit_codes():
    text = RUNNER.read_text(encoding="utf-8")
    for flag in ("--dsn", "--docker-exec", "--status", "--dry-run", "--adopt-up-to", "--help"):
        assert flag in text, f"раннер не поддерживает {flag}"
    for code in ("0", "2", "3", "4", "5", "6"):
        assert f"{code} —" in text, f"код выхода {code} не описан в справке"

def test_runner_applies_with_error_stop_and_advisory_lock():
    text = RUNNER.read_text(encoding="utf-8")
    assert "\\set ON_ERROR_STOP on" in text, "нет ON_ERROR_STOP в генерируемом скрипте"
    assert "pg_advisory_lock" in text and "pg_advisory_unlock" in text, "нет advisory-lock"
    assert "printf 'begin;\\n'" in text and "printf 'commit;\\n'" in text, "нет транзакций на файл"
    # Порядок внутри одного файла: begin -> \i <миграция> -> insert в реестр -> commit.
    # Ищем именно блок транзакции (в adopt-блоке insert есть, но без begin/commit).
    m = re.search(r"printf 'begin;\\n'.*?printf 'commit;\\n'", text, re.DOTALL)
    assert m, "не найден блок «транзакция на файл» (begin … commit)"
    block = m.group(0)
    assert re.search(r"printf '\\\\i %s", block), "в транзакции нет применения файла (\\i)"
    assert "insert into public.schema_migrations" in block, (
        "запись в реестр вынесена из транзакции применения файла"
    )

def test_lexicographic_order_is_locale_independent():
    """Порядок применения — лексикографический. Без LC_ALL=C glob зависит от локали."""
    text = RUNNER.read_text(encoding="utf-8")
    assert re.search(r"^export LC_ALL=C$", text, re.M), "раннер не фиксирует LC_ALL=C"
    assert "*.sql" in INITDB.read_text(encoding="utf-8"), "initdb-скрипт больше не глоббит *.sql"

def test_generated_script_is_fed_to_psql_via_stdin():
    """Готовый скрипт идёт в psql через STDIN, а не через `-f <путь>`.

    В режиме `--docker-exec` psql исполняется ВНУТРИ контейнера, а `mktemp`
    живёт на ХОСТЕ: `-f /tmp/swarm-migrations.XXXX.sql` внутри контейнера не
    существует. Поймано на живом демо-томе (`No such file or directory` после
    того, как 013 уже была отмечена в реестре). Поведенческий набор гоняется в
    DSN-режиме, где `-f` работает, поэтому без этой проверки регрессия в
    docker-режиме не видна ни одному тесту.
    """
    text = RUNNER.read_text(encoding="utf-8")
    assert '"${PSQL[@]}" < "$SCRIPT"' in text, (
        "скрипт больше не подаётся в psql через stdin: в --docker-exec режиме "
        "путь к хостовому mktemp внутри контейнера не существует"
    )
    assert not re.search(r'PSQL\[@\]\}"\s+-f\s+"\$SCRIPT"', text), (
        "вернулся psql -f <хостовый путь> вместо stdin"
    )

def test_runner_and_test_are_wired_into_gate():
    """Страж, не подключённый в гейт, — украшение (класс «проверки, которой нет»)."""
    harness = HARNESS.read_text(encoding="utf-8")
    assert "tests/test_migration_ledger_runner.py" in harness, (
        "тест реестра миграций не подключён в scripts/acceptance_harness.sh"
    )
    allow = ALLOWLIST.read_text(encoding="utf-8")
    for path in ("scripts/apply_migrations.sh", "tests/test_migration_ledger_runner.py"):
        assert path in allow, f"{path} вне scope allowlist — правка файла уронит гейт"

# ---------------------------------------------------------------------------
# Поведенческая часть: живая disposable-БД.
# ---------------------------------------------------------------------------

class Pg:
    """Хелперы для живого PG: создание/удаление БД, psql, запуск раннера."""

    def __init__(self, dsn: str):
        self.dsn = dsn
        self._created: list[str] = []

    def _admin(self):
        conn = psycopg2.connect(self.dsn)
        conn.autocommit = True
        return conn

    def _exec(self, sql: str) -> None:
        # Без `with connection`: psycopg2 открывает в нём транзакцию, а
        # CREATE/DROP DATABASE внутри транзакции запрещены.
        conn = self._admin()
        try:
            cur = conn.cursor()
            try:
                cur.execute(sql)
            finally:
                cur.close()
        finally:
            conn.close()

    def create_db(self, name: str) -> str:
        self._exec(f'drop database if exists "{name}" with (force)')
        self._exec(f'create database "{name}"')
        self._created.append(name)
        return _dsn_with_db(self.dsn, name)

    def drop_all(self) -> None:
        for name in self._created:
            try:
                self._exec(f'drop database if exists "{name}" with (force)')
            except Exception:
                pass
        self._created.clear()

    def psql(self, dsn: str, *args: str) -> subprocess.CompletedProcess:
        return subprocess.run(
            ["psql", dsn, "-X", "-q", "-v", "ON_ERROR_STOP=1", *args],
            capture_output=True, text=True,
        )

    def scalar(self, dsn: str, sql: str) -> str:
        out = self.psql(dsn, "-Atc", sql)
        assert out.returncode == 0, f"psql упал: {out.stderr.strip()}"
        return out.stdout.strip()

    def apply_schema(self, dsn: str) -> None:
        out = self.psql(dsn, "-f", str(SCHEMA))
        assert out.returncode == 0, f"schema_v0.sql не применился: {out.stderr.strip()}"

    def apply_migrations_raw(self, dsn: str, names: list[str]) -> None:
        """Как initdb-цепочка: по порядку, без реестра (унаследованный том)."""
        for name in names:
            out = self.psql(dsn, "-f", str(MIGRATIONS / name))
            assert out.returncode == 0, f"{name} не применился: {out.stderr.strip()}"

    def fresh_db(self, tag: str, with_schema: bool = True) -> str:
        name = f"c2_migrunner_{tag}_{os.getpid()}"
        dsn = self.create_db(name)
        if with_schema:
            self.apply_schema(dsn)
        return dsn

@pytest.fixture
def pg():
    if not PGDSN:
        pytest.fail(
            "BLOCKED: PGDSN не задан — поведенческие арбитры реестра миграций не "
            "исполнялись (нужна живая disposable-БД c2_/v0/test_)"
        )
    if not _disposable(PGDSN):
        pytest.fail(f"BLOCKED: PGDSN указывает на не-disposable БД: {_dbname(PGDSN)}")
    if not shutil.which("psql"):
        pytest.fail("BLOCKED: psql не найден в PATH — раннер и арбитр не исполнялись")
    helper = Pg(PGDSN)
    try:
        yield helper
    finally:
        helper.drop_all()

def run_runner(*args: str) -> subprocess.CompletedProcess:
    return subprocess.run([str(RUNNER), *args], capture_output=True, text=True)

def _migration_names() -> list[str]:
    return sorted(p.name for p in MIGRATIONS.glob("*.sql"))

def _ledger_rows(pg: Pg, dsn: str) -> dict[str, str]:
    out = pg.psql(dsn, "-Atc", "select filename || '|' || checksum from public.schema_migrations")
    assert out.returncode == 0, out.stderr
    rows = {}
    for line in out.stdout.splitlines():
        if line.strip():
            name, _, checksum = line.partition("|")
            rows[name] = checksum
    return rows

def test_applies_pending_migrations_and_records_ledger(pg):
    """Свежая БД: раннер накатывает все миграции и честно их записывает."""
    dsn = pg.fresh_db("apply")
    res = run_runner("--adopt-up-to", "none", "--dsn", dsn)
    assert res.returncode == 0, res.stderr
    names = _migration_names()
    rows = _ledger_rows(pg, dsn)
    assert sorted(rows) == names, f"реестр не совпал с каталогом: {sorted(rows)}"
    for name in names:
        assert rows[name] == _sha256(MIGRATIONS / name), f"{name}: контрольная сумма в реестре неверна"
    # llm_calls — маркер того, что миграции реально применились, а не только записались.
    assert pg.scalar(dsn, "select to_regclass('public.llm_calls') is not null") == "t"

def test_second_run_is_a_noop(pg):
    """Повторный прогон ничего не делает: иначе живой том нельзя накатывать дважды."""
    dsn = pg.fresh_db("noop")
    assert run_runner("--adopt-up-to", "none", "--dsn", dsn).returncode == 0
    before = pg.scalar(dsn, "select count(*) || '|' || max(applied_at)::text from public.schema_migrations")
    res = run_runner("--dsn", dsn)
    assert res.returncode == 0, res.stderr
    assert "pending=0" in res.stdout, res.stdout
    after = pg.scalar(dsn, "select count(*) || '|' || max(applied_at)::text from public.schema_migrations")
    assert before == after, "повторный прогон переписал реестр"

def test_status_and_dry_run_do_not_touch_the_database(pg):
    """--status/--dry-run — режимы наблюдения: БД после них не меняется."""
    dsn = pg.fresh_db("readonly")
    status = run_runner("--status", "--dsn", dsn)
    assert status.returncode == 0, status.stderr
    assert "pending" in status.stdout and "ledger: absent" in status.stdout
    dry = run_runner("--dry-run", "--dsn", dsn)
    assert dry.returncode == 0, dry.stderr
    assert "[dry-run] apply" in dry.stdout
    assert pg.scalar(dsn, "select to_regclass('public.schema_migrations') is null") == "t", (
        "--dry-run/--status создали реестр в БД"
    )

def test_refuses_uninitialized_database(pg):
    """БД без базовой схемы: раннер отказывается (иначе «успех» был бы ложным)."""
    dsn = pg.fresh_db("empty", with_schema=False)
    res = run_runner("--dsn", dsn)
    assert res.returncode == 3, f"ожидался rc=3, получен {res.returncode}: {res.stderr}"
    assert pg.scalar(dsn, "select to_regclass('public.schema_migrations') is null") == "t"

def test_refuses_legacy_volume_and_adopts_on_explicit_request(pg):
    """Главный сценарий дефекта: initdb накатал 001–013, 014 не доехала.

    Раннер обязан (а) отказаться угадывать на унаследованном томе, (б) по
    явному `--adopt-up-to 013_` отметить 001–013 без исполнения и накатить
    ровно 014 — то есть довезти фикс identity до живого тома.
    """
    dsn = pg.fresh_db("legacy")
    names = _migration_names()
    assert names[-1].startswith("014"), f"последняя миграция не 014: {names[-1]}"
    pg.apply_migrations_raw(dsn, names[:-1])

    # До раннера фикса на томе нет — это и есть дефект §5.3.
    ident = (
        "select position('assert_worker_identity' in "
        "pg_get_functiondef('public.task_rework_feedback(uuid,text,bigint)'::regprocedure)) > 0"
    )
    assert pg.scalar(dsn, ident) == "f", "на унаследованном томе identity уже был — сценарий не воспроизведён"

    refused = run_runner("--dsn", dsn)
    assert refused.returncode == 6, f"ожидался rc=6, получен {refused.returncode}: {refused.stderr}"

    adopted = run_runner("--adopt-up-to", "013_", "--dsn", dsn)
    assert adopted.returncode == 0, adopted.stderr
    assert "pending=1" in adopted.stdout and f"[apply] {names[-1]}" in adopted.stdout, adopted.stdout
    rows = _ledger_rows(pg, dsn)
    assert sorted(rows) == names, f"реестр не полон: {sorted(rows)}"
    assert pg.scalar(dsn, ident) == "t", "фикс identity не доехал до унаследованного тома"

def test_detects_checksum_drift(pg):
    """Правка уже применённого файла — это дрейф, а не «просто ещё одна миграция»."""
    dsn = pg.fresh_db("drift")
    assert run_runner("--adopt-up-to", "none", "--dsn", dsn).returncode == 0
    victim = _migration_names()[0]
    pg.psql(dsn, "-c", f"update public.schema_migrations set checksum = 'deadbeef' where filename = '{victim}'")
    res = run_runner("--dsn", dsn)
    assert res.returncode == 4, f"ожидался rc=4, получен {res.returncode}: {res.stderr}"
    assert victim in res.stderr

def test_failed_migration_is_not_recorded(pg, tmp_path):
    """Упавшая миграция не попадает в реестр: иначе реестр лжёт о схеме.

    Проверяется на своём каталоге миграций: первая создаёт таблицу, вторая
    падает. Транзакция на файл обязана откатить вторую целиком и не записать её.
    """
    dsn = pg.fresh_db("failed")
    sql_dir = tmp_path / "migrations"
    sql_dir.mkdir()
    (sql_dir / "001_probe.sql").write_text(
        "create table if not exists public.mig_runner_probe (id int);\n", encoding="utf-8"
    )
    (sql_dir / "002_broken.sql").write_text("select 1 / 0;\n", encoding="utf-8")

    res = run_runner("--sql-dir", str(sql_dir), "--adopt-up-to", "none", "--dsn", dsn)
    assert res.returncode == 5, f"ожидался rc=5, получен {res.returncode}: {res.stderr}"
    rows = _ledger_rows(pg, dsn)
    assert sorted(rows) == ["001_probe.sql"], f"в реестре оказалось лишнее: {sorted(rows)}"
    assert pg.scalar(dsn, "select to_regclass('public.mig_runner_probe') is not null") == "t"

def test_ledger_owner_is_not_a_superuser(pg):
    """Инвариант каталога: реестр не должен принадлежать суперпользователю."""
    dsn = pg.fresh_db("owner")
    assert run_runner("--adopt-up-to", "none", "--dsn", dsn).returncode == 0
    owner = pg.scalar(
        dsn,
        "select r.rolsuper from pg_class c join pg_roles r on r.oid = c.relowner "
        "where c.relname = 'schema_migrations'",
    )
    assert owner == "f", "владелец реестра — суперпользователь (нарушение инварианта каталога)"
