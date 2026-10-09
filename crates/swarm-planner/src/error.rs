//! Ошибки Planner.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum PlannerError {
    #[error("planner config error: {0}")]
    Config(String),

    #[error("LLM request failed: {0}")]
    Llm(String),

    #[error("LLM response is not valid JSON: {0}")]
    InvalidJson(String),

    #[error("graph validation failed: {0}")]
    Validation(String),

    #[error("database error: {0}")]
    Database(#[from] swarm_core::DispatcherError),
}

pub type Result<T> = std::result::Result<T, PlannerError>;
