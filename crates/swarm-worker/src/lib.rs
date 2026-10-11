//! LLM-worker: выполняет задачу через LLM, сохраняет артефакт на диск, сдаёт.
//!
//! ТЗ v0.3. В этом шаге воркер **только** вызывает LLM и сохраняет результат.
//!
//! FIND-01: shell-tools (agent_loop + sandbox + allowlist + audit) скрыты за
//! feature `shell-tools` (по умолчанию ВЫКЛ). Бинарь воркера их не подключает,
//! поэтому они не компилируются в бинарь. Включать — после F6/F7/F11/F12.

#[cfg(feature = "shell-tools")]
pub mod agent_loop;
#[cfg(feature = "shell-tools")]
pub mod allowlist;
pub mod artifacts;
#[cfg(feature = "shell-tools")]
pub mod audit;
pub mod error;
#[cfg(feature = "shell-tools")]
pub mod parse_step;
pub mod prompt;
pub mod retry;
#[cfg(feature = "shell-tools")]
pub mod sandbox;
pub mod storage;
pub mod worker;

#[cfg(feature = "shell-tools")]
pub use agent_loop::{
    parse_step, AgentLoopConfig, AgentResult, Step, ToolAudit, ToolAuditSink, ToolCall,
};
#[cfg(feature = "shell-tools")]
pub use allowlist::Allowlist;
#[cfg(feature = "shell-tools")]
pub use audit::PgAuditSink;
pub use error::{Result, WorkerError};
pub use retry::RetryPolicy;
#[cfg(feature = "shell-tools")]
pub use sandbox::{ExecOutput, Sandbox, SandboxConfig};
pub use worker::{process_lease, Outcome, WorkerConfig};
