"""Детерминированный (rule-based) анализ Excel-отчёта ЦГЦУ.

Отвечает за точные цифры: сколько поездов с отставанием, где задержки,
какие причины, как с персоналом и вагонооборотом. Работает локально (pandas),
без обращения к сети — это и есть «локальный контур».

Результат — словарь метрик, который кладётся в контракт задачи для роя
(воркер формулирует по нему управленческую записку) и используется как
гарантированный fallback, когда LLM недоступна.
"""
from __future__ import annotations

from pathlib import Path
from typing import Any

import pandas as pd

SHEET_TRAINS = "Движение поездов"
SHEET_STAFF = "Персонал"
SHEET_WAGONS = "Вагонооборот"


def _num(series: pd.Series) -> pd.Series:
    return pd.to_numeric(series, errors="coerce").fillna(0)


def analyze(path: str | Path) -> dict[str, Any]:
    """Вернуть структурированные метрики по всем трём разрезам отчёта."""
    xl = pd.ExcelFile(path)
    metrics: dict[str, Any] = {"source_file": Path(path).name}

    if SHEET_TRAINS in xl.sheet_names:
        t = xl.parse(SHEET_TRAINS)
        t.columns = [str(c).strip() for c in t.columns]
        delay = _num(t["Отставание, ч"])
        delayed = t[delay >= 1]
        metrics["trains"] = {
            "total": int(len(t)),
            "delayed": int(len(delayed)),
            "on_schedule": int(len(t) - len(delayed)),
            "avg_delay_h": round(float(delay[delay >= 1].mean()), 2) if len(delayed) else 0.0,
            "max_delay_h": int(delay.max()) if len(t) else 0,
            "total_delay_h": int(delay.sum()),
            "by_station": _top(delayed, "Станция назначения", delay),
            "by_cause": _top(delayed, "Причина задержки", delay),
            "worst": _worst(delayed, delay),
        }

    if SHEET_STAFF in xl.sheet_names:
        s = xl.parse(SHEET_STAFF)
        s.columns = [str(c).strip() for c in s.columns]
        plan, fact = _num(s["Штат по плану"]), _num(s["На смене (факт)"])
        shortage = s[fact < plan]
        metrics["staff"] = {
            "shifts": int(len(s)),
            "plan_total": int(plan.sum()),
            "fact_total": int(fact.sum()),
            "absent_total": int(_num(s["Неявки"]).sum()),
            "shifts_with_shortage": int(len(shortage)),
            "shortage": [
                {"department": r["Подразделение"], "shift": r["Смена"],
                 "plan": int(r["Штат по плану"]), "fact": int(r["На смене (факт)"]),
                 "absent": int(r["Неявки"]), "reason": r.get("Причина неявки", "")}
                for _, r in shortage.iterrows()
            ][:10],
        }

    if SHEET_WAGONS in xl.sheet_names:
        w = xl.parse(SHEET_WAGONS)
        w.columns = [str(c).strip() for c in w.columns]
        over = _num(w["Сверхнормативный простой, ч"])
        metrics["wagons"] = {
            "stations": int(len(w)),
            "arrived_total": int(_num(w["Прибыло вагонов"]).sum()),
            "sent_total": int(_num(w["Отправлено вагонов"]).sum()),
            "over_norm_total_h": round(float(over.sum()), 1),
            "avg_dwell_h": round(float(_num(w["Средний простой, ч"]).mean()), 2),
            "worst": [
                {"station": r["Станция"], "avg_dwell_h": float(r["Средний простой, ч"]),
                 "norm_h": float(r["Норма простоя, ч"]),
                 "over_h": float(r["Сверхнормативный простой, ч"])}
                for _, r in w.assign(_o=over).sort_values("_o", ascending=False).head(5).iterrows()
            ],
        }
    return metrics


def _top(df: pd.DataFrame, col: str, weights: pd.Series) -> list[dict]:
    if col not in df.columns or df.empty:
        return []
    g = df.assign(_w=weights.loc[df.index].values).groupby(col)["_w"].agg(["count", "sum"])
    g = g.sort_values("sum", ascending=False).head(8)
    return [{"name": str(k), "trains": int(r["count"]), "delay_h": int(r["sum"])}
            for k, r in g.iterrows()]


def _worst(df: pd.DataFrame, delay: pd.Series) -> list[dict]:
    if df.empty:
        return []
    top = df.assign(_d=delay.loc[df.index].values).sort_values("_d", ascending=False).head(5)
    return [{"train": str(r["Поезд"]), "from": str(r["Станция отправления"]),
             "to": str(r["Станция назначения"]), "delay_h": int(r["_d"]),
             "cause": str(r.get("Причина задержки", ""))} for _, r in top.iterrows()]


if __name__ == "__main__":
    import json
    import sys
    print(json.dumps(analyze(sys.argv[1]), ensure_ascii=False, indent=2))
