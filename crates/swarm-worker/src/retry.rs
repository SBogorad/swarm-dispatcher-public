//! Политика ретраев для вызовов LLM (ТЗ v0.3 §5).
//!
//! - Сетевой таймаут / 5xx / 429 → retry с exponential backoff + jitter.
//! - 4xx (кроме 429) / invalid JSON → не retry.
//! - Превышение бюджета токенов → не вызывать LLM вообще.

use swarm_llm::LlmError;

/// Политика повторов.
#[derive(Debug, Clone, Copy)]
pub struct RetryPolicy {
    pub max_attempts: u32,
    pub base_delay_ms: u64,
    pub max_delay_ms: u64,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            base_delay_ms: 200,
            max_delay_ms: 5_000,
        }
    }
}

impl RetryPolicy {
    /// Решать ли повтор по ошибке.
    pub fn should_retry(&self, err: &LlmError) -> bool {
        err.is_transient()
    }

    /// Задержка перед попыткой `attempt` (0-based) с jitter.
    ///
    /// `jitter_fn` возвращает детерминированное значение в [0,1) — в тестах
    /// подменяется, в бою — от таймера.
    pub fn delay_ms(&self, attempt: u32, jitter: f64) -> u64 {
        let exp = self.base_delay_ms.saturating_mul(1u64 << attempt.min(20));
        let capped = exp.min(self.max_delay_ms);
        let j = (capped as f64 * jitter.clamp(0.0, 1.0)) as u64;
        capped.saturating_sub(j)
    }
}

/// Псевдо-jitter без внешних зависимостей (не для криптографии).
pub fn jitter_unit() -> f64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64)
        .unwrap_or(0);
    (nanos % 1000) as f64 / 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transient_errors_retry() {
        let p = RetryPolicy::default();
        assert!(p.should_retry(&LlmError::Timeout));
        assert!(p.should_retry(&LlmError::Transport("conn reset".into())));
        assert!(p.should_retry(&LlmError::Http {
            status: 500,
            body: "x".into()
        }));
        assert!(p.should_retry(&LlmError::Http {
            status: 429,
            body: "x".into()
        }));
    }

    #[test]
    fn client_errors_do_not_retry() {
        let p = RetryPolicy::default();
        assert!(!p.should_retry(&LlmError::Http {
            status: 400,
            body: "bad".into()
        }));
        assert!(!p.should_retry(&LlmError::Http {
            status: 404,
            body: "no".into()
        }));
        assert!(!p.should_retry(&LlmError::InvalidJson("x".into())));
        assert!(!p.should_retry(&LlmError::Config("x".into())));
    }

    #[test]
    fn backoff_grows_and_caps() {
        let p = RetryPolicy {
            max_attempts: 5,
            base_delay_ms: 100,
            max_delay_ms: 1000,
        };
        let d0 = p.delay_ms(0, 0.0);
        let d1 = p.delay_ms(1, 0.0);
        let d10 = p.delay_ms(10, 0.0);
        assert_eq!(d0, 100);
        assert_eq!(d1, 200);
        assert_eq!(d10, 1000, "должно упираться в max_delay_ms");
    }
}
