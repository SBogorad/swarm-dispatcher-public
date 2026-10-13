//! LLM cost calculation (Issue #2: Cost ledger).
//!
//! Расчёт стоимости вызова LLM на основе модели и количества токенов.
//!
//! ВАЖНО (Issue #2): цены ниже — **оценки** для внутреннего учёта, а не
//! подтверждённые тарифы relay.fast (публичного API цен/баланса нет, только
//! web-интерфейс). Источник истины — сверка `llm_calls` с usage-логами
//! relay.fast (`scripts/reconcile_llm_cost.sh`). До сверки суммы в ledger
//! следует считать оценкой, а не фактом.

/// Известные модели и их цены (USD за 1M токенов): `(prompt, completion)`.
///
/// Мёртвых веток нет: раньше в `match` было две ветки `""` (вторая
/// недостижима — Rust всегда матчит первую), из-за чего «дорогая» цена
/// никогда не применялась. Теперь неизвестная модель обрабатывается явно
/// ([`is_known_model`]) и в ledger попадает предупреждение.
fn price_per_1m(model: &str) -> Option<(f64, f64)> {
    Some(match model {
        // deepseek-v4.1-flash — реальная модель воркеров (`.env`). Тот же
        // ценовой уровень, что deepseek-v4-flash.
        "deepseek-v4.1-flash" | "deepseek-v4-flash" => (0.015, 0.060),
        "deepseek-v4-pro" => (0.060, 0.240),
        "qwen3.8-flash" => (0.008, 0.024),
        "gpt-6-luna" => (0.008, 0.040),
        "gpt-4o-mini" => (0.150, 0.600),
        "claude-3.5-sonnet" => (3.0, 15.0),
        _ => return None,
    })
}

/// Цена для неизвестной модели — консервативная оценка (верхний уровень
/// известных flash-моделей), чтобы недооценка не прятала расход.
const UNKNOWN_MODEL_PRICE: (f64, f64) = (0.060, 0.240);

/// Известна ли цена модели. `false` → расчёт идёт по консервативной оценке,
/// и вызывающий обязан это залогировать (иначе расход молча неточен).
pub fn is_known_model(model: &str) -> bool {
    price_per_1m(model).is_some()
}

/// Расчёт стоимости вызова LLM (USD).
///
/// Возвращает `(prompt_cost, completion_cost, total_cost)`.
pub fn calculate_cost(model: &str, prompt_tokens: u32, completion_tokens: u32) -> (f64, f64, f64) {
    let (prompt_per_1m, completion_per_1m) = price_per_1m(model).unwrap_or(UNKNOWN_MODEL_PRICE);

    let prompt_cost = (prompt_tokens as f64 / 1_000_000.0) * prompt_per_1m;
    let completion_cost = (completion_tokens as f64 / 1_000_000.0) * completion_per_1m;
    let total_cost = prompt_cost + completion_cost;

    (prompt_cost, completion_cost, total_cost)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deepseek_flash_cost() {
        let (p, c, t) = calculate_cost("deepseek-v4-flash", 1000, 500);
        assert!((p - 0.000015).abs() < 1e-9);
        assert!((c - 0.000030).abs() < 1e-9);
        assert!((t - 0.000045).abs() < 1e-9);
    }

    /// Issue #2: реальная модель воркеров обязана быть в таблице цен, иначе
    /// расход считался по чужой цене молча.
    #[test]
    fn worker_model_is_known_and_priced_like_flash() {
        assert!(is_known_model("deepseek-v4.1-flash"));
        let (p, c, _) = calculate_cost("deepseek-v4.1-flash", 1000, 500);
        assert!((p - 0.000015).abs() < 1e-9);
        assert!((c - 0.000030).abs() < 1e-9);
    }

    #[test]
    fn known_models_are_flagged() {
        for m in [
            "deepseek-v4.1-flash",
            "deepseek-v4-flash",
            "deepseek-v4-pro",
            "qwen3.8-flash",
            "gpt-6-luna",
            "gpt-4o-mini",
            "claude-3.5-sonnet",
        ] {
            assert!(is_known_model(m), "{m} должна быть известна");
        }
    }

    /// Issue #2: неизвестная модель — явный сигнал, а не молчаливая подмена.
    #[test]
    fn unknown_model_is_flagged() {
        assert!(!is_known_model("gpt-99-ultra"));
        assert!(!is_known_model(""));
        // Расчёт всё равно даёт консервативную (не нулевую) оценку.
        let (_, _, t) = calculate_cost("gpt-99-ultra", 1_000_000, 0);
        assert!((t - 0.060).abs() < 1e-9);
    }

    /// Issue #2: пустая строка больше не отдельная ценовая ветка.
    #[test]
    fn empty_model_is_unknown() {
        assert!(!is_known_model(""));
    }
}
