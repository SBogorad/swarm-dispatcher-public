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

/// Жёсткие лимиты ОДНОГО прохода воркера (Issue #4).
///
/// Issue #4: «установить жёсткий предел: max_llm_calls, max_duration_sec,
/// max_cost_usd». Денежный лимит живёт в `BudgetPolicy` (Issue #3); здесь —
/// счётчики, которые останавливают сам ЦИКЛ процесса, а не отдельную задачу:
/// сколько LLM-вызовов сделано и сколько времени прошло с начала прохода.
///
/// `0` означает «без лимита» — это осознанный дефолт, чтобы штатный
/// долгоживущий воркер не менял поведение. Лимиты включаются явно.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RunLimits {
    /// Максимум LLM-вызовов за проход (0 — без лимита).
    pub max_llm_calls: u32,
    /// Максимум секунд с начала прохода (0 — без лимита).
    pub max_duration_sec: u64,
}

/// Почему проход остановлен.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunStop {
    /// Сделано `made` вызовов при лимите `limit`.
    LlmCalls { made: u32, limit: u32 },
    /// Прошло `elapsed_sec` секунд при лимите `limit`.
    Duration { elapsed_sec: u64, limit: u64 },
}

impl RunStop {
    /// Строка для лога/артефакта.
    pub fn reason(&self) -> String {
        match self {
            RunStop::LlmCalls { made, limit } => {
                format!("run_limit: llm calls {made} >= {limit}")
            }
            RunStop::Duration { elapsed_sec, limit } => {
                format!("run_limit: duration {elapsed_sec}s >= {limit}s")
            }
        }
    }
}

impl RunLimits {
    /// Проверить пределы прохода ПЕРЕД очередным claim.
    ///
    /// `None` — можно продолжать; `Some(RunStop)` — пора останавливаться.
    /// Семантика «строго выше»: ровно на границе (`made == limit`) уже
    /// останавливаемся — так лимит `1` означает ровно один вызов.
    pub fn check(&self, llm_calls: u32, elapsed_sec: u64) -> Option<RunStop> {
        if self.max_llm_calls > 0 && llm_calls >= self.max_llm_calls {
            return Some(RunStop::LlmCalls {
                made: llm_calls,
                limit: self.max_llm_calls,
            });
        }
        if self.max_duration_sec > 0 && elapsed_sec >= self.max_duration_sec {
            return Some(RunStop::Duration {
                elapsed_sec,
                limit: self.max_duration_sec,
            });
        }
        None
    }
}

/// Причина блокировки. Отображается в `block_reason` и состояние задачи.
#[derive(Debug, Clone, PartialEq)]
pub enum BudgetBlock {
    /// Суммарные расходы превысили глобальный лимит.
    GlobalCap { spent_usd: f64, cap_usd: f64 },
    /// Задача исчерпала свой токен-бюджет.
    TaskCap { spent_tokens: i64, cap_tokens: i64 },
    /// F8: дерево (root_id) исчерпало суммарный бюджет root_budget_cap.
    RootCap { root_spent: i64, root_cap: i64 },
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
            BudgetBlock::RootCap {
                root_spent,
                root_cap,
            } => format!(
                "budget_exhausted: root tree cap exceeded ({root_spent} >= {root_cap} tokens)"
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
/// F8: кроме локального токен-бюджета задачи проверяется суммарный бюджет
/// дерева (`root_spent`/`root_budget`): `root_budget = None` означает, что
/// лимита по дереву нет. Блокируем строго на границе (`>=`), как гейты в SQL.
///
/// Возвращает `Err(BudgetBlock)` — вызывающий обязан НЕ звать LLM и
/// заблокировать задачу.
pub fn check_before_call(
    policy: &BudgetPolicy,
    spent_usd: f64,
    spent_tokens: i64,
    budget_cap: i64,
    root_spent: i64,
    root_budget: Option<i64>,
) -> Result<(), BudgetBlock> {
    if !task_budget_ok(spent_tokens, budget_cap) {
        return Err(BudgetBlock::TaskCap {
            spent_tokens,
            cap_tokens: budget_cap,
        });
    }
    if let Some(root_cap) = root_budget {
        if !task_budget_ok(root_spent, root_cap) {
            return Err(BudgetBlock::RootCap {
                root_spent,
                root_cap,
            });
        }
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
        assert_eq!(check_before_call(&p, 0.0, 0, 100, 0, None), Ok(()));
        // Исчерпан токен-бюджет задачи — это первое, что проверяем.
        assert!(matches!(
            check_before_call(&p, 0.0, 100, 100, 0, None),
            Err(BudgetBlock::TaskCap { .. })
        ));
        // F8: исчерпан бюджет дерева (локальный цел).
        assert!(matches!(
            check_before_call(&p, 0.0, 0, 100, 500, Some(500)),
            Err(BudgetBlock::RootCap { .. })
        ));
        // F8: лимита по дереву нет (None) — не блокирует.
        assert_eq!(check_before_call(&p, 0.0, 0, 100, 10_000, None), Ok(()));
        // Глобальный лимит.
        assert!(matches!(
            check_before_call(&p, 9.0, 0, 100, 0, None),
            Err(BudgetBlock::GlobalCap { .. })
        ));
    }

    #[test]
    fn run_limits_zero_means_unlimited() {
        let l = RunLimits::default();
        assert_eq!(l.check(0, 0), None);
        assert_eq!(l.check(10_000, 86_400), None);
    }

    #[test]
    fn run_limits_stop_on_llm_calls() {
        let l = RunLimits {
            max_llm_calls: 3,
            max_duration_sec: 0,
        };
        assert_eq!(l.check(2, 0), None);
        // Ровно на границе — уже стоп: лимит 1 = ровно один вызов.
        assert_eq!(l.check(3, 0), Some(RunStop::LlmCalls { made: 3, limit: 3 }));
        assert!(matches!(l.check(4, 0), Some(RunStop::LlmCalls { .. })));
    }

    #[test]
    fn run_limits_stop_on_duration() {
        let l = RunLimits {
            max_llm_calls: 0,
            max_duration_sec: 60,
        };
        assert_eq!(l.check(0, 59), None);
        assert_eq!(
            l.check(0, 60),
            Some(RunStop::Duration {
                elapsed_sec: 60,
                limit: 60
            })
        );
    }

    #[test]
    fn run_limits_llm_calls_checked_first() {
        let l = RunLimits {
            max_llm_calls: 1,
            max_duration_sec: 1,
        };
        // Оба предела достигнуты — приоритет у вызовов (детерминизм сообщения).
        assert!(matches!(l.check(1, 1), Some(RunStop::LlmCalls { .. })));
    }

    #[test]
    fn run_stop_reason_is_descriptive() {
        assert!(RunStop::LlmCalls { made: 5, limit: 5 }
            .reason()
            .starts_with("run_limit: llm calls"));
        assert!(RunStop::Duration {
            elapsed_sec: 30,
            limit: 30
        }
        .reason()
        .starts_with("run_limit: duration"));
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
