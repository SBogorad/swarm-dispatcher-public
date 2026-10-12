#!/usr/bin/env python3
"""Генератор типового суточного отчёта ЦГЦУ (демонстрационные данные).

Создаёт Excel с тремя листами, как в реальной суточной ведомости:
  * «Движение поездов» — номера поездов, станции отправления/назначения,
    нормативный и фактический срок доставки, отставание в часах, причина задержки.
  * «Персонал»        — наличие персонала на сменах (план / факт / неявки).
  * «Вагонооборот»    — прибытие/отправление вагонов, простой, сверхнорматив.

Генерация детерминирована (фиксированный seed), чтобы демонстрация была
повторяемой байт-в-байт при одном и том же seed.

Запуск:
    python3 demo/generator/gen_report.py --out demo/samples/
"""
from __future__ import annotations

import argparse
import random
from datetime import datetime, timedelta
from pathlib import Path

from openpyxl import Workbook
from openpyxl.styles import Alignment, Font

STATIONS = [
    "Красноярск-Восточный", "Ачинск-1", "Абакан", "Канск-Енисейский",
    "Лесосибирск", "Боготол", "Мариинск", "Тайшет", "Ужур", "Шарыпово",
]
DESTINATIONS = [
    "Красноярск-Восточный", "Новосибирск-Главный", "Иркутск-Сортировочный",
    "Абакан", "Тайшет", "Кемерово-Сортировочное",
]
# Первые три — пустые: большинство поездов идут по графику.
CAUSES = [
    "", "", "",
    "Неприём поезда станцией назначения",
    "Занятость путей станции",
    "Отказ технических средств",
    "Ожидание локомотивной бригады",
    "Проверка состава",
]
CARGO = ["уголь", "лес", "руда", "нефтепродукты", "металл", "контейнеры", "стройматериалы"]
DEPARTMENTS = ["ТЧЭ-1", "ТЧЭ-2", "ДС-Ачинск", "ДС-Абакан", "ДС-Канск", "ДС-Мариинск"]
SHIFTS = ["Смена 1 (00:00-08:00)", "Смена 2 (08:00-16:00)", "Смена 3 (16:00-24:00)"]


def _sheet_trains(wb: Workbook, rnd: random.Random, count: int) -> None:
    ws = wb.active
    ws.title = "Движение поездов"
    headers = [
        "Поезд", "Индекс", "Станция отправления", "Станция назначения", "Дорога",
        "Отправление", "Прибытие (план)", "Прибытие (факт)", "Норматив, ч",
        "Факт, ч", "Отставание, ч", "Вагоны", "Вес, т", "Груз", "Причина задержки",
    ]
    ws.append(headers)
    for c in ws[1]:
        c.font = Font(bold=True)
    base = datetime(2026, 10, 11, 0, 0, 0)
    for _ in range(count):
        dep = base + timedelta(minutes=rnd.randint(0, 1439))
        norm = rnd.choice([24, 26, 28, 30, 32, 36, 40])
        delay = max(0, int(rnd.gauss(2.5, 5.0)))
        fact = norm + delay
        cause = "" if delay < 1 else rnd.choice(CAUSES[3:])
        ws.append([
            str(rnd.randint(1000, 9999)),
            f"{rnd.randint(1, 9)}{rnd.randint(100, 999)}",
            rnd.choice(STATIONS),
            rnd.choice(DESTINATIONS),
            "Красноярская",
            dep.strftime("%Y-%m-%d %H:%M"),
            (dep + timedelta(hours=norm)).strftime("%Y-%m-%d %H:%M"),
            (dep + timedelta(hours=fact)).strftime("%Y-%m-%d %H:%M"),
            norm, fact, delay,
            rnd.randint(40, 71),
            rnd.randint(1800, 6300),
            rnd.choice(CARGO),
            cause,
        ])
    for col, width in zip("ABCDEFGHIJKLMNO",
                          [10, 10, 24, 24, 14, 17, 17, 17, 12, 10, 14, 10, 10, 14, 34]):
        ws.column_dimensions[col].width = width


def _sheet_staff(wb: Workbook, rnd: random.Random) -> None:
    ws = wb.create_sheet("Персонал")
    ws.append(["Подразделение", "Смена", "Штат по плану", "На смене (факт)", "Неявки", "Причина неявки"])
    for c in ws[1]:
        c.font = Font(bold=True)
    for d in DEPARTMENTS:
        for s in SHIFTS:
            plan = rnd.randint(8, 18)
            absent = rnd.choice([0, 0, 0, 1, 1, 2, 3])
            fact = plan - absent
            reason = "" if absent == 0 else rnd.choice(
                ["болезнь", "отпуск", "учебный отпуск", "по вызову"])
            ws.append([d, s, plan, fact, absent, reason])
    for col, width in zip("ABCDEF", [16, 22, 15, 15, 10, 18]):
        ws.column_dimensions[col].width = width


def _sheet_wagons(wb: Workbook, rnd: random.Random) -> None:
    ws = wb.create_sheet("Вагонооборот")
    ws.append(["Станция", "Прибыло вагонов", "Отправлено вагонов",
               "Средний простой, ч", "Норма простоя, ч", "Сверхнормативный простой, ч"])
    for c in ws[1]:
        c.font = Font(bold=True)
    for st in STATIONS:
        arrived = rnd.randint(120, 900)
        sent = arrived + rnd.randint(-40, 40)
        norm = rnd.choice([6.0, 8.0, 10.0, 12.0])
        avg = round(norm + rnd.uniform(-1.5, 6.0), 1)
        ws.append([st, arrived, sent, avg, norm, round(max(0.0, avg - norm) * arrived, 1)])
    for col, width in zip("ABCDEF", [24, 16, 18, 17, 16, 24]):
        ws.column_dimensions[col].width = width


def generate(out_dir: Path, trains: int = 42, seed: int = 20261011,
             name: str = "cgu_daily_report.xlsx") -> Path:
    rnd = random.Random(seed)
    wb = Workbook()
    _sheet_trains(wb, rnd, trains)
    _sheet_staff(wb, rnd)
    _sheet_wagons(wb, rnd)
    # Заголовок-шапка отчёта на первом листе: строки 1..2 сдвигаются? Нет —
    # оставляем таблицу с первой строки, чтобы pandas читал её как header.
    out_dir.mkdir(parents=True, exist_ok=True)
    path = out_dir / name
    wb.save(path)
    return path


def main() -> None:
    ap = argparse.ArgumentParser(description="Генератор суточного отчёта ЦГЦУ")
    ap.add_argument("--out", default="demo/samples/", help="каталог для файла")
    ap.add_argument("--trains", type=int, default=42, help="число поездов")
    ap.add_argument("--seed", type=int, default=20261011, help="seed (детерминизм)")
    ap.add_argument("--name", default="cgu_daily_report.xlsx")
    args = ap.parse_args()
    p = generate(Path(args.out), args.trains, args.seed, args.name)
    print(f"[ok] отчёт сформирован: {p} ({p.stat().st_size} байт)")


if __name__ == "__main__":
    main()
