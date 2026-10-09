//! LLM-worker: выполняет задачу через LLM, сохраняет артефакт на диск, сдаёт.
//!
//! ТЗ v0.3. В этом шаге воркер **только** вызывает LLM и сохраняет результат —
//! никаких subprocess/shell (это v0.4).

pub mod agent_loop;
pub mod allowlist;
pub mod artifacts;
pub mod audit;
pub mod error;
pub mod parse_step;
pub mod prompt;
pub mod retry;
pub mod sandbox;
pub mod worker;

pub use agent_loop::{
    AgentLoopConfig, AgentResult, Step, ToolAudit, ToolAuditSink, ToolCall, parse_step,
};
pub use allowlist::Allowlist;
pub use audit::PgAuditSink;
pub use error::{Result, WorkerError};
pub use retry::RetryPolicy;
pub use sandbox::{ExecOutput, Sandbox, SandboxConfig};
pub use worker::{Outcome, WorkerConfig, process_lease};
