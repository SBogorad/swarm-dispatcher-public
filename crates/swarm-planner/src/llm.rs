//! Шим над `swarm-llm` для обратной совместимости Planner.
//!
//! Сам HTTP-клиент вынесен в crate `swarm-llm` (ТЗ v0.3 §2). Здесь остаётся
//! только тонкая обёртка с прежним публичным API (`LlmConfig`, `LlmClient::new`,
//! `config()`, `complete(system, user) -> Result<String>`), чтобы существующие
//! тесты и код Planner не менялись.

use crate::error::{PlannerError, Result};
use swarm_llm::{HttpLlmClient, LlmClient as _, LlmError, LlmRequest};

/// Конфигурация LLM (совместимый вид для Planner).
#[derive(Debug, Clone)]
pub struct LlmConfig {
    pub provider: String,
    pub api_key: Option<String>,
    pub model: String,
    pub base_url: Option<String>,
    pub max_retries: u32,
    pub max_tokens: u32,
    pub mock_file: Option<String>,
}

impl LlmConfig {
    /// Читает конфигурацию из env (прежние имена `PLANNER_LLM_*`).
    pub fn from_env() -> Result<Self> {
        let provider = std::env::var("PLANNER_LLM_PROVIDER")
            .map_err(|_| PlannerError::Config("PLANNER_LLM_PROVIDER required".into()))?;
        let api_key = std::env::var("PLANNER_LLM_API_KEY").ok();
        if provider != "mock"
            && provider != "openai-compatible"
            && provider != "relayfast"
            && api_key.is_none()
        {
            return Err(PlannerError::Config(
                "PLANNER_LLM_API_KEY required for this provider".into(),
            ));
        }
        let model = std::env::var("PLANNER_LLM_MODEL").unwrap_or_else(|_| "mock".into());
        let base_url = std::env::var("PLANNER_LLM_BASE_URL").ok();
        if provider == "openai-compatible" && base_url.is_none() {
            return Err(PlannerError::Config(
                "PLANNER_LLM_BASE_URL required for openai-compatible".into(),
            ));
        }
        let max_retries = std::env::var("PLANNER_MAX_RETRIES")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(2);
        let max_tokens = std::env::var("PLANNER_BUDGET_TOKENS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(50_000);
        let mock_file = std::env::var("PLANNER_LLM_MOCK_RESPONSE_FILE").ok();
        Ok(Self {
            provider,
            api_key,
            model,
            base_url,
            max_retries,
            max_tokens,
            mock_file,
        })
    }

    fn to_swarm_llm(&self) -> swarm_llm::LlmConfig {
        swarm_llm::LlmConfig {
            provider: self.provider.clone(),
            api_key: self.api_key.clone(),
            model: self.model.clone(),
            base_url: self.base_url.clone(),
            max_retries: self.max_retries,
            max_tokens: self.max_tokens,
            mock_file: self.mock_file.clone(),
            timeout_sec: 120,
        }
    }
}

/// Клиент LLM для Planner — обёртка над `swarm_llm::HttpLlmClient`.
pub struct LlmClient {
    inner: HttpLlmClient,
    cfg: LlmConfig,
}

impl LlmClient {
    pub fn new(cfg: LlmConfig) -> Result<Self> {
        let inner =
            HttpLlmClient::new(cfg.to_swarm_llm()).map_err(|e| PlannerError::Llm(e.to_string()))?;
        Ok(Self { inner, cfg })
    }

    pub fn config(&self) -> &LlmConfig {
        &self.cfg
    }

    /// Один вызов LLM. Возвращает текст ответа (или содержимое mock-файла).
    pub fn complete(&self, system: &str, user: &str) -> Result<String> {
        let req = LlmRequest {
            system: system.to_string(),
            user: user.to_string(),
            max_tokens: None,
            temperature: None,
        };
        self.inner.complete(&req).map(|r| r.text).map_err(map_err)
    }
}

fn map_err(e: LlmError) -> PlannerError {
    match e {
        LlmError::Config(m) => PlannerError::Config(m),
        LlmError::InvalidJson(m) => PlannerError::InvalidJson(m),
        other => PlannerError::Llm(other.to_string()),
    }
}
