//! Hard budget guard (Issue #3).
//!
//! Три независимых предохранителя против «бесконечной траты денег»:
//!
//! 1. **Глобальный лимит** `MAX_TOTAL_COST_USD` — суммарная стоимость всех
//!    LLM-вызовов (из cost ledger, Issue #2). Проверяется ПЕРЕД каждым вызовом.
//! 2. **Лимит задачи** `budget_cap` (в токенах) — уже есть в схеме, здесь лишь
//!    вынесен в явную проверку.
//! 3. **Circuit breaker на авторизацию** — HTTP 401/403 означает, что ключ
//!    невалиден: повторять бессмысленно (инцидент argo-memory: 4 часа пустых
//!    попыток). Такой вызов блокирует задачу немедленно и не ретраится.
//!
//! Плюс автоматическое отключение воркера после N подряд неудач (`should_disable`).

/// Денежные величины сравниваем в целых нанодолларах, чтобы не зависеть от
/// погрешности f64 и не терять мелкие расходы: 1 = $0.000000001.
///
/// Микродоллары (1e-6) оказались слишком грубыми: вызов за $0.0000004
/// округлялся в 0 и терялся — та же болезнь, что numeric(12,6) (Issue #14).
pub const NANO: f64 = 1_000_000_000.0;

/// Перевести USD в нанодоллары (округление к ближайшему).
pub fn usd_to_nano(usd: f64) -> i64 {
    if !usd.is_finite() {
        return i64::MAX; // неопределённость трактуем как «очень дорого» → стоп
    }
    (usd * NANO).round() as i64
}

/// Политика бюджета воркера (конфиг; состояние не хранит).
#[derive(Debug, Clone, Copy)]
pub struct BudgetPolicy {
    /// Общий лимит расходов на процесс воркера, USD (env `MAX_TOTAL_COST_USD`).
    pub max_total_cost_usd: f64,
    /// Сколько подряд неудачных задач терпим до самоотключения воркера.
    pub max_consecutive_errors: u32,
}

impl Default for BudgetPolicy {
    fn default() -> Self {
        Self {
            max_total_cost_usd: 5.0,
            max_consecutive_errors: 5,
        }
    }
}

/// Причина блокировки. Отображается в `block_reason` и состояние задачи.
#[derive(Debug, Clone, PartialEq)]
pub enum BudgetBlock {
    /// Суммарные расходы превысили глобальный лимит.
    GlobalCap { spent_usd: f64, cap_usd: f64 },
    /// Задача исчерпала свой токен-бюджет.
    TaskCap { spent_tokens: i64, cap_tokens: i64 },
    /// Провайдер отверг ключ (401/403) — повторять бессмысленно.
    AuthFailure { status: u16 },
}

impl BudgetBlock {
    /// Строка для `block_reason` / артефакта.
    pub fn reason(&self) -> String {
        match self {
            BudgetBlock::GlobalCap { spent_usd, cap_usd } => format!(
                "budget_exhausted: global cap exceeded (spent ${spent_usd:.6} >= cap ${cap_usd:.6})"
            ),
            BudgetBlock::TaskCap {
                spent_tokens,
                cap_tokens,
            } => format!(
                "budget_exhausted: task cap exceeded ({spent_tokens} >= {cap_tokens} tokens)"
            ),
            BudgetBlock::AuthFailure { status } => {
                format!("auth_failed: LLM provider rejected credentials (HTTP {status})")
            }
        }
    }

    /// Целевое состояние задачи. Авторизационный сбой — отдельное состояние:
    /// он требует вмешательства оператора (ротация ключа), а не просто бюджета.
    pub fn state(&self) -> &'static str {
        match self {
            BudgetBlock::AuthFailure { .. } => "failed",
            _ => "budget_exhausted",
        }
    }
}

/// Глобальный лимит не превышен?
///
/// `spent_usd` — суммарная стоимость из ledger, `cap_usd` — лимит.
/// Сравнение в микродолларах: ровно на границе (`spent == cap`) ещё разрешаем,
/// блокируем строго при превышении — как `spent_tokens > budget_cap` в схеме.
pub fn global_budget_ok(spent_usd: f64, cap_usd: f64) -> bool {
    usd_to_nano(spent_usd) <= usd_to_nano(cap_usd)
}

/// Токен-бюджет задачи не исчерпан?
///
/// Семантика совпадает с `claim_task` (схема): задача берётся, пока
/// `spent_tokens < budget_cap`.
pub fn task_budget_ok(spent_tokens: i64, budget_cap: i64) -> bool {
    spent_tokens < budget_cap
}

/// HTTP-статус означает проблему с ключом/правами (повтор бессмысленен)?
pub fn is_auth_failure(status: Option<u16>) -> bool {
    matches!(status, Some(401) | Some(403))
}

/// Пора ли отключать воркер целиком (слишком много неудач подряд)?
pub fn should_disable(consecutive_errors: u32, max_consecutive_errors: u32) -> bool {
    max_consecutive_errors > 0 && consecutive_errors >= max_consecutive_errors
}

/// Проверить все предохранители перед вызовом LLM.
///
/// Возвращает `Err(BudgetBlock)` — вызывающий обязан НЕ звать LLM и
/// заблокировать задачу.
pub fn check_before_call(
    policy: &BudgetPolicy,
    spent_usd: f64,
    spent_tokens: i64,
    budget_cap: i64,
) -> Result<(), BudgetBlock> {
    if !task_budget_ok(spent_tokens, budget_cap) {
        return Err(BudgetBlock::TaskCap {
            spent_tokens,
            cap_tokens: budget_cap,
        });
    }
    if !global_budget_ok(spent_usd, policy.max_total_cost_usd) {
        return Err(BudgetBlock::GlobalCap {
            spent_usd,
            cap_usd: policy.max_total_cost_usd,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nano_roundtrip() {
        assert_eq!(usd_to_nano(1.0), 1_000_000_000);
        assert_eq!(usd_to_nano(0.000017), 17_000);
        assert_eq!(usd_to_nano(0.0), 0);
    }

    #[test]
    fn global_cap_blocks_strictly_above() {
        // На границе — разрешено (как spent_tokens < budget_cap).
        assert!(global_budget_ok(5.0, 5.0));
        assert!(global_budget_ok(4.999999, 5.0));
        // Строго выше — блок.
        assert!(!global_budget_ok(5.000001, 5.0));
        assert!(!global_budget_ok(6.0, 5.0));
    }

    #[test]
    fn tiny_costs_do_not_round_to_zero() {
        // Именно этот случай терялся при numeric(12,6) — здесь он ловится.
        assert!(!global_budget_ok(0.0000004, 0.0000001));
        assert!(global_budget_ok(0.0000001, 0.0000004));
    }

    #[test]
    fn task_cap_semantics_match_schema() {
        assert!(task_budget_ok(0, 100));
        assert!(task_budget_ok(99, 100));
        assert!(!task_budget_ok(100, 100));
        assert!(!task_budget_ok(101, 100));
    }

    #[test]
    fn auth_failures_are_401_403() {
        assert!(is_auth_failure(Some(401)));
        assert!(is_auth_failure(Some(403)));
        assert!(!is_auth_failure(Some(429)));
        assert!(!is_auth_failure(Some(500)));
        assert!(!is_auth_failure(None));
    }

    #[test]
    fn disable_after_max_consecutive() {
        assert!(!should_disable(4, 5));
        assert!(should_disable(5, 5));
        assert!(should_disable(6, 5));
        // 0 = никогда не отключать автоматически.
        assert!(!should_disable(100, 0));
    }

    #[test]
    fn check_prefers_task_cap_then_global() {
        let p = BudgetPolicy {
            max_total_cost_usd: 5.0,
            max_consecutive_errors: 5,
        };
        // Всё в порядке.
        assert_eq!(check_before_call(&p, 0.0, 0, 100), Ok(()));
        // Исчерпан токен-бюджет задачи — это первое, что проверяем.
        assert!(matches!(
            check_before_call(&p, 0.0, 100, 100),
            Err(BudgetBlock::TaskCap { .. })
        ));
        // Глобальный лимит.
        assert!(matches!(
            check_before_call(&p, 9.0, 0, 100),
            Err(BudgetBlock::GlobalCap { .. })
        ));
    }

    #[test]
    fn reason_and_state_are_stable() {
        let g = BudgetBlock::GlobalCap {
            spent_usd: 6.0,
            cap_usd: 5.0,
        };
        assert!(g.reason().starts_with("budget_exhausted"));
        assert_eq!(g.state(), "budget_exhausted");

        let a = BudgetBlock::AuthFailure { status: 401 };
        assert!(a.reason().starts_with("auth_failed"));
        assert_eq!(a.state(), "failed");
    }
}
