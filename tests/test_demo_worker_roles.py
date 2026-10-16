"""Guard: демо-воркеры подключаются под СВОИМИ ролями БД, а не под суперпользователем.

Зачем (2026-10-16, PR #4): в `docker-compose.demo.yml` все воркеры ходили в
Postgres как суперпользователь `swarm`. `assert_worker_identity` (F3) намеренно
пропускает суперпользователя, поэтому в демо-контуре проверка identity
**обходилась**: подмена `WORKER_ID` не ловилась, а демо переставало быть честной
витриной защиты. См. `docs/KNOWN_LIMITATIONS.md` §5.12.

Правило (то же, что в продовом `docker-compose.yml`): для сервиса `worker-N`
пользователь DSN обязан равняться `swarm_worker_<normalize(WORKER_ID)>`, где
`normalize` — как в SQL (`step2b/roles_2b.sql`): нижний регистр, `[-. ]` → `_`.

Тест НЕ требует БД: чистый разбор compose-файла. Запуск:
  .venv/bin/python -m pytest -q tests/test_demo_worker_roles.py
"""
import re
from pathlib import Path

REPO = Path(__file__).resolve().parents[1]
DEMO = REPO / "docker-compose.demo.yml"
PROD = REPO / "docker-compose.yml"

SERVICE_RE = re.compile(r"^  ([A-Za-z0-9_-]+):\s*$")
DSN_RE = re.compile(r"^\s*DATABASE_URL:\s*postgres://([^:@/]+):")
WORKER_ID_RE = re.compile(r"^\s*WORKER_ID:\s*(\S+)\s*$")


def normalize_worker_id(worker_id: str) -> str:
    """Аналог SQL `normalize_worker_id`: `[-. ]` → `_`, нижний регистр."""
    return re.sub(r"[-. ]", "_", worker_id).lower()


def _services(path: Path):
    """Разбор compose на {service: {KEY: value}} без внешнего YAML-парсера.

    Берём только то, что нужно: имя сервиса и его переменные окружения.
    """
    services: dict[str, dict[str, str]] = {}
    current = None
    for line in path.read_text(encoding="utf-8").splitlines():
        m = SERVICE_RE.match(line)
        if m:
            current = m.group(1)
            services.setdefault(current, {})
            continue
        if current is None:
            continue
        m = DSN_RE.match(line)
        if m:
            services[current]["DATABASE_URL_USER"] = m.group(1)
            continue
        m = WORKER_ID_RE.match(line)
        if m:
            services[current]["WORKER_ID"] = m.group(1)
    return services


def _worker_services(path: Path):
    return {n: e for n, e in _services(path).items() if n.startswith("worker-")}


def test_demo_has_worker_services():
    """Якорь: если сервисы переименуют, тест обязан упасть, а не стать пустым."""
    workers = _worker_services(DEMO)
    assert len(workers) >= 2, f"в demo найдено воркеров: {len(workers)}"


def test_demo_workers_use_their_own_role():
    """Каждый демо-воркер: DSN-пользователь == `swarm_worker_<normalize(WORKER_ID)>`."""
    bad = []
    for name, env in sorted(_worker_services(DEMO).items()):
        user = env.get("DATABASE_URL_USER")
        wid = env.get("WORKER_ID")
        assert user, f"{name}: не найден DATABASE_URL"
        assert wid, f"{name}: не найден WORKER_ID"
        if user == "swarm":
            bad.append(f"{name}: подключён как суперпользователь `swarm` — identity обходится")
            continue
        expected = "swarm_worker_" + normalize_worker_id(wid)
        if user != expected:
            bad.append(f"{name}: WORKER_ID={wid} → ожидалась роль {expected}, а в DSN {user}")
    assert not bad, "демо-воркеры вне изоляции ролей:\n  " + "\n  ".join(bad)


def test_prod_compose_still_uses_per_worker_roles():
    """Продовый контур — эталон: его воркеры тоже обязаны быть под своими ролями.

    Демо-фикс не должен маскировать регрессию в проде (и наоборот).
    """
    bad = []
    for name, env in sorted(_worker_services(PROD).items()):
        user = env.get("DATABASE_URL_USER")
        wid = env.get("WORKER_ID")
        assert user, f"{name}: не найден DATABASE_URL"
        assert wid, f"{name}: не найден WORKER_ID"
        expected = "swarm_worker_" + normalize_worker_id(wid)
        if user != expected:
            bad.append(f"{name}: WORKER_ID={wid} → ожидалась роль {expected}, а в DSN {user}")
    assert not bad, "прод-воркеры вне изоляции ролей:\n  " + "\n  ".join(bad)
