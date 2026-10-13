#!/usr/bin/env python3
"""Issue #7/#10: каталог моделей для селектора воркера — из живого прайса relay.fast.

Тянет `GET /api/pricing`, пересчитывает прайс в USD за 1M токенов и пишет
JSON-каталог, который воркер читает из `SWARM_MODEL_CATALOG_FILE`.

Формула (сверена с `/api/log/token` по факту, см. README):
    input_usd_per_1M  = model_ratio * group_ratio * 2
    output_usd_per_1M = input_usd_per_1M * completion_ratio

Почему 2: в `/api/pricing` базовая единица — `group_ratio` при `model_ratio=1`
соответствует $0.01/1M, а цена считается как `model_ratio * 2 * group_ratio`
(для deepseek-v4-flash: 0.15 * 0.05 * 2 = 0.015, что совпадает с нашим ledger
и с `quota/500000` из `/api/log/token`).

Использование:
  RELAY_API_KEY=sk-... scripts/fetch_model_catalog.py            # JSON в stdout
  RELAY_API_KEY=sk-... scripts/fetch_model_catalog.py -o catalog.json
  scripts/fetch_model_catalog.py --api-key sk-... --include deepseek qwen

Коды возврата:
  0 — успех; 2 — нет ключа / сеть недоступна / прайс не распознан.
"""

import argparse
import json
import os
import sys
import urllib.error
import urllib.request

DEFAULT_BASE = "https://relay.fast"
PRICING_PATH = "/api/pricing"

# Уровень качества по модели (эвристика, переопределяется руками в каталоге).
# Смысл: задача уровня T уходит только модели с quality_tier >= T. Слабые
# flash-модели не должны брать сложные задачи — они жгут ретраи и дороже мощных.
# Порядок важен: сначала специфичные суффиксы класса (luna/mini/flash), потом
# префиксы семейств, иначе `gpt-6-luna` (дешёвый) получал бы tier мощного `gpt-6`.
QUALITY_BY_HINT = [
    ("luna", 2), ("mini", 2), ("flash", 2), ("kimi", 2), ("code", 2),
    ("gemini", 2),
    ("pro", 3), ("sonnet", 3), ("opus", 3), ("sol", 3), ("astra", 3),
    ("grok", 3), ("gpt-5", 3), ("gpt-6", 3),
]


def quality_tier_for(model: str) -> int:
    m = model.lower()
    for hint, tier in QUALITY_BY_HINT:
        if hint in m:
            return tier
    return 1


def fetch_pricing(base_url: str, api_key: str, timeout: int = 20) -> dict:
    url = base_url.rstrip("/") + PRICING_PATH
    req = urllib.request.Request(url, headers={"Authorization": f"Bearer {api_key}"})
    with urllib.request.urlopen(req, timeout=timeout) as resp:  # noqa: S310 - доверенный хост
        return json.loads(resp.read().decode("utf-8"))


def to_catalog(pricing: dict, include: list, exclude: list, only_chat: bool) -> list:
    """Прайс relay.fast -> список моделей для селектора.

    Только chat-модели (`supported_endpoint_types` содержит openai/anthropic):
    генерация картинок и прочее к выбору модели воркера отношения не имеют.
    """
    models = []
    for m in pricing.get("data", []):
        name = (m.get("model_name") or "").strip()
        # Имя должно содержать хотя бы один алфанум: в прайсе встречаются
        # записи с невидимыми символами (zero-width) и без реального имени —
        # они не модели, а мусор разметки.
        if not any(ch.isalnum() for ch in name):
            continue
        endpoints = m.get("supported_endpoint_types") or []
        if only_chat and not ({"openai", "anthropic"} & set(endpoints)):
            continue
        # Модели с фиксированной ценой за штуку (картинки) пропускаем.
        if m.get("model_price"):
            continue
        group_ratio = (m.get("group_ratio") or {}).get("default")
        model_ratio = m.get("model_ratio")
        completion_ratio = m.get("completion_ratio")
        if group_ratio is None or model_ratio is None or completion_ratio is None:
            continue
        input_usd = float(model_ratio) * float(group_ratio) * 2.0
        output_usd = input_usd * float(completion_ratio)
        if input_usd <= 0 or output_usd <= 0:
            continue
        if include and not any(h in name for h in include):
            continue
        if exclude and any(h in name for h in exclude):
            continue
        models.append({
            "name": name,
            "input_usd_per_1m": round(input_usd, 6),
            "output_usd_per_1m": round(output_usd, 6),
            "quality_tier": quality_tier_for(name),
            "enabled": True,
        })
    models.sort(key=lambda x: x["input_usd_per_1m"])
    return models


def main() -> int:
    ap = argparse.ArgumentParser(
        description="Каталог моделей из прайса relay.fast (Issue #7/#10)")
    ap.add_argument("-o", "--output", help="файл для записи (по умолчанию stdout)")
    ap.add_argument("--api-key", help="ключ relay.fast (по умолчанию RELAY_API_KEY)")
    ap.add_argument("--base-url", default=os.environ.get("RELAY_BASE_URL", DEFAULT_BASE))
    ap.add_argument("--include", nargs="*", default=[], help="оставить модели с этими подстроками")
    ap.add_argument("--exclude", nargs="*", default=[], help="убрать модели с этими подстроками")
    ap.add_argument("--all-endpoints", action="store_true",
                    help="не фильтровать по chat-эндпоинтам")
    ap.add_argument("--pretty", action="store_true", help="форматировать JSON")
    args = ap.parse_args()

    key = args.api_key or os.environ.get("RELAY_API_KEY")
    if not key:
        print("ERROR: --api-key или RELAY_API_KEY обязателен", file=sys.stderr)
        return 2

    try:
        pricing = fetch_pricing(args.base_url, key)
    except (urllib.error.URLError, OSError, ValueError) as exc:
        print(f"ERROR: не удалось получить прайс: {exc}", file=sys.stderr)
        return 2

    models = to_catalog(pricing, args.include, args.exclude,
                        only_chat=not args.all_endpoints)
    if not models:
        print("ERROR: прайс получен, но ни одна модель не распознана", file=sys.stderr)
        return 2

    doc = {"source": f"{args.base_url.rstrip('/')}{PRICING_PATH}",
           "models": models}
    text = json.dumps(doc, ensure_ascii=False, indent=2 if args.pretty else None)
    if args.output:
        with open(args.output, "w", encoding="utf-8") as fh:
            fh.write(text + "\n")
        print(f"записано {len(models)} моделей в {args.output}", file=sys.stderr)
    else:
        print(text)
    return 0


if __name__ == "__main__":
    sys.exit(main())
