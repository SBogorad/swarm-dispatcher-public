//! Ошибки LLM-worker.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum WorkerError {
    #[error("worker config error: {0}")]
    Config(String),

    #[error("database error: {0}")]
    Database(#[from] swarm_core::DispatcherError),

    #[error("llm error: {0}")]
    Llm(#[from] swarm_llm::LlmError),

    #[error("artifact error: {0}")]
    Artifact(String),

    #[error("budget exhausted before LLM call")]
    BudgetExhausted,

    #[error("lease lost during work")]
    LeaseLost,

    // ---- v0.4: shell tools + sandbox ----
    /// WK004: команда не прошла allowlist (ТЗ v0.4 §4).
    #[error("WK004 allowlist rejected: {reason} (command: {command:?})")]
    AllowlistRejected { command: String, reason: String },

    /// WK005: команда превысила таймаут (ТЗ v0.4 §5).
    #[error("WK005 command timeout after {timeout_sec}s: {command:?}")]
    CommandTimeout { command: String, timeout_sec: u64 },

    /// Ошибка sandbox-контейнера (Docker).
    #[error("sandbox error: {0}")]
    Sandbox(String),

    /// Исчерпан бюджет итераций Agent Loop (ТЗ v0.4 §6).
    #[error("agent loop exceeded max iterations ({max})")]
    MaxIterations { max: u32 },

    /// LLM вернул невалидный tool call.
    #[error("invalid tool call from llm: {0}")]
    InvalidToolCall(String),
}

pub type Result<T> = std::result::Result<T, WorkerError>;
