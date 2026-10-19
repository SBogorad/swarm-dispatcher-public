"""Полнота ИТОГОВОЙ схемы после полной цепочки применения.

Зачем (2026-10-20). Класс дефекта, который этот страж ловит: «миграция лежит в
`schema/migrations/`, но до схемы не доехала». Именно так 015–018 не оказались
на живом томе: файлы в дереве есть, реестр о них молчит, функции памяти в
каталоге отсутствуют, и воркер работает без recall, не падая при этом.

Почему НЕ «каталог из schema_v0.sql vs каталог после миграций». Такое сравнение
краснело бы всегда: `schema_v0.sql` — не снимок всей схемы, а БАЗА, поверх
которой накатываются миграции. На 2026-10-20 разница штатная: 11 функций и
3 таблицы. Различие — устройство, а не дрейф. Поэтому проверяется не сходство с
базой, а полнота результата цепочки.

Почему поведенческий, а не статический. Статическая проверка («файл есть в
каталоге и упомянут в реестре») не поймает миграцию, которая падает на синтаксисе
или на отсутствующем объекте. Здесь цепочка реально применяется на живой
disposable-БД тем же порядком, что и в проде:

    schema_v0.sql -> roles.sql -> roles_2b.sql -> role patches -> migrations/*.sql

Граница честности: проверяется initdb-путь (`schema_v0` + `05-migrations`-порядок).
Поведение раннера живого тома (no-op, дрейф, --adopt-up-to) — отдельный страж,
`tests/test_migration_ledger_runner.py`.

Запуск:
  PGDSN="host=127.0.0.1 port=... user=postgres password=... dbname=c2_swarm" \
    .venv/bin/python -m pytest -q tests/test_schema_chain_completeness.py
"""
from __future__ import annotations

import os
import shutil
import subprocess
from pathlib import Path

import psycopg2
import pytest

REPO = Path(__file__).resolve().parents[1]
SCHEMA = REPO / "schema" / "schema_v0.sql"
ROLES = REPO / "step2" / "roles.sql"
ROLES_2B = REPO / "step2b" / "roles_2b.sql"
PATCHES = (
    REPO / "step2" / "roles_patch_planner_status.sql",
    REPO / "step2" / "roles_patch_tool_call_audit.sql",
)
MIGRATIONS = REPO / "schema" / "migrations"
# Настоящий initdb-скрипт. Страж гоняет ЕГО, а не повторяет его логику:
# реестр `schema_migrations` создаётся именно здесь, и копия этой логики в тесте
# проверяла бы тест, а не прод (именно так первый прогон дал ложный красный).
INITDB = REPO / "scripts" / "05-migrations.sh"
HARNESS = REPO / "scripts" / "acceptance_harness.sh"
ALLOWLIST = REPO / "scripts" / "acceptance_allowlist.txt"

PGDSN = os.environ.get("PGDSN", "")

# Снимок ожидаемого каталога. Меняется ОСОЗНАННО вместе с миграцией, которая
# добавляет объект: рост числа — повод посмотреть, что именно доехало.
#
# 10 таблиц, а не 9: `public.schema_migrations` создаёт сам initdb-скрипт
# `scripts/05-migrations.sh`, а не миграция. Первый вариант стража считал 9,
# потому что эмулировал накат файлов у себя и реестр не создавал — то есть
# проверял копию логики, а не прод. Теперь страж гоняет настоящий скрипт,
# и реестр честно входит в каталог.
EXPECTED_FUNCTIONS = 37
EXPECTED_TABLES = 10

# RPC, без которых контур деградирует молча: воркер работает, но без handoff
# и без памяти между графами.
REQUIRED_RPCS = ("task_parent_artifacts", "task_recall_memory")

# Пароли ролей — фиксированные c2_*: БД одноразовая и изолированная
# (инвариант scripts/scratch_pg.sh: боевые роли так не трогаются).
ROLE_VARS = {
    "worker_pw": "c2_worker",
    "watchdog_pw": "c2_watchdog",
    "planner_pw": "c2_planner",
    "reviewer_pw": "c2_reviewer",
    "operator_pw": "c2_operator",
    "cli_pw": "c2_cli",
}
ROLES_2B_VARS = {"w1_pw": "c2_w1", "w2_pw": "c2_w2", "w_1_pw": "c2_w_1", "gpu_1_pw": "c2_gpu_1"}


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


def _migration_names() -> list[str]:
    return sorted(p.name for p in MIGRATIONS.glob("*.sql"))


class Pg:
    """Одноразовая БД + psql с ON_ERROR_STOP (как в проде)."""

    def __init__(self, dsn: str):
        self.dsn = dsn
        self._created: list[str] = []

    def _admin(self):
        conn = psycopg2.connect(self.dsn)
        conn.autocommit = True
        return conn

    def _exec(self, sql: str) -> None:
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
            capture_output=True,
            text=True,
        )

    def apply(self, dsn: str, path: Path, variables: dict[str, str] | None = None) -> None:
        args = ["-f", str(path)]
        for key, value in (variables or {}).items():
            args += ["-v", f"{key}={value}"]
        out = self.psql(dsn, *args)
        assert out.returncode == 0, f"{path.name} не применился: {out.stderr.strip()}"

    def scalar(self, dsn: str, sql: str) -> str:
        out = self.psql(dsn, "-Atc", sql)
        assert out.returncode == 0, f"psql упал: {out.stderr.strip()}"
        return out.stdout.strip()


@pytest.fixture
def pg():
    if not PGDSN:
        pytest.fail(
            "BLOCKED: PGDSN не задан — страж полноты схемы не исполнялся "
            "(нужна живая disposable-БД c2_/v0/test_)"
        )
    if not _disposable(PGDSN):
        pytest.fail(f"BLOCKED: PGDSN указывает на не-disposable БД: {_dbname(PGDSN)}")
    if not shutil.which("psql"):
        pytest.fail("BLOCKED: psql не найден в PATH — цепочка не применялась")
    helper = Pg(PGDSN)
    try:
        yield helper
    finally:
        helper.drop_all()


# ---------------------------------------------------------------------------
# Поведенческая часть: полная цепочка с нуля.
# ---------------------------------------------------------------------------

def _apply_full_chain(pg: Pg, tag: str = "chain") -> tuple[str, list[str]]:
    """schema -> roles -> roles_2b -> patches -> 05-migrations.sh. Как в проде.

    Последний шаг — НАСТОЯЩИЙ initdb-скрипт: он же создаёт реестр и пишет в него
    каждую миграцию. Если бы тест накатывал файлы сам, он не проверял бы ни
    реестр, ни порядок, ни `set -eu` продакшн-скрипта.
    """
    dsn = pg.create_db(f"c2_schemachain_{tag}_{os.getpid()}")
    pg.apply(dsn, SCHEMA)
    pg.apply(dsn, ROLES, ROLE_VARS)
    pg.apply(dsn, ROLES_2B, ROLES_2B_VARS)
    for patch in PATCHES:
        if patch.is_file():
            pg.apply(dsn, patch)
    _run_initdb_script(dsn)
    return dsn, _migration_names()


def _dsn_parts(dsn: str) -> dict[str, str]:
    parts: dict[str, str] = {}
    for chunk in dsn.split():
        if "=" in chunk:
            key, _, value = chunk.partition("=")
            parts[key] = value
    return parts


def _run_initdb_script(dsn: str) -> None:
    """Прогнать scripts/05-migrations.sh так, как это делает postgres-образ.

    Скрипт вызывает `psql -U "$POSTGRES_USER" -d "$POSTGRES_DB"` без host/port,
    полагаясь на libpq-окружение. Передаём его через PGHOST/PGPORT/PGPASSWORD,
    а каталог миграций — через MIGRATIONS_DIR (в образе это /sql/migrations).
    """
    parts = _dsn_parts(dsn)
    env = dict(os.environ)
    env.update(
        {
            "POSTGRES_USER": parts.get("user", "postgres"),
            "POSTGRES_DB": parts.get("dbname", "postgres"),
            "PGHOST": parts.get("host", "127.0.0.1"),
            "PGPORT": parts.get("port", "5432"),
            "MIGRATIONS_DIR": str(MIGRATIONS),
        }
    )
    if "password" in parts:
        env["PGPASSWORD"] = parts["password"]
    out = subprocess.run(
        ["sh", str(INITDB)], capture_output=True, text=True, env=env
    )
    assert out.returncode == 0, (
        "scripts/05-migrations.sh упал — свежий том не поднимется: "
        f"{out.stderr.strip() or out.stdout.strip()}"
    )


def test_full_chain_applies_without_errors(pg):
    """Цепочка с нуля проходит целиком: ни один файл не падает.

    Падение здесь означает, что том с нуля не поднимется вовсе — самый
    дорогой из отказов, потому что обнаруживается только при пересоздании.
    """
    dsn, names = _apply_full_chain(pg)
    assert names, "в schema/migrations нет ни одного .sql"
    # Контроль, что БД действительно жива после цепочки.
    assert pg.scalar(dsn, "select 1") == "1"


def test_catalog_matches_expected_size(pg):
    """Итоговый каталог = ожидаемые 37 функций и 10 таблиц.

    Числа — снимок устройства на 2026-10-20. Если они разошлись, значит либо
    миграция не доехала, либо добавилась новая и снимок надо обновить осознанно.
    Десятая таблица — `schema_migrations`: её создаёт initdb-скрипт, не миграция.
    37-я функция — `schema_version()` из миграции 019 (гард версии схемы).
    """
    dsn, _names = _apply_full_chain(pg, "catalog")
    functions = int(
        pg.scalar(
            dsn,
            "select count(*) from pg_proc p "
            "join pg_namespace n on n.oid = p.pronamespace "
            "where n.nspname = 'public'",
        )
    )
    tables = int(
        pg.scalar(
            dsn,
            "select count(*) from pg_tables where schemaname = 'public'",
        )
    )
    assert functions == EXPECTED_FUNCTIONS, (
        f"функций в public: {functions}, ожидалось {EXPECTED_FUNCTIONS}. "
        "Либо миграция не доехала, либо снимок пора обновить"
    )
    assert tables == EXPECTED_TABLES, (
        f"таблиц в public: {tables}, ожидалось {EXPECTED_TABLES}"
    )


def test_every_migration_reached_the_ledger(pg):
    """Каждый файл миграции отмечен в public.schema_migrations.

    Реестр — единственный источник правды о том, что применено. Молчащий
    реестр при живом файле означает, что раннер живого тома пойдёт накатывать
    это повторно (и упадёт на `create table` без `if not exists`).
    """
    dsn, names = _apply_full_chain(pg, "ledger")
    applied = pg.psql(dsn, "-Atc", "select filename from public.schema_migrations order by 1")
    assert applied.returncode == 0, f"реестр недоступен: {applied.stderr.strip()}"
    recorded = [line for line in applied.stdout.splitlines() if line.strip()]
    missing = [name for name in names if name not in recorded]
    assert not missing, f"миграции не попали в реестр: {missing}"


def test_handoff_and_memory_rpcs_exist(pg):
    """RPC handoff и памяти обязаны быть в каталоге после цепочки.

    Без них контур работает, но деградирует молча: родительские артефакты не
    доезжают, уроки прошлых графов не вспоминаются, ошибки повторяются.
    """
    dsn, _names = _apply_full_chain(pg, "rpcs")
    wanted = ", ".join(f"'{name}'" for name in REQUIRED_RPCS)
    present = pg.psql(
        dsn,
        "-Atc",
        "select proname from pg_proc p "
        "join pg_namespace n on n.oid = p.pronamespace "
        f"where n.nspname = 'public' and proname in ({wanted})",
    )
    assert present.returncode == 0, f"запрос каталога упал: {present.stderr.strip()}"
    found = {line.strip() for line in present.stdout.splitlines() if line.strip()}
    absent = [name for name in REQUIRED_RPCS if name not in found]
    assert not absent, f"в каталоге нет RPC: {absent}"


def test_memory_table_exists(pg):
    """`agent_memory` — носитель памяти роя (у контейнера воркера нет диска)."""
    dsn, _names = _apply_full_chain(pg, "memory")
    assert pg.scalar(dsn, "select to_regclass('public.agent_memory') is not null") == "t"


def test_schema_version_rpc_reports_last_migration(pg):
    """`schema_version()` — единственный способ сервисам узнать версию схемы.

    Реестр `schema_migrations` принадлежит `swarm_owner`, а `revoke all ... from
    public` закрывает чтение всем сервисным ролям: проверено на живом томе,
    `has_table_privilege('swarm_worker', 'public.schema_migrations', 'select')`
    = false. Без этого RPC код физически не может отличить «схема на 014» от
    «схема на 018» — именно поэтому 015–018 жили в дереве, но не в БД.

    Проверяется три вещи сразу: значение совпадает с последней миграцией,
    сервисная роль МОЖЕТ вызвать функцию, и НЕ МОЖЕТ прочитать сам реестр
    (least privilege, а не «выдали select на таблицу»).
    """
    dsn, names = _apply_full_chain(pg, "schemaversion")
    got = pg.scalar(dsn, "select public.schema_version()")
    assert got == names[-1], (
        f"schema_version() вернул {got!r}, ожидалась последняя миграция {names[-1]!r}"
    )
    assert pg.scalar(
        dsn,
        "select has_function_privilege('swarm_worker', "
        "'public.schema_version()', 'execute')",
    ) == "t", "swarm_worker не может вызвать schema_version() — гард в коде мёртв"
    assert pg.scalar(
        dsn,
        "select has_table_privilege('swarm_worker', "
        "'public.schema_migrations', 'select')",
    ) == "f", "реестр открыт сервисной роли: RPC перестал быть least privilege"


# ---------------------------------------------------------------------------
# Статическая часть: страж подключён и шапка не врёт.
# ---------------------------------------------------------------------------

def test_guard_is_wired_into_gate():
    """Страж, не подключённый в гейт, — украшение."""
    harness = HARNESS.read_text(encoding="utf-8")
    assert "tests/test_schema_chain_completeness.py" in harness, (
        "страж полноты схемы не подключён в scripts/acceptance_harness.sh"
    )
    allow = ALLOWLIST.read_text(encoding="utf-8")
    assert "tests/test_schema_chain_completeness.py" in allow, (
        "тест вне scope allowlist — правка файла уронит гейт"
    )


def test_schema_header_states_it_is_a_base_not_a_snapshot():
    """Шапка `schema_v0.sql` не должна называть файл источником истины.

    Именно эта формулировка привела к ошибочному выводу «в снимке нет
    task_parent_artifacts → свежий том останется без handoff». Файл — база,
    полная схема = база + миграции.
    """
    header = "\n".join(SCHEMA.read_text(encoding="utf-8").splitlines()[:12])
    assert "Единый источник истины" not in header, (
        "шапка schema_v0.sql снова называет файл единым источником истины"
    )
    assert "schema/migrations" in header, (
        "шапка schema_v0.sql не указывает, что полная схема = файл + миграции"
    )
