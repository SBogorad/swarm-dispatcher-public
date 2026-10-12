//! LLM cost calculation (Issue #2: Cost ledger).
//!
//! Расчёт стоимости вызова LLM на основе модели и количества токенов.
//! Цены relay.fast актуальны на 2026-10-12.

/// Расчёт стоимости вызова LLM (USD).
pub fn calculate_cost(
    model: &str,
    prompt_tokens: u32,
    completion_tokens: u32,
) -> (f64, f64, f64) {
    let (prompt_per_1m, completion_per_1m) = match model {
        // relay.fast цены на 2026-10-12
        "deepseek-v4-flash" => (0.015, 0.060),
        "claude-haiku-5-5" => (0.004, 0.018),
        "qwen3.8-flash" => (0.008, 0.024),
        "gpt-6-luna" => (0.008, 0.040),
        "claude-opus-5" => (1.5, 7.5), // дороже, но мощнее
        
        // Дефолт для неизвестных моделей — консервативная оценка deepseek-v4-flash
        _ => (0.015, 0.060),
    };

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

    #[test]
    fn claude_haiku_cost() {
        let (p, c, t) = calculate_cost("claude-haiku-5-5", 10_000, 2_000);
        assert!((p - 0.00004).abs() < 1e-9);
        assert!((c - 0.000036).abs() < 1e-9);
        assert!((t - 0.000076).abs() < 1e-9);
    }

    #[test]
    fn unknown_model_uses_default() {
        let (p, c, t) = calculate_cost("gpt-99-ultra", 1000, 500);
        // Дефолт — deepseek-v4-flash
        assert!((p - 0.000015).abs() < 1e-9);
        assert!((c - 0.000030).abs() < 1e-9);
        assert!((t - 0.000045).abs() < 1e-9);
    }
}
