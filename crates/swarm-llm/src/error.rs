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

    /// Ответ получен, но текста в нём нет.
    ///
    /// Отдельный вариант, потому что это НЕ «битый JSON»: тело провайдера
    /// валидно, пусто именно поле с текстом. Реальный случай (найден живым
    /// прогоном на reasoning-модели `deepseek-v4.1-flash`): модель тратит
    /// весь `max_tokens` на `reasoning_content`, и к моменту обрыва
    /// `content` ещё не начат — провайдер отдаёт `"content": null`.
    /// Строго типизированное поле роняло десериализацию с диагнозом
    /// «response is not valid json», что уводило разбор инцидента в сторону:
    /// чинить надо было лимит токенов, а не парсер.
    #[error("llm response has no text content: {0}")]
    EmptyContent(String),
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

    /// Ответ обрезан лимитом токенов (`finish_reason=length`) либо пуст.
    ///
    /// Такой вызов имеет смысл повторить с БОЛЬШИМ лимитом: сам по себе он
    /// не транзиентный (повтор с тем же лимитом даст тот же обрыв), но
    /// исправимый — в отличие от «провайдер вернул мусор».
    pub fn is_length_truncated(&self) -> bool {
        matches!(self, LlmError::EmptyContent(_))
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
