"""Issue #7/#10: каталог моделей из прайса relay.fast — юнит-тесты (без сети).

Проверяем пересчёт прайса в USD за 1M токенов и отсев мусора, не трогая сеть:
`fetch_pricing` подменяется готовым словарём в формате `GET /api/pricing`.
"""
from __future__ import annotations

import importlib.util
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent


def _load():
    spec = importlib.util.spec_from_file_location(
        "fetch_model_catalog", REPO / "scripts" / "fetch_model_catalog.py"
    )
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


fmc = _load()


def _entry(name, model_ratio, group_ratio, completion_ratio,
           endpoints=("openai",), model_price=0):
    return {
        "model_name": name,
        "model_ratio": model_ratio,
        "group_ratio": {"default": group_ratio},
        "completion_ratio": completion_ratio,
        "model_price": model_price,
        "supported_endpoint_types": list(endpoints),
    }


def test_price_formula_matches_known_flash_model():
    """deepseek-v4-flash: 0.15 * 0.05 * 2 = 0.015 in, *4 = 0.060 out —
    та же цена, что в ledger (`llm_cost.rs`) и в `/api/log/token`."""
    p = {"data": [_entry("deepseek-v4-flash", 0.15, 0.05, 4)]}
    cat = fmc.to_catalog(p, [], [], only_chat=True)
    assert len(cat) == 1
    m = cat[0]
    assert abs(m["input_usd_per_1m"] - 0.015) < 1e-9
    assert abs(m["output_usd_per_1m"] - 0.060) < 1e-9


def test_chat_filter_excludes_image_models():
    p = {"data": [
        _entry("chat-model", 0.15, 0.05, 4),
        _entry("gpt-image-x", 0, 0.02, 0, endpoints=("image-generation",),
               model_price=0.5),
    ]}
    cat = fmc.to_catalog(p, [], [], only_chat=True)
    assert [m["name"] for m in cat] == ["chat-model"]


def test_priced_per_item_models_excluded():
    p = {"data": [_entry("per-image", 1, 0.02, 5, model_price=6)]}
    assert fmc.to_catalog(p, [], [], only_chat=True) == []


def test_nameless_entry_is_dropped():
    """В прайсе встречаются записи без имени (мусор разметки) — их не берём."""
    p = {"data": [_entry("", 1, 0.02, 5), _entry("real", 0.15, 0.05, 4)]}
    cat = fmc.to_catalog(p, [], [], only_chat=True)
    assert [m["name"] for m in cat] == ["real"]


def test_zero_or_missing_ratio_is_dropped():
    p = {"data": [
        _entry("zero", 0, 0.05, 4),
        {"model_name": "missing", "group_ratio": {"default": 0.05}},
    ]}
    assert fmc.to_catalog(p, [], [], only_chat=True) == []


def test_include_and_exclude_filters():
    p = {"data": [
        _entry("deepseek-v4-flash", 0.15, 0.05, 4),
        _entry("qwen3.8-flash", 0.075, 0.05, 3.13),
        _entry("gpt-6-sol", 1.0, 0.02, 5),
    ]}
    cat = fmc.to_catalog(p, ["flash"], [], only_chat=True)
    assert set(m["name"] for m in cat) == {"deepseek-v4-flash", "qwen3.8-flash"}
    cat2 = fmc.to_catalog(p, [], ["deepseek"], only_chat=True)
    assert "deepseek-v4-flash" not in [m["name"] for m in cat2]


def test_sorted_by_input_price():
    p = {"data": [
        _entry("expensive", 1.0, 0.02, 5),
        _entry("cheap", 0.05, 0.02, 5),
    ]}
    cat = fmc.to_catalog(p, [], [], only_chat=True)
    assert [m["name"] for m in cat] == ["cheap", "expensive"]


def test_quality_tier_heuristics():
    assert fmc.quality_tier_for("gpt-6-luna") == 2   # дешёвый суффикс важнее семейства
    assert fmc.quality_tier_for("gpt-6-sol") == 3
    assert fmc.quality_tier_for("deepseek-v4-flash") == 2
    assert fmc.quality_tier_for("deepseek-v4-pro") == 3
    assert fmc.quality_tier_for("claude-opus-5") == 3
    assert fmc.quality_tier_for("unknown-model") == 1