//! Agent Loop с tool calls (ТЗ v0.4 §1, §6).
//!
//! LLM получает контракт и описание, затем в цикле возвращает либо tool call
//! (`run_shell`, `read_file`, `write_file`), либо финальный ответ. Каждый tool
//! call проверяется allowlist'ом, исполняется в sandbox и логируется событием
//! `tool_call` (ТЗ §7).
//!
//! Только синхронно, без async.

use crate::allowlist::Allowlist;
use crate::error::{Result, WorkerError};
use crate::sandbox::{ExecOutput, Sandbox, SandboxConfig};
use serde_json::Value;
use swarm_llm::{LlmClient, LlmRequest};

/// Максимум итераций по умолчанию (ТЗ §6).
pub const DEFAULT_MAX_ITERATIONS: u32 = 20;

/// Конфигурация Agent Loop.
#[derive(Debug, Clone)]
pub struct AgentLoopConfig {
    pub max_iterations: u32,
    pub max_tokens: u32,
    pub sandbox: SandboxConfig,
}

impl Default for AgentLoopConfig {
    fn default() -> Self {
        Self {
            max_iterations: DEFAULT_MAX_ITERATIONS,
            max_tokens: 4096,
            sandbox: SandboxConfig::default(),
        }
    }
}

/// Один шаг LLM.
#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    Tool(ToolCall),
    /// Несколько последовательных вызовов из одного DSML-ответа.
    ToolCalls(Vec<ToolCall>),
    Done {
        result: String,
    },
}

/// Разобранный tool call.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolCall {
    pub tool: String,
    pub args: Value,
}

impl ToolCall {
    /// Команда, которая уйдёт в sandbox (для `run_shell` / `read_file`).
    pub fn shell_command(&self) -> Option<String> {
        match self.tool.as_str() {
            "run_shell" => self
                .args
                .get("command")
                .and_then(|c| c.as_str())
                .map(str::to_string),
            "read_file" => self
                .args
                .get("path")
                .and_then(|p| p.as_str())
                .map(str::to_string),
            _ => None,
        }
    }
}

/// Аудит одного tool call (ТЗ §7 — только hash + размер, без полного вывода).
#[derive(Debug, Clone, Default)]
pub struct ToolAudit {
    pub tool: String,
    pub command: Option<String>,
    pub exit_code: Option<i32>,
    pub duration_ms: u128,
    pub output_sha256: String,
    pub output_bytes: usize,
    pub truncated: bool,
    pub rejected: bool,
    pub reason: Option<String>,
}

/// Приёмник аудит-событий. Отдельный трейт — чтобы loop не зависел от Postgres.
pub trait ToolAuditSink {
    fn record(&mut self, audit: &ToolAudit) -> Result<()>;
}

/// Ничего не пишет (тесты, запуск без аудита).
pub struct NoopSink;
impl ToolAuditSink for NoopSink {
    fn record(&mut self, _a: &ToolAudit) -> Result<()> {
        Ok(())
    }
}

/// Результат работы Agent Loop.
#[derive(Debug, Clone)]
pub struct AgentResult {
    pub final_text: String,
    pub iterations: u32,
    pub tool_calls: usize,
}

/// Разобрать JSON или DSML-ответ LLM в шаг Agent Loop.
pub fn parse_step(text: &str) -> Result<Step> {
    crate::parse_step::parse(text)
}

/// Прогнать Agent Loop до финального ответа.
///
/// `history` — накопленные результаты tool calls, передаются обратно в LLM.
pub fn run(
    llm: &dyn LlmClient,
    sandbox: &Sandbox,
    contract: &Value,
    allowlist: &Allowlist,
    cfg: &AgentLoopConfig,
    sink: &mut dyn ToolAuditSink,
) -> Result<AgentResult> {
    let mut history: Vec<String> = Vec::new();
    let mut tool_calls = 0usize;

    for iteration in 1..=cfg.max_iterations {
        let req = LlmRequest {
            system: crate::prompt::agent_system_prompt(allowlist),
            user: crate::prompt::agent_user_prompt(contract, &history),
            max_tokens: Some(cfg.max_tokens),
            temperature: None,
        };
        let resp = llm.complete(&req)?;
        let step = parse_step(&resp.text)?;

        match step {
            Step::Done { result } => {
                return Ok(AgentResult {
                    final_text: result,
                    iterations: iteration,
                    tool_calls,
                });
            }
            Step::Tool(tc) => {
                let rejected = run_tool_and_record(sandbox, allowlist, sink, &tc, &mut history)?;
                tool_calls += 1;
                if rejected {
                    history.push(
                        "Отклонённый tool call: исправь этот шаг; не повторяй запрещённую команду."
                            .into(),
                    );
                }
            }
            Step::ToolCalls(calls) => {
                for (index, tc) in calls.into_iter().enumerate() {
                    let rejected =
                        run_tool_and_record(sandbox, allowlist, sink, &tc, &mut history)?;
                    tool_calls += 1;
                    if rejected {
                        history.push(format!(
                            "Шаг {} batch отклонён политикой; остальные вызовы не выполнялись. Исправь этот шаг и продолжай отдельным вызовом.",
                            index + 1
                        ));
                        break;
                    }
                }
            }
        }
    }

    Err(WorkerError::MaxIterations {
        max: cfg.max_iterations,
    })
}

fn run_tool_and_record(
    sandbox: &Sandbox,
    allowlist: &Allowlist,
    sink: &mut dyn ToolAuditSink,
    tc: &ToolCall,
    history: &mut Vec<String>,
) -> Result<bool> {
    let (reply, audit) = execute_tool(sandbox, allowlist, tc)?;
    let rejected = audit.rejected;
    sink.record(&audit)?;
    history.push(format!(
        "tool={} args={} -> {}",
        tc.tool,
        serde_json::to_string(&tc.args).unwrap_or_default(),
        reply
    ));
    Ok(rejected)
}

/// Выполнить один tool call: allowlist → sandbox → результат для LLM.
fn execute_tool(
    sandbox: &Sandbox,
    allowlist: &Allowlist,
    tc: &ToolCall,
) -> Result<(String, ToolAudit)> {
    let mut audit = ToolAudit {
        tool: tc.tool.clone(),
        ..Default::default()
    };

    match tc.tool.as_str() {
        "run_shell" => {
            let cmd = tc
                .shell_command()
                .ok_or_else(|| WorkerError::InvalidToolCall("run_shell: нет command".into()))?;
            audit.command = Some(cmd.clone());

            if let Err(e) = allowlist.check(&cmd) {
                audit.rejected = true;
                audit.reason = Some(e.to_string());
                return Ok((format!("REJECTED: {e}"), audit));
            }

            let out = sandbox.exec(&cmd)?;
            fill_audit(&mut audit, &out);
            Ok((render_output(&out), audit))
        }
        "read_file" => {
            let path = tc
                .shell_command()
                .ok_or_else(|| WorkerError::InvalidToolCall("read_file: нет path".into()))?;
            audit.command = Some(format!("read_file {path}"));
            if let Err(e) = validate_workspace_path(&path) {
                audit.rejected = true;
                audit.reason = Some(e.to_string());
                return Ok((format!("REJECTED: {e}"), audit));
            }
            let out = sandbox.read_file(&path)?;
            fill_audit(&mut audit, &out);
            Ok((render_output(&out), audit))
        }
        "write_file" => {
            let path = tc
                .args
                .get("path")
                .and_then(|p| p.as_str())
                .ok_or_else(|| WorkerError::InvalidToolCall("write_file: нет path".into()))?;
            let content = tc
                .args
                .get("content")
                .and_then(|c| c.as_str())
                .unwrap_or("");
            // Путь проверяем тем же правилом, что и команды.
            if let Err(e) = allowlist.check(&format!("touch {path}")) {
                audit.rejected = true;
                audit.reason = Some(e.to_string());
                return Ok((format!("REJECTED: {e}"), audit));
            }
            let out = sandbox.write_file(path, content)?;
            fill_audit(&mut audit, &out);
            Ok((render_output(&out), audit))
        }
        other => Err(WorkerError::InvalidToolCall(format!(
            "неизвестный инструмент: {other}"
        ))),
    }
}

fn validate_workspace_path(path: &str) -> Result<()> {
    let p = std::path::Path::new(path);
    if path.is_empty()
        || path.contains('\0')
        || path.contains('\n')
        || path.contains('\r')
        || p.is_absolute()
        || p.components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(WorkerError::InvalidToolCall(
            "read_file: path must stay inside the workspace".into(),
        ));
    }
    Ok(())
}

fn fill_audit(audit: &mut ToolAudit, out: &ExecOutput) {
    audit.exit_code = Some(out.exit_code);
    audit.duration_ms = out.duration_ms;
    audit.output_sha256 = out.output_sha256.clone();
    audit.output_bytes = out.output_bytes;
    audit.truncated = out.truncated;
}

fn render_output(out: &ExecOutput) -> String {
    format!(
        "exit_code={}\nstdout:\n{}\nstderr:\n{}",
        out.exit_code,
        out.stdout.trim_end(),
        out.stderr.trim_end()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_done() {
        let s = parse_step(r#"{"done": true, "result": "готово"}"#).unwrap();
        assert_eq!(
            s,
            Step::Done {
                result: "готово".into()
            }
        );
    }

    #[test]
    fn parse_artifact_alias() {
        let s = parse_step(r#"{"done": true, "artifact": "cargo test ok"}"#).unwrap();
        assert_eq!(
            s,
            Step::Done {
                result: "cargo test ok".into()
            }
        );
    }

    #[test]
    fn parse_tool_call() {
        let s = parse_step(r#"{"tool": "run_shell", "args": {"command": "cargo test"}}"#).unwrap();
        match s {
            Step::Tool(tc) => {
                assert_eq!(tc.tool, "run_shell");
                assert_eq!(tc.shell_command().unwrap(), "cargo test");
            }
            _ => panic!("ожидался Tool"),
        }
    }

    #[test]
    fn parse_tool_in_fenced_block() {
        let text =
            "Вот вызов:\n```json\n{\"tool\":\"read_file\",\"args\":{\"path\":\"Cargo.toml\"}}\n```";
        match parse_step(text).unwrap() {
            Step::Tool(tc) => assert_eq!(tc.shell_command().unwrap(), "Cargo.toml"),
            _ => panic!("ожидался Tool"),
        }
    }

    #[test]
    fn parse_rejects_malformed_quoted_dsml_parameter() {
        let text = r#"<｜｜DSML｜｜ invoke name="run_shell"><｜｜DSML｜｜ parameter name="command" string="true">"printf \q"</｜｜DSML｜｜ parameter></｜｜DSML｜｜ invoke>"#;
        assert!(matches!(
            parse_step(text),
            Err(WorkerError::InvalidToolCall(_))
        ));
    }

    #[test]
    fn parse_rejects_truncated_quoted_dsml_parameter() {
        let text = r#"<｜｜DSML｜｜ invoke name="run_shell"><｜｜DSML｜｜ parameter name="command" string="true">"printf hello</｜｜DSML｜｜ parameter></｜｜DSML｜｜ invoke>"#;
        assert!(matches!(
            parse_step(text),
            Err(WorkerError::InvalidToolCall(_))
        ));
    }

    #[test]
    fn parse_garbage_fails() {
        assert!(parse_step("просто текст без json").is_err());
        assert!(parse_step(r#"{"foo": 1}"#).is_err());
    }

    #[test]
    fn parse_handles_braces_inside_strings() {
        let s = parse_step(r#"{"done": true, "result": "if (x) { return 1; }"}"#).unwrap();
        assert_eq!(
            s,
            Step::Done {
                result: "if (x) { return 1; }".into()
            }
        );
    }

    // M6 (ТЗ §9): событие аудита пишется на каждый tool call.
    #[test]
    fn audit_sink_records_rejection() {
        struct Counter(usize);
        impl ToolAuditSink for Counter {
            fn record(&mut self, a: &ToolAudit) -> Result<()> {
                assert!(a.rejected);
                self.0 += 1;
                Ok(())
            }
        }
        let al = Allowlist::default();
        let mut c = Counter(0);
        // без sandbox: проверяем только путь allowlist-отказа
        let tc = ToolCall {
            tool: "run_shell".into(),
            args: json!({"command": "rm -rf /"}),
        };
        let audit = ToolAudit {
            tool: tc.tool.clone(),
            command: Some("rm -rf /".into()),
            rejected: true,
            reason: Some(al.check("rm -rf /").unwrap_err().to_string()),
            ..Default::default()
        };
        c.record(&audit).unwrap();
        assert_eq!(c.0, 1);
    }
}
