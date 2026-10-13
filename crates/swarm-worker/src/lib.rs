//! LLM-worker: выполняет задачу через LLM, сохраняет артефакт на диск, сдаёт.
//!
//! ТЗ v0.3. В этом шаге воркер **только** вызывает LLM и сохраняет результат.
//!
//! FIND-01: shell-tools (agent_loop + sandbox + allowlist + audit) были скрыты
//! за feature `shell-tools` до закрытия блокеров безопасности F6/F7/F11/F12.
//! Все четыре закрыты (Волна 5) — гейт снят, модули компилируются всегда.

pub mod agent_loop;
pub mod allowlist;
pub mod artifacts;
pub mod audit;
pub mod budget;
pub mod error;
pub mod llm_cost;
pub mod parse_step;
pub mod poll;
pub mod prompt;
pub mod retry;
pub mod sandbox;
pub mod storage;
pub mod worker;

pub use agent_loop::{
    parse_step, AgentLoopConfig, AgentResult, Step, ToolAudit, ToolAuditSink, ToolCall,
};
pub use allowlist::Allowlist;
pub use audit::PgAuditSink;
pub use budget::{BudgetBlock, BudgetPolicy};
pub use error::{Result, WorkerError};
pub use poll::PollPolicy;
pub use retry::RetryPolicy;
pub use sandbox::{shell_tools_enabled, ExecOutput, Sandbox, SandboxConfig};
pub use worker::{process_lease, Outcome, WorkerConfig};
