"""Streamlit-кабина «ЦГЦУ Ассистент».

Экран для руководства: загрузить Excel, задать вопрос, увидеть жизненный цикл
задачи в реальном времени и получить готовую справку.

UI ничего не считает сам — он общается с FastAPI-мостом, который ставит задачу
в рой. Вся аналитика и текст справки приходят от движка.
"""
from __future__ import annotations

import os
import time

import requests
import streamlit as st

API = os.environ.get("DEMO_API_URL", "http://127.0.0.1:8080").rstrip("/")

PRESETS = {
    "Справка по задержкам": (
        "Сформируй справку по поездам с нарушением графика движения и просрочкой "
        "доставки. Укажи станции задержки и причины."
    ),
    "Наличие персонала на сменах": (
        "Сформируй справку по наличию персонала на сменах: где недокомплект, "
        "сколько человек не вышло и по каким причинам."
    ),
    "Сводка по вагонообороту": (
        "Сформируй сводку по вагонообороту: прибытие/отправление вагонов, "
        "сверхнормативный простой и станции с наибольшим простоем."
    ),
}

STEPS = [
    ("pending", "Постановка задачи"),
    ("ready", "Планировщик: задача к исполнению"),
    ("leased", "Воркер: анализ отчёта"),
    ("review", "Аудит контракта"),
    ("done", "Готово"),
]
ORDER = {s: i for i, (s, _) in enumerate(STEPS)}

st.set_page_config(page_title="ЦГЦУ Ассистент", page_icon="🚂", layout="wide")

st.title("ЦГЦУ Ассистент")
st.caption("Аналитика выполняется локально в контуре, данные не покидают сеть")

if "task_id" not in st.session_state:
    st.session_state.task_id = None

with st.sidebar:
    st.header("Запрос")
    uploaded = st.file_uploader("Суточный отчёт (Excel)", type=["xlsx", "xls"])
    st.write("Пресеты:")
    preset = None
    for name in PRESETS:
        if st.button(name, use_container_width=True):
            preset = name
    question = st.text_area(
        "Вопрос к отчёту", value=PRESETS["Справка по задержкам"], height=120)
    if preset:
        question = PRESETS[preset]
    run = st.button("▶ Сформировать справку", type="primary", use_container_width=True)

if run:
    if uploaded is None:
        st.error("Сначала загрузите Excel-файл отчёта.")
    else:
        try:
            r = requests.post(
                f"{API}/tasks",
                files={"file": (uploaded.name, uploaded.getvalue())},
                data={"question": question},
                timeout=30,
            )
            r.raise_for_status()
            st.session_state.task_id = r.json()["task_id"]
        except Exception as e:  # noqa: BLE001
            st.error(f"Не удалось поставить задачу: {e}")

task_id = st.session_state.task_id
if task_id:
    st.subheader("Жизненный цикл задачи")
    status_box = st.empty()
    kanban = st.empty()
    events_box = st.empty()
    result_box = st.container()

    data = None
    for _ in range(120):  # до ~2 минут
        try:
            r = requests.get(f"{API}/tasks/{task_id}", timeout=15)
            r.raise_for_status()
            data = r.json()
        except Exception as e:  # noqa: BLE001
            status_box.warning(f"Ожидание моста… {e}")
            time.sleep(1)
            continue

        state = data["state"]
        cur = ORDER.get(state, 0)
        chips = []
        for i, (s, label) in enumerate(STEPS):
            mark = "✅" if i < cur else ("🔄" if i == cur else "⬜")
            chips.append(f"{mark} {label}")
        kanban.markdown("  →  ".join(chips))
        elapsed = data.get("elapsed_sec")
        status_box.info(
            f"Состояние: **{state}** · попыток: {data['attempts']} · "
            f"rework: {data['rework']} · токенов: {data['spent_tokens']}"
            + (f" · время: {elapsed} с" if elapsed is not None else ""))
        with events_box.expander("Журнал событий (hash-цепочка движка)"):
            for e in data["events"]:
                st.text(f"{e['seq']:>3}  {e['at'][11:19]}  {e['kind']:<20} {e['actor']}")

        if data["terminal"]:
            break
        time.sleep(1)

    if data and data["state"] == "done" and data.get("memo"):
        with result_box:
            st.subheader("Управленческая справка")
            st.markdown(data["memo"])
            st.download_button(
                "Скачать справку (.md)", data=data["memo"],
                file_name=f"memo_{task_id[:8]}.md", mime="text/markdown")
            st.success(f"Готово за {data.get('elapsed_sec')} с. "
                       "Вычисление выполнено локально в контуре.")
    elif data and data["state"] == "blocked":
        st.error(f"Задача заблокирована: {data.get('block_reason')}")
