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

fn default_attempts() -> u32 {
    3
}

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
    /// F-NEW-A: машиночитаемая постановка задачи. В отличие от свободного
    /// `description`, это структурированное поле цели: планировщик заполняет
    /// его для корневой задачи, чтобы контракт `kind = "root"` не выглядел
    /// пустой обёрткой без задания. Пробрасывается воркеру в промпт (как и
    /// `description` — через `<untrusted>`), поэтому санируется тем же путём.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub goal: Option<String>,
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
        if self.kind.trim().is_empty()
            || self.checks.is_empty()
            || self.budget.attempts == 0
            || self
                .needs
                .min_ram_gb
                .is_some_and(|v| !v.is_finite() || v < 0.0)
            || self
                .needs
                .coder_class
                .is_some_and(|v| !v.is_finite() || v < 0.0)
        {
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
    /// FIND-CODE-02: число возвратов задачи на доработку. `> 0` означает, что
    /// предыдущая попытка была отвергнута ревьюером.
    pub rework: i32,
    /// FIND-CODE-02: диагностика последнего ревью
    /// (`{verdict, artifact_ref, checks, reviewer}`) или `null`, если ревью
    /// ещё не было. Воркер при `rework > 0` обязан подложить это в промпт,
    /// иначе он перегенерирует то же решение вслепую (корень
    /// `changes_requested = 39` на проде).
    pub last_review: Option<serde_json::Value>,
}

impl Lease {
    /// Типизированный разбор контракта. **Не влияет на аренду** — вызывается
    /// только тем, кому нужен типизированный доступ к полям контракта.
    pub fn typed_contract(&self) -> Result<TaskContract, DispatcherError> {
        let contract: TaskContract = serde_json::from_value(self.contract.clone())
            .map_err(DispatcherError::Serialization)?;
        contract.validate()?;
        Ok(contract)
    }
}

/// Итерация 3 (DAG handoff): ссылка на артефакт родительской задачи.
///
/// Возвращается RPC `task_parent_artifacts` только держателю живой аренды
/// дочерней задачи: воркер должен видеть вход, произведённый зависимостями,
/// не имея прямого `SELECT` к `tasks`/`artifacts`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParentArtifactRef {
    pub parent_task_id: Uuid,
    /// `contract.kind` родителя (`coalesce(..., 'task')` для legacy-контрактов).
    pub parent_kind: String,
    /// `sha256:<hex>`.
    pub artifact_ref: String,
    /// Hex-часть `artifact_ref` без префикса (для сверки содержимого).
    pub artifact_sha256: String,
}

/// Pstack `reflect`/`correct`: урок, выученный роем на прошлом графе.
///
/// Возвращается RPC `task_recall_memory` — общая память роя вместо
/// `MEMORY.md`, которого у контейнера воркера нет. `created_at` — текст
/// (ISO-8601 из `timestamptz::text`): `swarm-core` не тянет `chrono`-фичу
/// `postgres`, а сортировку и отображение делает SQL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryLesson {
    /// Формулировка урока (10..2000 символов, гарантирует CHECK в БД).
    pub lesson: String,
    /// `kind`, к которому урок относится (`*` — применим к любому виду).
    pub kind: String,
    /// Сколько реворков понадобилось задаче-источнику.
    pub rework_count: i32,
    /// Момент записи урока, ISO-8601.
    pub created_at: String,
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
    /// Issue #3: исчерпан бюджет (токен-лимит или глобальный).
    BudgetExhausted,
    /// Issue #3: необратимый сбой (например, авторизация 401/403).
    Failed,
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
            TaskState::BudgetExhausted => "budget_exhausted",
            TaskState::Failed => "failed",
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
            "budget_exhausted" => TaskState::BudgetExhausted,
            "failed" => TaskState::Failed,
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

/// Кандидат на ревью — строка `review_candidates()` (Issue #26).
///
/// Доступна роли `swarm_reviewer` (SECURITY DEFINER витрина), у которой нет
/// прямого SELECT на `tasks`/`artifacts`.
#[derive(Debug, Clone)]
pub struct ReviewCandidate {
    pub id: Uuid,
    pub result_ref: Option<String>,
    pub contract: serde_json::Value,
    pub storage_path: Option<String>,
    pub created_by: Option<String>,
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
            goal: None,
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
            goal: None,
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
            goal: None,
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
        for s in [
            "pending",
            "ready",
            "leased",
            "review",
            "done",
            "blocked",
            "budget_exhausted",
            "failed",
        ] {
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
