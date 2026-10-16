//! Общий LLM-клиент (sync, `reqwest::blocking`).
//!
//! Провайдеры: `openai`, `anthropic`, `openai-compatible` (алиас `relayfast`),
//! `mock` (ответ из файла — для тестов).
//!
//! Вынесен из `swarm-planner` в отдельный crate (ТЗ v0.3 §2), чтобы Planner и
//! LLM-worker использовали один и тот же клиент без дублирования.

use serde::{Deserialize, Serialize};

pub mod error;

pub use error::LlmError;

/// Конфигурация LLM, читается из env.
#[derive(Debug, Clone)]
pub struct LlmConfig {
    pub provider: String,
    pub api_key: Option<String>,
    pub model: String,
    pub base_url: Option<String>,
    pub max_retries: u32,
    pub max_tokens: u32,
    pub mock_file: Option<String>,
    /// Таймаут одного HTTP-запроса, секунды.
    pub timeout_sec: u64,
}

impl LlmConfig {
    /// Читает конфигурацию из env с заданным префиксом (по умолчанию `LLM_`).
    ///
    /// Для обратной совместимости с Planner поддерживается префикс `PLANNER_LLM_`.
    pub fn from_env_prefixed(prefix: &str) -> Result<Self, LlmError> {
        let var = |name: &str| std::env::var(format!("{prefix}{name}"));
        // Обратная совместимость: PLANNER_LLM_* (если prefix == "PLANNER_").
        let legacy = |name: &str| std::env::var(format!("PLANNER_LLM_{name}"));

        // Причина: у VarError ровно два варианта, а для отсутствующей
        // переменной исходный текст не добавляет ничего к явному сообщению.
        // ast-grep-ignore: required-error-context
        let provider = var("PROVIDER")
            .or_else(|_| legacy("PROVIDER"))
            .map_err(|_| LlmError::Config(format!("{prefix}PROVIDER required")))?;
        let api_key = var("API_KEY").or_else(|_| legacy("API_KEY")).ok();
        if provider != "mock" && provider != "openai-compatible" && api_key.is_none() {
            return Err(LlmError::Config(
                "API key required for this provider".into(),
            ));
        }
        let model = var("MODEL")
            .or_else(|_| legacy("MODEL"))
            .unwrap_or_else(|_| "mock".into());
        let base_url = var("BASE_URL").or_else(|_| legacy("BASE_URL")).ok();
        if provider == "openai-compatible" && base_url.is_none() {
            return Err(LlmError::Config(
                "BASE_URL required for openai-compatible".into(),
            ));
        }
        let max_retries = var("MAX_RETRIES")
            .or_else(|_| std::env::var("PLANNER_MAX_RETRIES"))
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(2);
        let max_tokens = var("MAX_TOKENS")
            .or_else(|_| std::env::var("PLANNER_BUDGET_TOKENS"))
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(4096);
        let mock_file = var("MOCK_RESPONSE_FILE")
            .or_else(|_| legacy("MOCK_RESPONSE_FILE"))
            .ok();
        let timeout_sec = var("TIMEOUT_SEC")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(120);
        Ok(Self {
            provider,
            api_key,
            model,
            base_url,
            max_retries,
            max_tokens,
            mock_file,
            timeout_sec,
        })
    }

    /// Совместимость: читает `LLM_*`, при отсутствии — `PLANNER_LLM_*`.
    pub fn from_env() -> Result<Self, LlmError> {
        Self::from_env_prefixed("LLM_")
    }
}

/// Запрос к LLM.
#[derive(Debug, Clone)]
pub struct LlmRequest {
    pub system: String,
    pub user: String,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
    /// Модель для ЭТОГО запроса (Issue #7/#10). `None` → модель из конфига.
    ///
    /// Нужно, чтобы выбирать модель по измеренной стоимости успеха, не
    /// пересоздавая HTTP-клиент (в нём же ключ и base_url).
    pub model: Option<String>,
}

impl LlmRequest {
    /// Модель, которой реально уйдёт запрос: явная из запроса или из конфига.
    pub fn effective_model<'a>(&'a self, cfg_model: &'a str) -> &'a str {
        self.model.as_deref().unwrap_or(cfg_model)
    }
}

/// Ответ LLM.
#[derive(Debug, Clone)]
pub struct LlmResponse {
    pub text: String,
    pub tokens_in: u32,
    pub tokens_out: u32,
    pub model: String,
}

/// Трейт клиента — позволяет подменять реализацию в тестах.
pub trait LlmClient: Send + Sync {
    fn complete(&self, req: &LlmRequest) -> Result<LlmResponse, LlmError>;

    /// FIND-CODE-03: модель по умолчанию этого клиента — для аудита
    /// (`llm_calls.model`) при ошибке ДО ответа, когда реальное имя модели из
    /// ответа ещё неизвестно. Дефолт `"unknown"` сохраняет совместимость с
    /// тестовыми клиентами; HttpLlmClient отдаёт свою настроенную модель.
    fn default_model(&self) -> &str {
        "unknown"
    }
}

/// HTTP-клиент (blocking).
pub struct HttpLlmClient {
    cfg: LlmConfig,
    http: reqwest::blocking::Client,
}

#[derive(Serialize)]
struct ChatMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Serialize)]
struct OpenAiRequest<'a> {
    model: &'a str,
    messages: Vec<ChatMessage<'a>>,
    max_tokens: u32,
    temperature: f32,
}

#[derive(Deserialize)]
struct OpenAiChoice {
    message: OpenAiMessage,
}
#[derive(Deserialize)]
struct OpenAiMessage {
    /// Текст ответа. `Option` — потому что провайдер отдаёт `null`, когда
    /// модель израсходовала весь лимит токенов на размышление.
    ///
    /// Живой прогон (2026-10-16) на `deepseek-v4.1-flash`: при `finish_reason`
    /// `length` ответ приходит как `{"content":null,"reasoning_content":"..."}`.
    /// Строго типизированное `String` роняло ВСЮ десериализацию с диагнозом
    /// «response is not valid json», хотя JSON был валиден — врал не парсер.
    #[serde(default)]
    content: Option<String>,
    /// Ход размышления reasoning-моделей (DeepSeek и аналоги).
    /// Служит запасным источником текста, если `content` пуст.
    #[serde(default)]
    reasoning_content: Option<String>,
}

impl OpenAiMessage {
    /// Текст ответа: `content`, при его отсутствии — `reasoning_content`.
    ///
    /// Непустое `content` ВСЕГДА в приоритете: подменять рабочий ответ
    /// «сырыми мыслями» нельзя — потребитель (планировщик) парсит JSON, и
    /// рассуждения сломали бы разбор. Фолбэк нужен лишь там, где `content`
    /// пуст, то есть весь бюджет ушёл на reasoning.
    fn into_text(self) -> Option<String> {
        let has_text = |s: &str| !s.trim().is_empty();
        let take = |o: Option<String>| o.filter(|s| has_text(s));
        take(self.content).or_else(|| take(self.reasoning_content))
    }
}
#[derive(Deserialize)]
struct OpenAiUsage {
    #[serde(default)]
    prompt_tokens: u32,
    #[serde(default)]
    completion_tokens: u32,
}
#[derive(Deserialize)]
struct OpenAiResponse {
    choices: Vec<OpenAiChoice>,
    #[serde(default)]
    usage: Option<OpenAiUsage>,
    #[serde(default)]
    model: Option<String>,
}

#[derive(Serialize)]
struct AnthropicRequest<'a> {
    model: &'a str,
    max_tokens: u32,
    system: &'a str,
    messages: Vec<ChatMessage<'a>>,
}
#[derive(Deserialize)]
struct AnthropicContent {
    text: String,
}
#[derive(Deserialize)]
struct AnthropicUsage {
    #[serde(default)]
    input_tokens: u32,
    #[serde(default)]
    output_tokens: u32,
}
#[derive(Deserialize)]
struct AnthropicResponse {
    content: Vec<AnthropicContent>,
    #[serde(default)]
    usage: Option<AnthropicUsage>,
    #[serde(default)]
    model: Option<String>,
}

impl HttpLlmClient {
    pub fn new(cfg: LlmConfig) -> Result<Self, LlmError> {
        let http = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(cfg.timeout_sec))
            .build()
            .map_err(|e| LlmError::Transport(e.to_string()))?;
        Ok(Self { cfg, http })
    }

    pub fn config(&self) -> &LlmConfig {
        &self.cfg
    }

    fn dispatch(&self, req: &LlmRequest) -> Result<LlmResponse, LlmError> {
        let max_tokens = req.max_tokens.unwrap_or(self.cfg.max_tokens);
        match self.cfg.provider.as_str() {
            "mock" => self.mock_response(),
            "openai" => self.openai_chat(
                req,
                max_tokens,
                self.cfg
                    .base_url
                    .as_deref()
                    .unwrap_or("https://api.openai.com"),
            ),
            "openai-compatible" | "relayfast" => {
                let base = self
                    .cfg
                    .base_url
                    .as_deref()
                    .ok_or_else(|| LlmError::Config("base_url required".into()))?;
                self.openai_chat(req, max_tokens, base)
            }
            "anthropic" => self.anthropic_chat(req, max_tokens),
            other => Err(LlmError::Config(format!("unknown provider: {other}"))),
        }
    }

    fn mock_response(&self) -> Result<LlmResponse, LlmError> {
        let path = self
            .cfg
            .mock_file
            .as_deref()
            .ok_or_else(|| LlmError::Config("mock requires MOCK_RESPONSE_FILE".into()))?;
        // Волна 5.2: поддержка ПОСЛЕДОВАТЕЛЬНОСТИ ответов.
        // Если MOCK_RESPONSE_FILE — шаблон со звёздочкой (`dir/case*.json`),
        // мок выдаёт файлы по порядку сортировки при каждом вызове, сохраняя
        // счётчик в файле `dir/.mock_cursor`. Позволяет честно тестировать
        // retry (первый ответ невалиден, второй — валиден), чего одиночный
        // файл не давал (тест `planner_retries_on_invalid_json` был вакуумным).
        if let Some(dir) = path.strip_suffix("*") {
            return self.mock_sequence(dir);
        }
        let text = std::fs::read_to_string(path)
            .map_err(|e| LlmError::Transport(format!("mock file {path}: {e}")))?;
        Ok(LlmResponse {
            text,
            tokens_in: 0,
            tokens_out: 0,
            model: "mock".into(),
        })
    }

    /// Выдать следующий файл из последовательности `dir*` по счётчику.
    fn mock_sequence(&self, dir: &str) -> Result<LlmResponse, LlmError> {
        let dir = dir.trim_end_matches('/').to_string();
        let mut files: Vec<String> = std::fs::read_dir(&dir)
            .map_err(|e| LlmError::Transport(format!("mock seq dir {dir}: {e}")))?
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".json"))
            .collect();
        files.sort();
        if files.is_empty() {
            return Err(LlmError::Config(format!("mock seq dir {dir}: no *.json")));
        }
        let cursor_path = format!("{dir}/.mock_cursor");
        let n: usize = std::fs::read_to_string(&cursor_path)
            .ok()
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(0);
        let idx = n.min(files.len() - 1); // после конца — последний ответ
        std::fs::write(&cursor_path, (idx + 1).to_string())
            .map_err(|e| LlmError::Transport(format!("mock cursor {cursor_path}: {e}")))?;
        let text = std::fs::read_to_string(format!("{dir}/{}", files[idx]))
            .map_err(|e| LlmError::Transport(format!("mock seq file: {e}")))?;
        Ok(LlmResponse {
            text,
            tokens_in: 0,
            tokens_out: 0,
            model: "mock".into(),
        })
    }

    fn openai_chat(
        &self,
        req: &LlmRequest,
        max_tokens: u32,
        base: &str,
    ) -> Result<LlmResponse, LlmError> {
        let url = format!("{}/v1/chat/completions", base.trim_end_matches('/'));
        let body = OpenAiRequest {
            model: req.effective_model(&self.cfg.model),
            messages: vec![
                ChatMessage {
                    role: "system",
                    content: &req.system,
                },
                ChatMessage {
                    role: "user",
                    content: &req.user,
                },
            ],
            max_tokens,
            temperature: req.temperature.unwrap_or(0.2),
        };
        let mut r = self.http.post(&url).json(&body);
        if let Some(k) = &self.cfg.api_key {
            r = r.bearer_auth(k);
        }
        let resp = r.send().map_err(|e| LlmError::Transport(e.to_string()))?;
        let status = resp.status();
        let text = resp
            .text()
            .map_err(|e| LlmError::Transport(e.to_string()))?;
        if !status.is_success() {
            return Err(LlmError::Http {
                status: status.as_u16(),
                body: truncate(&text, 400),
            });
        }
        let parsed: OpenAiResponse = serde_json::from_str(&text)
            .map_err(|e| LlmError::InvalidJson(format!("{e}: {}", truncate(&text, 400))))?;
        let usage = parsed.usage.unwrap_or(OpenAiUsage {
            prompt_tokens: 0,
            completion_tokens: 0,
        });
        parsed
            .choices
            .into_iter()
            .next()
            .and_then(|c| {
                let text = c.message.into_text()?;
                Some(LlmResponse {
                    text,
                    tokens_in: usage.prompt_tokens,
                    tokens_out: usage.completion_tokens,
                    model: parsed.model.unwrap_or_else(|| self.cfg.model.clone()),
                })
            })
            .ok_or_else(|| {
                LlmError::EmptyContent(format!(
                    "choices[0].message has no content and no reasoning_content; finish_reason unknown; raw={}",
                    truncate(&text, 200)
                ))
            })
    }

    fn anthropic_chat(&self, req: &LlmRequest, max_tokens: u32) -> Result<LlmResponse, LlmError> {
        let base = self
            .cfg
            .base_url
            .as_deref()
            .unwrap_or("https://api.anthropic.com");
        let url = format!("{}/v1/messages", base.trim_end_matches('/'));
        let body = AnthropicRequest {
            model: req.effective_model(&self.cfg.model),
            max_tokens,
            system: &req.system,
            messages: vec![ChatMessage {
                role: "user",
                content: &req.user,
            }],
        };
        let key = self
            .cfg
            .api_key
            .as_deref()
            .ok_or_else(|| LlmError::Config("anthropic requires api key".into()))?;
        let resp = self
            .http
            .post(&url)
            .header("x-api-key", key)
            .header("anthropic-version", "2023-06-01")
            .json(&body)
            .send()
            .map_err(|e| LlmError::Transport(e.to_string()))?;
        let status = resp.status();
        let text = resp
            .text()
            .map_err(|e| LlmError::Transport(e.to_string()))?;
        if !status.is_success() {
            return Err(LlmError::Http {
                status: status.as_u16(),
                body: truncate(&text, 400),
            });
        }
        let parsed: AnthropicResponse = serde_json::from_str(&text)
            .map_err(|e| LlmError::InvalidJson(format!("{e}: {}", truncate(&text, 400))))?;
        let usage = parsed.usage.unwrap_or(AnthropicUsage {
            input_tokens: 0,
            output_tokens: 0,
        });
        parsed
            .content
            .into_iter()
            .next()
            .map(|c| LlmResponse {
                text: c.text,
                tokens_in: usage.input_tokens,
                tokens_out: usage.output_tokens,
                model: parsed.model.unwrap_or_else(|| self.cfg.model.clone()),
            })
            .ok_or_else(|| LlmError::InvalidJson("empty content".into()))
    }
}

impl LlmClient for HttpLlmClient {
    fn complete(&self, req: &LlmRequest) -> Result<LlmResponse, LlmError> {
        self.dispatch(req)
    }

    fn default_model(&self) -> &str {
        &self.cfg.model
    }
}

/// Фабрика из env.
pub fn from_env() -> Result<Box<dyn LlmClient>, LlmError> {
    let cfg = LlmConfig::from_env()?;
    Ok(Box::new(HttpLlmClient::new(cfg)?))
}

/// Фабрика из env с префиксом.
pub fn from_env_prefixed(prefix: &str) -> Result<Box<dyn LlmClient>, LlmError> {
    let cfg = LlmConfig::from_env_prefixed(prefix)?;
    Ok(Box::new(HttpLlmClient::new(cfg)?))
}

/// Обрезает строку до `n` БАЙТ, не разрывая символ UTF-8.
///
/// F18: тело ответа провайдера может быть на русском/китайском (провайдеры
/// отвечают на языке запроса). Старая реализация делала `&s[..n]` — срез по
/// байту; если `n` попадал в середину многобайтового символа, это **паника**
/// (`byte index N is not a char boundary`), а не обрезка. Паника в обработчике
/// ошибки прячет исходную ошибку провайдера и роняет воркер. Проверено на
/// `truncate("ыыыы", 3)` — паниковало до фикса.
fn truncate(s: &str, n: usize) -> String {
    if s.len() <= n {
        return s.to_string();
    }
    // Отступаем к ближайшей границе символа вниз.
    let mut end = n;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &s[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    // F18: обрезка тела ответа не должна паниковать на многобайтовом UTF-8.
    // Провайдеры отвечают на языке запроса (русский/китайский); байтовый срез
    // `&s[..n]` попадал в середину символа и ронял воркер паникой вместо
    // обрезки. Проверено: до фикса `truncate("ыыыы", 3)` паниковало.
    #[test]
    fn truncate_is_utf8_safe() {
        // 3 байта — середина второго символа 'ы' (2 байта).
        let out = truncate("ыыыы", 3);
        assert!(out.ends_with('…'), "обрезанная строка помечена: {out:?}");
        // Обрезка не длиннее лимита + 3 байта самого многоточия ('…').
        assert!(
            out.len() <= 3 + '…'.len_utf8(),
            "результат не превышает границу: {out:?}"
        );
        // Китайский: 3 байта на символ.
        let out = truncate("中文字符", 4);
        assert!(out.ends_with('…'));
        // Строка короче лимита возвращается целиком, без маркера.
        assert_eq!(truncate("ок", 400), "ок");
        // ASCII на границе — без паники.
        assert_eq!(truncate("abcdef", 3), "abc…");
    }

    #[test]
    fn mock_returns_file_contents() {
        let dir = std::env::temp_dir();
        let p = dir.join(format!("swarm_llm_mock_{}.txt", std::process::id()));
        std::fs::write(&p, "hello-from-mock").unwrap();
        let cfg = LlmConfig {
            provider: "mock".into(),
            api_key: None,
            model: "mock".into(),
            base_url: None,
            max_retries: 0,
            max_tokens: 16,
            mock_file: Some(p.to_string_lossy().into_owned()),
            timeout_sec: 5,
        };
        let c = HttpLlmClient::new(cfg).unwrap();
        let r = c
            .complete(&LlmRequest {
                system: "s".into(),
                user: "u".into(),
                max_tokens: None,
                temperature: None,
                model: None,
            })
            .unwrap();
        assert_eq!(r.text, "hello-from-mock");
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn mock_sequence_advances_per_call() {
        let dir = std::env::temp_dir().join(format!("swarm_llm_seq_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("001_invalid.json"), "not json").unwrap();
        std::fs::write(dir.join("002_valid.json"), "{\"ok\":true}").unwrap();
        let cfg = LlmConfig {
            provider: "mock".into(),
            api_key: None,
            model: "mock".into(),
            base_url: None,
            max_retries: 0,
            max_tokens: 16,
            mock_file: Some(format!("{}*", dir.display())),
            timeout_sec: 5,
        };
        let c = HttpLlmClient::new(cfg).unwrap();
        let req = LlmRequest {
            system: "s".into(),
            user: "u".into(),
            max_tokens: None,
            temperature: None,
            model: None,
        };
        let r1 = c.complete(&req).unwrap();
        assert_eq!(r1.text, "not json", "первый ответ — невалидный");
        let r2 = c.complete(&req).unwrap();
        assert_eq!(r2.text, "{\"ok\":true}", "второй ответ — валидный");
        let r3 = c.complete(&req).unwrap();
        assert_eq!(r3.text, "{\"ok\":true}", "после конца — последний");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unknown_provider_errors() {
        let cfg = LlmConfig {
            provider: "nope".into(),
            api_key: Some("x".into()),
            model: "m".into(),
            base_url: None,
            max_retries: 0,
            max_tokens: 16,
            mock_file: None,
            timeout_sec: 5,
        };
        let c = HttpLlmClient::new(cfg).unwrap();
        let err = c.complete(&LlmRequest {
            system: "s".into(),
            user: "u".into(),
            max_tokens: None,
            temperature: None,
            model: None,
        });
        assert!(matches!(err, Err(LlmError::Config(_))));
    }

    // --- reasoning_content: content=null (живой прогон 2026-10-16) ---

    #[test]
    fn openai_message_deserializes_content_null_with_reasoning() {
        // Реальный ответ deepseek-v4.1-flash при обрыве по max_tokens:
        // content=null, весь текст — в reasoning_content.
        let raw = r#"{"content":null,"reasoning_content":"The task: design a REST API..."}"#;
        let m: OpenAiMessage = serde_json::from_str(raw).unwrap();
        assert!(m.content.is_none());
        assert!(m.reasoning_content.is_some());
        let text = m.into_text().unwrap();
        assert!(text.contains("design a REST API"));
    }

    #[test]
    fn openai_message_deserializes_content_present() {
        // Нормальный ответ: content заполнен, reasoning_content может быть.
        let raw = r#"{"content":"hello","reasoning_content":"thinking..."}"#;
        let m: OpenAiMessage = serde_json::from_str(raw).unwrap();
        // content в приоритете — reasoning не подменяет рабочий ответ.
        let text = m.into_text().unwrap();
        assert_eq!(text, "hello");
    }

    #[test]
    fn openai_message_deserializes_content_empty_falls_back_to_reasoning() {
        // content="" (пустая строка) — тоже fallback на reasoning.
        let raw = r#"{"content":"","reasoning_content":"fallback text"}"#;
        let m: OpenAiMessage = serde_json::from_str(raw).unwrap();
        let text = m.into_text().unwrap();
        assert_eq!(text, "fallback text");
    }

    #[test]
    fn openai_message_both_empty_returns_none() {
        // content=null, reasoning_content=null → нет текста → EmptyContent.
        let raw = r#"{"content":null,"reasoning_content":null}"#;
        let m: OpenAiMessage = serde_json::from_str(raw).unwrap();
        assert!(m.into_text().is_none());
    }

    #[test]
    fn openai_message_missing_fields_defaults_to_none() {
        // Поля вообще отсутствуют (старый провайдер без reasoning_content).
        let raw = r#"{}"#;
        let m: OpenAiMessage = serde_json::from_str(raw).unwrap();
        assert!(m.into_text().is_none());
    }

    #[test]
    fn openai_response_with_null_content_parses_without_invalid_json() {
        // Полный ответ провайдера с content=null — до фикса падал с
        // «response is not valid json: invalid type: null, expected a string».
        // Сейчас десериализация проходит, а пустой ответ даёт EmptyContent.
        let raw = r#"{"id":"abc","model":"deepseek-v4.1-flash","object":"chat.completion","choices":[{"index":0,"message":{"role":"assistant","content":null,"reasoning_content":"thinking only"},"finish_reason":"length"}],"usage":{"prompt_tokens":10,"completion_tokens":400,"total_tokens":410}}"#;
        let parsed: OpenAiResponse = serde_json::from_str(raw).unwrap();
        let c = parsed.choices.into_iter().next().unwrap();
        let text = c.message.into_text().unwrap();
        assert_eq!(text, "thinking only");
    }

    #[test]
    fn empty_content_error_is_not_transient_but_is_length_truncated() {
        // EmptyContent — не транзиентная (повтор с тем же лимитом даст тот же
        // обрыв), но is_length_truncated=true → воркер может ретраить с
        // большим лимитом.
        let e = LlmError::EmptyContent("test".into());
        assert!(!e.is_transient());
        assert!(e.is_length_truncated());
    }
}
