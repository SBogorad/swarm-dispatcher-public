use crate::errors::DispatcherError;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Needs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_ram_gb: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coder_class: Option<f64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub affinity_tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Budget {
    #[serde(default = "default_attempts")]
    pub attempts: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_rework: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_depth: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_children: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tasks: Option<u32>,
}

fn default_attempts() -> u32 { 3 }

impl Default for Budget {
    fn default() -> Self {
        Self {
            attempts: default_attempts(),
            max_rework: None,
            max_depth: None,
            max_children: None,
            max_tasks: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskContract {
    pub kind: String,
    /// Текст задачи от пользователя (через Planner). Пробрасывается воркеру,
    /// чтобы LLM знал, что именно делать. Помечается как недоверенный ввод
    /// в промпте воркера (`<untrusted>`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Непустой массив check-ов. Интерпретируется воркером, схема проверяет только форму.
    pub checks: Vec<serde_json::Value>,
    #[serde(default)]
    pub needs: Needs,
    #[serde(default)]
    pub budget: Budget,
}

impl TaskContract {
    /// Локальная валидация формы. Совпадает по строгости с SQL-валидацией:
    /// что отклоняем здесь — отклонил бы и SQL. SQL остаётся источником истины.
    pub fn validate(&self) -> Result<(), DispatcherError> {
        if self.checks.is_empty() {
            return Err(DispatcherError::ContractInvalid);
        }
        Ok(())
    }
}

/// Аренда, возвращённая из `claim_task`. Frozen-семантика на стороне вызывающего.
///
/// `contract` хранится как **сырой JSON**, а не как `TaskContract`: SQL
/// принимает контракты, которые Rust-тип не разберёт (без `kind`, с дробным
/// `budget.attempts`). Если бы `claim` десериализовал контракт сразу после
/// выдачи аренды, задача зависла бы в `leased` на весь срок аренды, затем
/// `reclaim_expired` сжёг бы попытку, и через `attempts` попыток задача ушла
/// бы в `blocked`. Разбор — только явно, через [`Lease::typed_contract`].
#[derive(Debug, Clone)]
pub struct Lease {
    pub task_id: Uuid,
    pub contract: serde_json::Value,
    pub epoch: i64,
    pub budget_cap: i64,
    pub spent_tokens: i64,
    pub worker_id: String,
}

impl Lease {
    /// Типизированный разбор контракта. **Не влияет на аренду** — вызывается
    /// только тем, кому нужен типизированный доступ к полям контракта.
    pub fn typed_contract(&self) -> Result<TaskContract, DispatcherError> {
        serde_json::from_value(self.contract.clone()).map_err(Into::into)
    }
}

/// Вердикт ревьюера.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Approved,
    ChangesRequested,
    Rejected,
}

impl Verdict {
    pub fn as_str(&self) -> &'static str {
        match self {
            Verdict::Approved => "approved",
            Verdict::ChangesRequested => "changes_requested",
            Verdict::Rejected => "rejected",
        }
    }
}

/// Состояния задачи. Соответствует CHECK в схеме.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskState {
    Pending,
    Ready,
    Leased,
    Review,
    Done,
    Blocked,
}

impl TaskState {
    pub fn as_str(&self) -> &'static str {
        match self {
            TaskState::Pending => "pending",
            TaskState::Ready => "ready",
            TaskState::Leased => "leased",
            TaskState::Review => "review",
            TaskState::Done => "done",
            TaskState::Blocked => "blocked",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "pending" => TaskState::Pending,
            "ready" => TaskState::Ready,
            "leased" => TaskState::Leased,
            "review" => TaskState::Review,
            "done" => TaskState::Done,
            "blocked" => TaskState::Blocked,
            _ => return None,
        })
    }
}

/// Строка функции `task_status()` — путь чтения, доступный сервисным ролям
/// после применения `roles.sql` (прямой SELECT к `tasks` им закрыт).
#[derive(Debug, Clone)]
pub struct TaskStatusRow {
    pub id: Uuid,
    pub root_id: Uuid,
    pub state: String,
    pub lease_epoch: i64,
    pub owner: Option<String>,
    pub attempts: i32,
    pub rework: i32,
    pub block_reason: Option<String>,
}

/// Строка таблицы `tasks` для отображения в CLI / отчётах.
#[derive(Debug, Clone)]
pub struct TaskRow {
    pub id: Uuid,
    pub root_id: Uuid,
    pub state: String,
    pub owner: Option<String>,
    pub lease_epoch: i64,
    pub attempts: i32,
    pub rework: i32,
    pub priority: i32,
    pub block_reason: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contract_valid() {
        let c = TaskContract {
            kind: "test".into(),
            description: None,
            checks: vec![serde_json::json!({"type": "cmd", "run": "true"})],
            needs: Needs::default(),
            budget: Budget::default(),
        };
        assert!(c.validate().is_ok());
    }

    #[test]
    fn contract_empty_checks_rejected() {
        let c = TaskContract {
            kind: "test".into(),
            description: None,
            checks: vec![],
            needs: Needs::default(),
            budget: Budget::default(),
        };
        assert!(matches!(
            c.validate(),
            Err(DispatcherError::ContractInvalid)
        ));
    }

    #[test]
    fn contract_deserialize_minimal() {
        let raw = r#"{"kind":"x","checks":[{"type":"cmd"}]}"#;
        let c: TaskContract = serde_json::from_str(raw).unwrap();
        assert_eq!(c.kind, "x");
        assert_eq!(c.checks.len(), 1);
        assert_eq!(c.budget.attempts, 3);
        assert!(c.needs.min_ram_gb.is_none());
        assert!(c.needs.affinity_tags.is_empty());
    }

    #[test]
    fn contract_serialize_omits_none() {
        let c = TaskContract {
            kind: "x".into(),
            description: None,
            checks: vec![serde_json::json!({"type": "cmd"})],
            needs: Needs::default(),
            budget: Budget {
                attempts: 5,
                max_rework: None,
                max_depth: None,
                max_children: None,
                max_tasks: None,
            },
        };
        let s = serde_json::to_string(&c).unwrap();
        assert!(!s.contains("min_ram_gb"), "None must be omitted: {s}");
        assert!(!s.contains("max_rework"), "None must be omitted: {s}");
        assert!(s.contains("\"attempts\":5"));
    }

    #[test]
    fn task_state_roundtrip() {
        for s in ["pending", "ready", "leased", "review", "done", "blocked"] {
            let st = TaskState::parse(s).expect(s);
            assert_eq!(st.as_str(), s);
        }
        assert!(TaskState::parse("banana").is_none());
    }

    #[test]
    fn verdict_as_str() {
        assert_eq!(Verdict::Approved.as_str(), "approved");
        assert_eq!(Verdict::ChangesRequested.as_str(), "changes_requested");
        assert_eq!(Verdict::Rejected.as_str(), "rejected");
    }
}
