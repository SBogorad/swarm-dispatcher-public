//! Ошибки LLM-клиента.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum LlmError {
    #[error("llm config error: {0}")]
    Config(String),

    #[error("llm transport error: {0}")]
    Transport(String),

    #[error("llm http error: status={status} body={body}")]
    Http { status: u16, body: String },

    #[error("llm response is not valid json: {0}")]
    InvalidJson(String),

    #[error("llm request timed out")]
    Timeout,
}

impl LlmError {
    /// Транзиентная ошибка — имеет смысл повторить (сеть, 5xx, 429, таймаут).
    pub fn is_transient(&self) -> bool {
        match self {
            LlmError::Timeout => true,
            LlmError::Transport(_) => true,
            LlmError::Http { status, .. } => *status >= 500 || *status == 429,
            _ => false,
        }
    }

    /// HTTP-статус, если ошибка HTTP.
    pub fn status(&self) -> Option<u16> {
        match self {
            LlmError::Http { status, .. } => Some(*status),
            _ => None,
        }
    }
}

pub type Result<T> = std::result::Result<T, LlmError>;
