//! Разбор ответа LLM в шаг Agent Loop (ТЗ v0.4 §6).
//!
//! Поддерживает JSON-формат и DSML, который возвращает DeepSeek через
//! OpenAI-compatible endpoint. DSML-маркеры намеренно разбираются как текст,
//! поскольку разметка содержит нестандартные fullwidth-токены.

use crate::agent_loop::{Step, ToolCall};
use crate::error::{Result, WorkerError};
use serde_json::{Map, Value};

const INVOKE_OPEN: &str = "<｜｜DSML｜｜ invoke ";
const INVOKE_CLOSE: &str = "</｜｜DSML｜｜ invoke>";
const PARAM_OPEN: &str = "<｜｜DSML｜｜ parameter ";
const PARAM_CLOSE: &str = "</｜｜DSML｜｜ parameter>";

/// Разбирает JSON- или DSML-ответ в один шаг Agent Loop.
/// При нескольких последовательных DSML invoke возвращает все tool calls.
pub fn parse(text: &str) -> Result<Step> {
    if text.contains(INVOKE_OPEN) {
        let calls = parse_dsml(text).map_err(|e| {
            WorkerError::InvalidToolCall(format!(
                "невалидный DSML tool call: {e}; ответ: {}",
                brief(text)
            ))
        })?;
        return match calls.len() {
            0 => Err(WorkerError::InvalidToolCall(format!(
                "DSML не содержит invoke: {}",
                brief(text)
            ))),
            1 => Ok(Step::Tool(calls.into_iter().next().expect("one call"))),
            _ => Ok(Step::ToolCalls(calls)),
        };
    }

    parse_json(text)
}

fn parse_json(text: &str) -> Result<Step> {
    let trimmed = text.trim();
    let fenced = trimmed.find("```json").and_then(|start| {
        let content_start = start + "```json".len();
        trimmed[content_start..]
            .find("```")
            .map(|end| trimmed[content_start..content_start + end].trim())
    });
    let candidate = fenced.unwrap_or(trimmed);
    // Do not fish arbitrary braces out of prose. JSON mode accepts only a full
    // JSON object (optionally fenced); malformed prose becomes a visible error.
    let json = if candidate.starts_with('{') {
        let end = extract_json_object(candidate).ok_or_else(|| {
            WorkerError::InvalidToolCall(format!("незакрытый JSON-объект: {}", brief(text)))
        })?;
        if end.len() != candidate.len() {
            return Err(WorkerError::InvalidToolCall(format!(
                "лишний текст после JSON-объекта: {}",
                brief(text)
            )));
        }
        end
    } else {
        return Err(WorkerError::InvalidToolCall(format!(
            "ответ не является DSML или JSON-объектом: {}",
            brief(text)
        )));
    };
    let v: Value = serde_json::from_str(&json).map_err(|e| {
        WorkerError::InvalidToolCall(format!("невалидный JSON: {e}; ответ: {}", brief(text)))
    })?;

    if v.get("done").and_then(Value::as_bool).unwrap_or(false) {
        let result = v
            .get("result")
            .or_else(|| v.get("artifact"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        return Ok(Step::Done { result });
    }
    if let Some(tool) = v.get("tool").and_then(Value::as_str) {
        return Ok(Step::Tool(ToolCall {
            tool: tool.to_string(),
            args: v.get("args").cloned().unwrap_or(Value::Null),
        }));
    }
    Err(WorkerError::InvalidToolCall(format!(
        "ни 'done', ни 'tool' не найдены: {}",
        brief(text)
    )))
}

fn parse_dsml(text: &str) -> std::result::Result<Vec<ToolCall>, String> {
    let mut rest = text;
    let mut calls = Vec::new();
    while let Some(start) = rest.find(INVOKE_OPEN) {
        rest = &rest[start + INVOKE_OPEN.len()..];
        let header_end = rest
            .find('>')
            .ok_or("у invoke отсутствует закрывающий '>'")?;
        let header = &rest[..header_end];
        let name = quoted_attribute(header, "name")?.ok_or("у invoke отсутствует name")?;
        if name.is_empty() {
            return Err("пустое имя инструмента".into());
        }
        rest = &rest[header_end + 1..];
        let invoke_end = rest
            .find(INVOKE_CLOSE)
            .ok_or("у invoke отсутствует закрывающий тег")?;
        if rest[..invoke_end].contains(INVOKE_OPEN) {
            return Err("вложенный invoke до закрытия текущего invoke".into());
        }
        let body = &rest[..invoke_end];
        let mut args = Map::new();
        let mut body_rest = body;
        while let Some(param_start) = body_rest.find(PARAM_OPEN) {
            body_rest = &body_rest[param_start + PARAM_OPEN.len()..];
            let param_header_end = body_rest
                .find('>')
                .ok_or("у parameter отсутствует закрывающий '>'")?;
            let param_header = &body_rest[..param_header_end];
            let param_name =
                quoted_attribute(param_header, "name")?.ok_or("у parameter отсутствует name")?;
            if param_name.is_empty() {
                return Err("пустое имя parameter".into());
            }
            let is_string = match quoted_attribute(param_header, "string")?.as_deref() {
                Some("true") => true,
                Some("false") => false,
                Some(other) => return Err(format!("неизвестный тип parameter string={other:?}")),
                None => return Err("у parameter отсутствует string".into()),
            };
            body_rest = &body_rest[param_header_end + 1..];
            let param_end = body_rest
                .find(PARAM_CLOSE)
                .ok_or("у parameter отсутствует закрывающий тег")?;
            if body_rest[..param_end].contains(PARAM_OPEN) {
                return Err("вложенный parameter до закрытия текущего parameter".into());
            }
            let raw_value = body_rest[..param_end].trim();
            let value = if is_string {
                decode_string_value(raw_value).map_err(|e| {
                    format!("parameter {param_name:?} содержит невалидную JSON-строку: {e}")
                })?
            } else {
                serde_json::from_str(raw_value).map_err(|e| {
                    format!("parameter {param_name:?} помечен string=false, но содержит невалидный JSON: {e}")
                })?
            };
            if args.insert(param_name.clone(), value).is_some() {
                return Err(format!("повторное имя parameter {param_name:?}"));
            }
            body_rest = &body_rest[param_end + PARAM_CLOSE.len()..];
        }
        // DeepSeek передаёт аргументы либо отдельными parameters, либо одним
        // JSON-объектом `args`. В ответах relay.fast рядом встречается метаданный
        // parameter `tool`; он не является аргументом инструмента. Остальные
        // дополнительные параметры объединяем, но конфликт ключей считаем ошибкой.
        let args = match args.get("args") {
            Some(Value::Object(nested)) => {
                let mut merged = nested.clone();
                for (key, value) in args {
                    if key == "args" || key == "tool" {
                        continue;
                    }
                    if merged.contains_key(&key) {
                        return Err(format!("дублированный аргумент {key:?} рядом с args"));
                    }
                    merged.insert(key, value);
                }
                merged
            }
            Some(_) => return Err("args parameter должен быть JSON-объектом".into()),
            None => args,
        };
        calls.push(ToolCall {
            tool: name,
            args: Value::Object(args),
        });
        rest = &rest[invoke_end + INVOKE_CLOSE.len()..];
    }
    Ok(calls)
}

/// Читает простые quoted-атрибуты в произвольном порядке, например name="...".
fn quoted_attribute(header: &str, attribute: &str) -> std::result::Result<Option<String>, String> {
    let needle = format!("{attribute}=\"");
    let Some(pos) = header.find(&needle) else {
        if header
            .split_whitespace()
            .any(|token| token.starts_with(&format!("{attribute}=")))
        {
            return Err(format!("атрибут {attribute} должен быть в кавычках"));
        }
        return Ok(None);
    };
    let value_start = pos + needle.len();
    let value_end = header[value_start..]
        .find('"')
        .map(|i| value_start + i)
        .ok_or_else(|| format!("атрибут {attribute} не закрыт кавычкой"))?;
    Ok(Some(header[value_start..value_end].to_string()))
}

/// Если строка закодирована как JSON-строка, снимает кавычки и escape-последовательности.
/// Иначе сохраняет содержимое буквально: это важно для shell-команд с обратными слешами.
fn decode_string_value(raw: &str) -> std::result::Result<Value, String> {
    // string=true — raw UTF-8 содержимое. Если значение начинается/заканчивается
    // кавычкой, оно похоже на JSON-строку и должно быть валидным JSON целиком.
    if raw.starts_with('"') || raw.ends_with('"') {
        return match serde_json::from_str::<Value>(raw) {
            Ok(Value::String(s)) => Ok(Value::String(s)),
            Ok(_) => Err("ожидалась JSON-строка".into()),
            Err(e) => Err(e.to_string()),
        };
    }
    Ok(Value::String(raw.to_string()))
}

/// Первый сбалансированный JSON-объект; учитывает кавычки и escape-последовательности.
fn extract_json_object(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let start = bytes.iter().position(|&b| b == b'{')?;
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escaped = false;
    for (i, &byte) in bytes.iter().enumerate().skip(start) {
        if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
            continue;
        }
        match byte {
            b'"' => in_string = true,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(text[start..=i].to_string());
                }
            }
            _ => {}
        }
    }
    None
}

fn brief(text: &str) -> String {
    let shortened: String = text.chars().take(180).collect();
    if text.chars().count() > 180 {
        format!("{shortened}…")
    } else {
        shortened
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dsml(invoke: &str) -> String {
        format!("<｜｜DSML｜｜ calls>\n{invoke}\n</｜｜DSML｜｜ calls>")
    }

    #[test]
    fn parses_simple_dsml_call() {
        let text = dsml(
            "<｜｜DSML｜｜ invoke name=\"run_shell\"><｜｜DSML｜｜ parameter name=\"command\" string=\"true\">cargo test</｜｜DSML｜｜ parameter></｜｜DSML｜｜ invoke>",
        );
        let Step::Tool(call) = parse(&text).unwrap() else {
            panic!("expected one tool call")
        };
        assert_eq!(call.tool, "run_shell");
        assert_eq!(call.args["command"], "cargo test");
    }

    #[test]
    fn parses_two_parameters_including_json_value() {
        let text = dsml(
            "<｜｜DSML｜｜ invoke name=\"write_file\"><｜｜DSML｜｜ parameter name=\"path\" string=\"true\">src/lib.rs</｜｜DSML｜｜ parameter><｜｜DSML｜｜ parameter name=\"metadata\" string=\"false\">{\"mode\": 3}</｜｜DSML｜｜ parameter></｜｜DSML｜｜ invoke>",
        );
        let Step::Tool(call) = parse(&text).unwrap() else {
            panic!("expected one tool call")
        };
        assert_eq!(call.args["path"], "src/lib.rs");
        assert_eq!(call.args["metadata"]["mode"], 3);
    }

    #[test]
    fn parses_json_args_parameter_without_extra_nesting() {
        let text = dsml(
            r#"<｜｜DSML｜｜ invoke name="run_shell"><｜｜DSML｜｜ parameter name="args" string="false">{"command":"cargo test"}</｜｜DSML｜｜ parameter></｜｜DSML｜｜ invoke>"#,
        );
        let Step::Tool(call) = parse(&text).unwrap() else {
            panic!("expected one tool call")
        };
        assert_eq!(call.args["command"], "cargo test");
        assert!(call.shell_command().is_some());
    }

    #[test]
    fn parses_multiple_invokes_in_order() {
        let text = dsml(concat!(
            "<｜｜DSML｜｜ invoke name=\"run_shell\"><｜｜DSML｜｜ parameter name=\"command\" string=\"true\">pwd</｜｜DSML｜｜ parameter></｜｜DSML｜｜ invoke>",
            "<｜｜DSML｜｜ invoke name=\"run_shell\"><｜｜DSML｜｜ parameter name=\"command\" string=\"true\">cargo test</｜｜DSML｜｜ parameter></｜｜DSML｜｜ invoke>"
        ));
        let Step::ToolCalls(calls) = parse(&text).unwrap() else {
            panic!("expected two tool calls")
        };
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].args["command"], "pwd");
        assert_eq!(calls[1].args["command"], "cargo test");
    }

    #[test]
    fn decodes_json_escaped_quotes_in_string_parameter() {
        let text = dsml(
            r#"<｜｜DSML｜｜ invoke name="run_shell"><｜｜DSML｜｜ parameter name="command" string="true">"printf \"hello\""</｜｜DSML｜｜ parameter></｜｜DSML｜｜ invoke>"#,
        );
        let Step::Tool(call) = parse(&text).unwrap() else {
            panic!("expected one tool call")
        };
        assert_eq!(call.args["command"], r#"printf "hello""#);
    }

    #[test]
    fn invalid_dsml_and_json_return_invalid_tool_call() {
        let err = parse("<｜｜DSML｜｜ invoke name=\"run_shell\">no close").unwrap_err();
        assert!(matches!(err, WorkerError::InvalidToolCall(_)));
        let err = parse("just words, no call").unwrap_err();
        assert!(matches!(err, WorkerError::InvalidToolCall(_)));
    }

    #[test]
    fn rejects_malformed_quoted_string_parameter() {
        let text = dsml(
            r#"<｜｜DSML｜｜ invoke name="run_shell"><｜｜DSML｜｜ parameter name="command" string="true">"printf \q"</｜｜DSML｜｜ parameter></｜｜DSML｜｜ invoke>"#,
        );
        assert!(matches!(parse(&text), Err(WorkerError::InvalidToolCall(_))));
    }

    #[test]
    fn rejects_truncated_quoted_string_parameter() {
        let text = dsml(
            r#"<｜｜DSML｜｜ invoke name="run_shell"><｜｜DSML｜｜ parameter name="command" string="true">"printf hello</｜｜DSML｜｜ parameter></｜｜DSML｜｜ invoke>"#,
        );
        assert!(matches!(parse(&text), Err(WorkerError::InvalidToolCall(_))));
    }

    #[test]
    fn json_format_remains_supported() {
        assert_eq!(
            parse(r#"{"done":true,"result":"ok"}"#).unwrap(),
            Step::Done {
                result: "ok".into()
            }
        );
        let Step::Tool(call) =
            parse(r#"```json {"tool":"run_shell","args":{"command":"cargo test"}} ```"#).unwrap()
        else {
            panic!("expected tool")
        };
        assert_eq!(call.args["command"], "cargo test");
    }
}
