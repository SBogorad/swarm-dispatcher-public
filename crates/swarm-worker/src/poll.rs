//! Issue #15: backoff опроса очереди вместо busy-poll.
//!
//! До фикса воркер в простое вызывал `claim_task` каждые 2 секунды: 2 воркера
//! × 30 запросов/мин вхолостую, при масштабировании — N × 30. Здесь —
//! экспоненциальный backoff с потолком: при простое интервал растёт от
//! `base_ms` до `max_ms`, а при получении задачи сбрасывается к `base_ms`.
//!
//! Почему не LISTEN/NOTIFY (вариант из issue): синхронный крейт `postgres`
//! требует отдельного соединения и потока для асинхронных уведомлений, что
//! усложняет shutdown и тесты. Backoff даёт тот же эффект (в простое — редкие
//! опросы, 2 с → 30 с = в 15 раз меньше запросов) без новой конкурентной
//! модели. NOTIFY можно добавить позже поверх этого как ускоритель.

use std::time::Duration;

/// Политика backoff опроса (конфиг; состояние не хранит).
#[derive(Debug, Clone, Copy)]
pub struct PollPolicy {
    /// Базовая задержка после получения задачи / при старте, мс.
    pub base_ms: u64,
    /// Потолок задержки при длительном простое, мс.
    pub max_ms: u64,
}

impl Default for PollPolicy {
    fn default() -> Self {
        Self {
            base_ms: 2000,
            max_ms: 30_000,
        }
    }
}

impl PollPolicy {
    /// Собрать политику из env-значений, отбросив невалидные (0 → дефолт).
    pub fn from_env(base_ms: Option<u64>, max_ms: Option<u64>) -> Self {
        let d = Self::default();
        let base = match base_ms {
            Some(v) if v > 0 => v,
            _ => d.base_ms,
        };
        let max = match max_ms {
            Some(v) if v > 0 => v,
            _ => d.max_ms,
        };
        // Потолок не может быть меньше базы — иначе backoff «отрицательный».
        Self {
            base_ms: base,
            max_ms: max.max(base),
        }
    }

    /// Задержка перед следующим опросом после `idle_rounds` пустых ответов.
    ///
    /// `idle_rounds == 0` → `base_ms`; далее удвоение до `max_ms`.
    /// Сдвиг ограничен 30, чтобы `1 << n` не переполнился.
    pub fn idle_delay(&self, idle_rounds: u32) -> Duration {
        let shift = idle_rounds.min(30);
        let factor = 1u64.checked_shl(shift).unwrap_or(u64::MAX);
        let ms = self.base_ms.saturating_mul(factor).min(self.max_ms);
        Duration::from_millis(ms)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_idle_round_uses_base() {
        let p = PollPolicy::default();
        assert_eq!(p.idle_delay(0), Duration::from_millis(2000));
    }

    #[test]
    fn delay_grows_exponentially_then_caps() {
        let p = PollPolicy {
            base_ms: 2000,
            max_ms: 30_000,
        };
        assert_eq!(p.idle_delay(0), Duration::from_millis(2000));
        assert_eq!(p.idle_delay(1), Duration::from_millis(4000));
        assert_eq!(p.idle_delay(2), Duration::from_millis(8000));
        assert_eq!(p.idle_delay(3), Duration::from_millis(16_000));
        // 2000 * 2^4 = 32000 > 30000 → потолок.
        assert_eq!(p.idle_delay(4), Duration::from_millis(30_000));
        assert_eq!(p.idle_delay(50), Duration::from_millis(30_000));
    }

    #[test]
    fn huge_shift_does_not_overflow() {
        let p = PollPolicy::default();
        // Раньше `1u64 << 40` в debug паниковал; теперь — saturating.
        assert_eq!(p.idle_delay(u32::MAX), Duration::from_millis(30_000));
    }

    #[test]
    fn from_env_falls_back_on_zero_and_keeps_max_above_base() {
        let p = PollPolicy::from_env(Some(0), Some(0));
        assert_eq!(p.base_ms, 2000);
        assert_eq!(p.max_ms, 30_000);
        // Потолок меньше базы — поднимаем до базы, чтобы не было «отрицательного» backoff.
        let q = PollPolicy::from_env(Some(5000), Some(1000));
        assert_eq!(q.base_ms, 5000);
        assert_eq!(q.max_ms, 5000);
        assert_eq!(q.idle_delay(3), Duration::from_millis(5000));
    }
}
