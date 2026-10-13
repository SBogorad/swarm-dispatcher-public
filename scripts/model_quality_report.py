#!/usr/bin/env python3
"""Issue #7/#10: статистика качества моделей → JSON для селектора воркера.

Читает view `llm_model_quality` (миграция 010) и печатает JSON-массив,
который воркер читает из `SWARM_MODEL_STATS_FILE`. Селектор
(`crates/swarm-worker/src/model_selector.rs`) выбирает модель по ИЗМЕРЕННОЙ
цене успешного результата (`cost_per_success_usd`), а не по названию.

Почему отдельный скрипт, а не запрос в воркере:
  - воркер не должен ходить в БД за статистикой на каждом вызове LLM;
  - статистика меняется медленно, её достаточно снимать периодически
    (cron/automation), а воркер читает готовый файл;
  - view закрыта правами (см. 010) и читается ролью-оператором.

Использование:
  SWARM_DSN=postgres://... scripts/model_quality_report.py            # JSON в stdout
  SWARM_DSN=postgres://... scripts/model_quality_report.py -o stats.json
  SWARM_DSN=... scripts/model_quality_report.py --min-samples 5       # только надёжные

Коды возврата:
  0 — успех (в т.ч. пустая статистика: это не ошибка, просто нет данных);
  2 — нет DSN / нет psycopg2 / ошибка БД.
"""

import argparse
import json
import os
import sys

try:
    import psycopg2
except ImportError:  # pragma: no cover - зависит от окружения
    print("ERROR: psycopg2 required (pip install psycopg2-binary)", file=sys.stderr)
    sys.exit(2)


def _dsn() -> str:
    d = os.environ.get("SWARM_DSN") or os.environ.get("DATABASE_URL")
    if not d:
        print("ERROR: SWARM_DSN or DATABASE_URL must be set", file=sys.stderr)
        sys.exit(2)
    return d


# Ровно то, что ждёт `ModelStat` в Rust (serde). Порядок и имена полей важны.
QUERY = """
select model,
       tasks,
       tasks_done,
       success_rate,
       total_cost_usd,
       cost_per_success_usd,
       avg_rework
  from public.llm_model_quality
 order by model
"""


def fetch_rows(dsn: str) -> list:
    conn = psycopg2.connect(dsn)
    try:
        with conn.cursor() as cur:
            cur.execute(QUERY)
            return cur.fetchall()
    finally:
        conn.close()


def to_stats(rows: list, min_samples: int) -> list:
    """Строки view → список словарей под `ModelStat`.

    Модели без единого успеха (`cost_per_success_usd is null`) пропускаем:
    цену успешного результата для них измерить нельзя, а подставлять ноль
    означало бы «бесплатная и всегда успешная» — прямо противоположное правде.
    """
    stats = []
    for (model, tasks, tasks_done, success_rate, total_cost, cost_per_success,
         avg_rework) in rows:
        if cost_per_success is None or tasks_done == 0:
            continue
        if tasks < min_samples:
            continue
        stats.append({
            "model": model,
            "tasks": int(tasks),
            "tasks_done": int(tasks_done),
            "success_rate": float(success_rate),
            "total_cost_usd": float(total_cost),
            "cost_per_success_usd": float(cost_per_success),
            "avg_rework": float(avg_rework),
        })
    return stats


def main() -> int:
    ap = argparse.ArgumentParser(
        description="Статистика качества моделей для селектора воркера (Issue #7/#10)")
    ap.add_argument("-o", "--output", help="файл для записи (по умолчанию stdout)")
    ap.add_argument("--min-samples", type=int, default=0,
                    help="не выводить модели с числом задач меньше N (по умолчанию 0)")
    ap.add_argument("--pretty", action="store_true", help="форматировать JSON")
    args = ap.parse_args()

    if args.min_samples < 0:
        print("ERROR: --min-samples не может быть отрицательным", file=sys.stderr)
        return 2

    try:
        rows = fetch_rows(_dsn())
    except Exception as exc:  # noqa: BLE001 - показываем причину оператору
        print(f"ERROR: не удалось прочитать llm_model_quality: {exc}", file=sys.stderr)
        return 2

    stats = to_stats(rows, args.min_samples)
    text = json.dumps(stats, ensure_ascii=False,
                      indent=2 if args.pretty else None)
    if args.output:
        with open(args.output, "w", encoding="utf-8") as fh:
            fh.write(text + "\n")
        print(f"записано {len(stats)} моделей в {args.output}", file=sys.stderr)
    else:
        print(text)
    return 0


if __name__ == "__main__":
    sys.exit(main())
