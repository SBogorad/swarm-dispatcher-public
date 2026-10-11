//! ПРАВИЛО: `postgres::Error` НЕ конвертируется через `?`/`#[from]`.
//!
//! У варианта `Database` стоит `#[source]`, а не `#[from]`. Иначе любой
//! будущий `?` на `postgres::Error` молча превратит доменную ошибку
//! (`LeaseLost`, `CycleDetected`, …) в общий `Database`, и маппинг SQLSTATE
//! будет обойдён. Любой `?` на ошибке postgres обязан идти явно:
//!
//! ```text
//! self.client.query_one(...).map_err(DispatcherError::from_pg)?;
//! Client::connect(dsn, NoTls).map_err(DispatcherError::from_pg)?;
//! ```

use thiserror::Error;

/// Полная иерархия ошибок диспетчера.
///
/// Соответствие SQLSTATE → вариант зафиксировано в `from_sqlstate`.
/// Добавление нового SQLSTATE требует добавления варианта и теста.
#[derive(Debug, Error)]
pub enum DispatcherError {
    #[error("lease lost or expired")]
    LeaseLost,

    #[error("artifact not found")]
    ArtifactMissing,

    #[error("contract invalid")]
    ContractInvalid,

    #[error("task not found")]
    TaskNotFound,

    #[error("dependency cycle")]
    CycleDetected,

    #[error("depth limit exceeded")]
    DepthExceeded,

    #[error("fanout limit exceeded")]
    FanoutExceeded,

    #[error("task limit exceeded")]
    TaskLimitExceeded,

    #[error("task is not in 'done' state")]
    NotDone,

    #[error("root task not found")]
    RootNotFound,

    #[error("dependency spans different roots")]
    CrossRootDependency,

    #[error("task is not in 'pending' state")]
    TaskNotPending,

    #[error("task is not in 'blocked' state")]
    NotBlocked,

    #[error("review is stale (wrong artifact or state)")]
    ReviewStale,

    #[error("self-review is not allowed")]
    SelfReview,

    #[error("acceptance checks failed")]
    ChecksFailed,

    #[error("worker capabilities are invalid")]
    WorkerInvalid,

    #[error("worker is not registered")]
    WorkerUnknown,

    #[error("database error")]
    Database(#[source] postgres::Error),

    #[error("serialization error")]
    Serialization(#[from] serde_json::Error),
}

impl DispatcherError {
    /// Маппинг ошибки `postgres` в доменную ошибку по SQLSTATE.
    ///
    /// Если SQLSTATE неизвестен — возвращаем как есть.
    pub fn from_pg(e: postgres::Error) -> Self {
        if let Some(db) = e.as_db_error() {
            if let Some(known) = Self::from_sqlstate(db.code().code()) {
                return known;
            }
        }
        Self::Database(e)
    }

    fn from_sqlstate(code: &str) -> Option<Self> {
        Some(match code {
            "LS001" => Self::LeaseLost,
            "AR001" => Self::ArtifactMissing,
            "CT001" => Self::ContractInvalid,
            "GR000" => Self::TaskNotFound,
            "GR001" => Self::CycleDetected,
            "GR002" => Self::DepthExceeded,
            "GR003" => Self::FanoutExceeded,
            "GR004" => Self::TaskLimitExceeded,
            "GR005" => Self::NotDone,
            "GR006" => Self::RootNotFound,
            "GR007" => Self::CrossRootDependency,
            "GR008" => Self::TaskNotPending,
            "GR009" => Self::NotBlocked,
            "RV001" => Self::ReviewStale,
            "RV002" => Self::SelfReview,
            "RV003" => Self::ChecksFailed,
            "WK001" => Self::WorkerInvalid,
            "WK002" => Self::WorkerUnknown,
            _ => return None,
        })
    }

    pub fn is_transient(&self) -> bool {
        // Сериализация/дедлоки Postgres. В v0 ретраев нет — заготовка.
        matches!(self, Self::Database(e)
            if matches!(e.code().map(|c| c.code()), Some("40001" | "40P01")))
    }

    /// `insufficient_privilege` (42501): роли не хватает гранта на функцию/таблицу.
    ///
    /// Нужно вызывающему, который хочет отличить «нет прав читать» от
    /// «нет данных» — например, Planner под ролью `swarm_planner` без гранта
    /// на `task_status`.
    pub fn is_insufficient_privilege(&self) -> bool {
        matches!(self, Self::Database(e)
            if e.code().map(|c| c.code() == "42501").unwrap_or(false))
    }

    /// `unique_violation` (23505): вставка конфликтует с существующей строкой.
    ///
    /// Для Planner это сигнал идемпотентного пропуска: граф с таким `root_id`
    /// уже создан (актуально, когда читать `task_status` роли не разрешено).
    pub fn is_unique_violation(&self) -> bool {
        matches!(self, Self::Database(e)
            if e.code().map(|c| c.code() == "23505").unwrap_or(false))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sqlstate_known_codes_map() {
        // Проверяем именно маппинг SQLSTATE → вариант, а не текст сообщения.
        use DispatcherError::*;
        let cases: [(&str, fn(&DispatcherError) -> bool); 18] = [
            ("LS001", |e| matches!(e, LeaseLost)),
            ("AR001", |e| matches!(e, ArtifactMissing)),
            ("CT001", |e| matches!(e, ContractInvalid)),
            ("GR000", |e| matches!(e, TaskNotFound)),
            ("GR001", |e| matches!(e, CycleDetected)),
            ("GR002", |e| matches!(e, DepthExceeded)),
            ("GR003", |e| matches!(e, FanoutExceeded)),
            ("GR004", |e| matches!(e, TaskLimitExceeded)),
            ("GR005", |e| matches!(e, NotDone)),
            ("GR006", |e| matches!(e, RootNotFound)),
            ("GR007", |e| matches!(e, CrossRootDependency)),
            ("GR008", |e| matches!(e, TaskNotPending)),
            ("GR009", |e| matches!(e, NotBlocked)),
            ("RV001", |e| matches!(e, ReviewStale)),
            ("RV002", |e| matches!(e, SelfReview)),
            ("RV003", |e| matches!(e, ChecksFailed)),
            ("WK001", |e| matches!(e, WorkerInvalid)),
            ("WK002", |e| matches!(e, WorkerUnknown)),
        ];
        for (code, is_expected) in cases {
            let e = DispatcherError::from_sqlstate(code)
                .unwrap_or_else(|| panic!("sqlstate {code} not mapped"));
            assert!(
                is_expected(&e),
                "sqlstate {code} mapped to wrong variant: {e:?}"
            );
        }
    }

    #[test]
    fn sqlstate_unknown_returns_none() {
        assert!(DispatcherError::from_sqlstate("99999").is_none());
        assert!(DispatcherError::from_sqlstate("").is_none());
        assert!(DispatcherError::from_sqlstate("XX000").is_none());
    }
}
