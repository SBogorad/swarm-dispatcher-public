"""Swarm Ops Console — кабина координации роя (Streamlit).

Экран для инженеров, архитекторов и руководства: поставить цель, увидеть, как
движок декомпозирует её в граф задач, как задачи проходят
`pending → ready → leased → review → done`, кто из воркеров что держит и что
записано в неизменяемом журнале (хэш-цепочка).

UI ничего не считает сам. Он читает витрину движка через FastAPI-мост:
канбан — из `tasks`, воркеры — из `workers`, журнал — из `events` с хэшами.
"""
from __future__ import annotations

import os
import time

import requests
import streamlit as st

API = os.environ.get("DEMO_API_URL", "http://127.0.0.1:8080").rstrip("/")

PRESETS = {
    "Рефакторинг модуля": (
        "Проанализировать модуль авторизации, составить план рефакторинга, "
        "написать юнит-тесты и провести аудит безопасности."
    ),
    "Разбор инцидента": (
        "Разобрать инцидент в проде: собрать логи, найти первопричину, "
        "предложить устранение и оформить постмортем."
    ),
    "Подготовка релиза": (
        "Подготовить релиз: проверить зависимости, собрать артефакты, "
        "прогнать приёмочные проверки и оформить заметки о выпуске."
    ),
}

COLUMNS = [
    ("pending", "Pending", "⏳"),
    ("ready", "Ready", "🟢"),
    ("leased", "In Progress", "🔄"),
    ("review", "Under Review", "🔎"),
    ("done", "Done", "✅"),
    ("blocked", "Blocked", "⛔"),
]
ORDER = [c for c, _, _ in COLUMNS]

st.set_page_config(page_title="Swarm Ops Console", page_icon="🐝", layout="wide")


def _api_get(path: str, timeout: float = 10.0):
    r = requests.get(f"{API}{path}", timeout=timeout)
    r.raise_for_status()
    return r.json()


def _short(uid: str, n: int = 8) -> str:
    return (uid or "")[:n]


st.title("🐝 Swarm Ops Console")
st.caption("Координация автономных агентов · состояния, аренды и журнал — из движка")

if "root_id" not in st.session_state:
    st.session_state.root_id = None
if "selected" not in st.session_state:
    st.session_state.selected = None

with st.sidebar:
    st.header("Постановка цели")
    goal = st.text_area(
        "Цель для роя", value=PRESETS["Рефакторинг модуля"], height=140)
    st.write("Пресеты:")
    for name, text in PRESETS.items():
        if st.button(name, use_container_width=True):
            goal = text
    run = st.button("▶ Запустить рой", type="primary", use_container_width=True)
    st.divider()
    auto = st.toggle("Автообновление", value=True)

if run:
    if not (goal or "").strip():
        st.error("Введите цель.")
    else:
        try:
            r = requests.post(f"{API}/goals", data={"goal": goal}, timeout=30)
            r.raise_for_status()
            body = r.json()
            st.session_state.root_id = body["root_id"]
            st.session_state.selected = None
            if body.get("already_existed"):
                st.info("Граф для этой цели уже существует — показываю его.")
            else:
                st.success(f"Движок принял граф из {body['tasks_created']} задач.")
        except Exception as e:  # noqa: BLE001
            st.error(f"Не удалось поставить цель: {e}")

try:
    board = _api_get("/board")
except Exception as e:  # noqa: BLE001
    st.error(f"Мост недоступен: {e}")
    st.stop()

# ---- Панель узлов (Workers) -------------------------------------------------
st.subheader("Узлы роя")
if board["workers"]:
    wcols = st.columns(min(4, max(1, len(board["workers"]))))
    for i, w in enumerate(board["workers"]):
        caps = w.get("capabilities") or {}
        ram = caps.get("ram_gb", "?")
        cls = (caps.get("models") or {}).get("coder_class", "?")
        seen = (w.get("last_seen") or "")[11:19]
        with wcols[i % len(wcols)]:
            st.metric(label=f"🧩 {w['id']}", value=f"аренд: {w['leased']}",
                      delta=f"RAM {ram} · класс {cls}")
            st.caption(f"последняя активность: {seen} UTC")
else:
    st.info("Воркеры ещё не регистрировались (нет строк в `workers`).")

st.divider()

# ---- Канбан -----------------------------------------------------------------
st.subheader("Канбан задач роя")
counts = board["counts"]
head = st.columns(len(COLUMNS))
for col, (state, label, icon) in zip(head, COLUMNS):
    col.metric(f"{icon} {label}", counts.get(state, 0))

tasks = board["tasks"]
by_state: dict[str, list] = {c: [] for c in ORDER}
for t in tasks:
    by_state.setdefault(t["state"], []).append(t)

cols = st.columns(len(COLUMNS))
for col, (state, label, icon) in zip(cols, COLUMNS):
    with col:
        items = by_state.get(state, [])
        for t in items:
            holder = t["lease_owner"] or "—"
            deps = f"{t['deps_total']}"
            cap = f"{t['spent_tokens']}/{t['budget_cap']}"
            with st.container(border=True):
                st.markdown(f"**{t['kind']}** · `{_short(t['id'])}`")
                st.caption(t["description"][:90] or "—")
                st.caption(f"держит: {holder} · эпоха {t['lease_epoch']} · "
                           f"зависимостей {deps} · токены {cap}")
                if st.button("Открыть", key=f"open_{t['id']}", use_container_width=True):
                    st.session_state.selected = t["id"]

st.divider()

# ---- Карточка задачи --------------------------------------------------------
tid = st.session_state.selected
if tid:
    try:
        d = _api_get(f"/tasks/{tid}")
    except Exception as e:  # noqa: BLE001
        st.error(f"Карточка недоступна: {e}")
        d = None

    if d:
        contract = d.get("contract") or {}
        st.subheader(f"Карточка задачи · `{_short(tid)}`")
        m = st.columns(5)
        m[0].metric("Состояние", d["state"])
        m[1].metric("Держатель", d["lease_owner"] or "—")
        m[2].metric("Эпоха аренды", d["lease_epoch"])
        m[3].metric("Токены", f"{d['spent_tokens']}/{d['budget_cap']}")
        m[4].metric("Время", f"{d['elapsed_sec']} с" if d["elapsed_sec"] is not None else "—")
        st.caption(f"Вид: `{contract.get('kind', '—')}` · "
                   f"постановка: {contract.get('description', '—')}")

        c1, c2 = st.columns(2)
        with c1:
            st.markdown("**Контракт и проверки**")
            st.json({"kind": contract.get("kind"),
                     "checks": contract.get("checks"),
                     "needs": contract.get("needs"),
                     "budget": contract.get("budget")})
        with c2:
            st.markdown("**Аудит (хэш-цепочка)**")
            valid = d.get("chain_valid")
            if valid is True:
                st.success("verify_chain: цепочка целостна ✅")
            elif valid is False:
                st.error("verify_chain: ЦЕПОЧКА ПОВРЕЖДЕНА ❌")
            else:
                st.caption("verify_chain недоступен")
            for c in d.get("chain", []):
                st.code(f"{c['seq']:>3} {c['at'][:19]} {c['kind']}\n"
                        f"    hash {c['hash'][:16]}… prev {str(c['prev_hash'])[:16]}…",
                        language=None)

        if d.get("reviews"):
            st.markdown("**Ревью**")
            for rv in d["reviews"]:
                st.caption(f"{rv['reviewer']} · {rv['verdict']} · проверок "
                           f"{len(rv['checks'] or [])}")

        if d.get("artifacts"):
            st.markdown("**Артефакт**")
            try:
                r = requests.get(f"{API}/tasks/{tid}/artifact", timeout=15)
                if r.ok:
                    st.markdown(r.text)
                    st.download_button("Скачать артефакт (.md)", data=r.text,
                                       file_name=f"{_short(tid)}.md",
                                       mime="text/markdown")
            except Exception as e:  # noqa: BLE001
                st.warning(f"Артефакт не получен: {e}")

        if d["state"] == "blocked":
            st.error(f"Заблокирована: {d.get('block_reason')}")

st.divider()

# ---- Живой протокол ---------------------------------------------------------
st.subheader("Протокол координации (журнал движка)")
all_events: list[tuple] = []
for t in tasks:
    try:
        card = _api_get(f"/tasks/{t['id']}")
    except Exception:  # noqa: BLE001
        continue
    for e in card.get("events", []):
        all_events.append((e["at"], e["kind"], e["actor"], _short(t["id"])))
all_events.sort(reverse=True)
for at, kind, actor, short in all_events[:30]:
    st.text(f"{at[:19]}  {kind:<18} {str(actor or '—'):<12} {short}")

if auto:
    time.sleep(2)
    st.rerun()
