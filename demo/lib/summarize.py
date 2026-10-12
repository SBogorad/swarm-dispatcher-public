"""Формирование управленческой записки по метрикам отчёта.

Два режима, как требует ТЗ демо-стенда:
  1. Локальная LLM (Ollama, OpenAI-совместимый endpoint) — если доступна.
  2. Детерминированный rule-based шаблон — гарантированная демонстрация
     без сети и без модели.

Оба режима дают готовую справку на русском; в тексте честно указан источник.
"""
from __future__ import annotations

import json
import os
import urllib.error
import urllib.request
from typing import Any


def _fmt_trains(m: dict[str, Any]) -> list[str]:
    t = m.get("trains")
    if not t:
        return []
    lines = [
        f"* Всего проследовало поездов: **{t['total']}**, из них с нарушением графика — **{t['delayed']}** "
        f"({round(100 * t['delayed'] / t['total'], 1)}%).",
        f"* Среднее отставание по задержанным: **{t['avg_delay_h']} ч**, максимальное — **{t['max_delay_h']} ч**, "
        f"суммарно **{t['total_delay_h']} ч**.",
    ]
    if t.get("by_station"):
        lines.append("* Станции с наибольшим отставанием:")
        for s in t["by_station"][:5]:
            lines.append(f"    - {s['name']}: {s['trains']} поездов, {s['delay_h']} ч отставания.")
    if t.get("by_cause"):
        lines.append("* Основные причины задержек:")
        for c in t["by_cause"][:4]:
            lines.append(f"    - {c['name']}: {c['trains']} случаев, {c['delay_h']} ч.")
    if t.get("worst"):
        lines.append("* Наибольшие задержки по поездам:")
        for w in t["worst"][:3]:
            lines.append(
                f"    - поезд {w['train']} ({w['from']} → {w['to']}): {w['delay_h']} ч, "
                f"причина: {w['cause'] or 'не указана'}.")
    return lines


def _fmt_staff(m: dict[str, Any]) -> list[str]:
    s = m.get("staff")
    if not s:
        return []
    lines = [
        f"* Смен проанализировано: **{s['shifts']}**; по плану **{s['plan_total']}** человек, "
        f"фактически на сменах **{s['fact_total']}** (неявок — {s['absent_total']}).",
    ]
    if s.get("shifts_with_shortage"):
        lines.append(f"* Смен с недокомплектом: **{s['shifts_with_shortage']}**.")
        for r in s["shortage"][:5]:
            lines.append(
                f"    - {r['department']}, {r['shift']}: план {r['plan']} / факт {r['fact']} "
                f"(неявка: {r['reason'] or 'без причины'}).")
    else:
        lines.append("* Все смены укомплектованы по плану.")
    return lines


def _fmt_wagons(m: dict[str, Any]) -> list[str]:
    w = m.get("wagons")
    if not w:
        return []
    lines = [
        f"* Прибыло вагонов: **{w['arrived_total']}**, отправлено: **{w['sent_total']}**.",
        f"* Средний простой: **{w['avg_dwell_h']} ч**; сверхнормативный простой суммарно "
        f"**{w['over_norm_total_h']} ч**.",
    ]
    if w.get("worst"):
        lines.append("* Станции со сверхнормативным простоем:")
        for s in w["worst"][:3]:
            lines.append(
                f"    - {s['station']}: средний простой {s['avg_dwell_h']} ч при норме {s['norm_h']} ч "
                f"(сверхнорматив {s['over_h']} ч).")
    return lines


def rule_based(metrics: dict[str, Any], question: str = "") -> str:
    """Гарантированная справка без обращения к LLM."""
    head = ["# Аналитическая справка", ""]
    if question:
        head += [f"**Запрос:** {question}", ""]
    head += [f"**Источник данных:** {metrics.get('source_file', '—')}", ""]
    body: list[str] = []
    for section, fn in (("Движение поездов и просрочка доставки", _fmt_trains),
                        ("Наличие персонала на сменах", _fmt_staff),
                        ("Вагонооборот и простой", _fmt_wagons)):
        lines = fn(metrics)
        if lines:
            body += [f"## {section}", ""] + lines + [""]
    body += [
        "## Вывод",
        "",
        "Основные потери связаны с задержками поездов на подходах к станциям "
        "назначения и со сверхнормативным простоем вагонов. Рекомендуется "
        "проверить диспетчеризацию станций с наибольшим отставанием и "
        "укомплектованность смен с недокомплектом.",
        "",
        "_Справка сформирована детерминированным аналитическим модулем "
        "(rule-based), сеть не использовалась._",
    ]
    return "\n".join(head + body)


def _ollama_reachable(base: str, timeout: float) -> bool:
    """Быстрая проверка доступности Ollama до дорогого HTTP-запроса.

    Без этого недостижимый endpoint (например, Ollama слушает только
    loopback хоста, а воркер — в контейнере) заставляет ждать полный
    DEMO_OLLAMA_TIMEOUT (по умолчанию 90 с) и только затем откатываться на
    rule-based. Наблюдалось на стенде: задача шла ~94 с вместо ~2 с.
    TCP-проба с коротким таймаутом делает откат почти мгновенным.
    """
    import socket
    from urllib.parse import urlparse
    u = urlparse(base)
    host = u.hostname or "127.0.0.1"
    port = u.port or (443 if u.scheme == "https" else 80)
    try:
        with socket.create_connection((host, port), timeout=timeout):
            return True
    except OSError:
        return False


def _ollama_summary(metrics: dict[str, Any], question: str) -> str | None:
    """Запрос к локальной Ollama (OpenAI-совместимый /v1/chat/completions)."""
    base = os.environ.get("DEMO_OLLAMA_URL", "http://127.0.0.1:11434").rstrip("/")
    model = os.environ.get("DEMO_OLLAMA_MODEL", "deepseek-coder:6.7b")
    timeout = float(os.environ.get("DEMO_OLLAMA_TIMEOUT", "90"))
    probe = float(os.environ.get("DEMO_OLLAMA_PROBE_SEC", "2"))
    if not _ollama_reachable(base, probe):
        return None  # мгновенный откат на rule-based, без ожидания таймаута
    system = (
        "Ты — аналитик Центра управления перевозками. По переданным метрикам "
        "сформируй краткую управленческую справку на русском языке: цифры, "
        "станции, причины, вывод и рекомендации. Не выдумывай данные, "
        "используй только предоставленные метрики."
    )
    user = (f"Запрос: {question or 'Общая сводка'}\n\n"
            f"Метрики (JSON):\n{json.dumps(metrics, ensure_ascii=False)}")
    payload = json.dumps({
        "model": model,
        "messages": [{"role": "system", "content": system},
                     {"role": "user", "content": user}],
        "temperature": 0.2,
        "max_tokens": 900,
    }).encode("utf-8")
    req = urllib.request.Request(
        f"{base}/v1/chat/completions", data=payload,
        headers={"Content-Type": "application/json"}, method="POST")
    try:
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            data = json.loads(resp.read().decode("utf-8"))
        return data["choices"][0]["message"]["content"].strip()
    except (urllib.error.URLError, KeyError, TimeoutError, OSError, ValueError):
        return None


def summarize(metrics: dict[str, Any], question: str = "",
              prefer_llm: bool = True) -> dict[str, Any]:
    """Вернуть {text, engine}. engine: 'ollama' | 'rule-based'."""
    if prefer_llm:
        text = _ollama_summary(metrics, question)
        if text:
            return {"text": text, "engine": "ollama"}
    return {"text": rule_based(metrics, question), "engine": "rule-based"}


if __name__ == "__main__":
    import sys
    from analyze import analyze  # локальный запуск: python demo/bridge/summarize.py <файл>
    m = analyze(sys.argv[1])
    print(summarize(m, "Сводка по задержкам")["text"])
