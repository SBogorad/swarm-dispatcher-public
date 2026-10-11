//! Парсинг ответа LLM → граф задач, детерминированный root_id, локальная валидация.

use crate::error::{PlannerError, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

/// Namespace из ТЗ (DNS namespace, RFC 4122).
pub const ROOT_NAMESPACE: Uuid = Uuid::from_bytes([
    0x6b, 0xa7, 0xb8, 0x10, 0x9d, 0xad, 0x11, 0xd1, 0x80, 0xb4, 0x00, 0xc0, 0x4f, 0xd4, 0x30, 0xc8,
]);

/// Задача в ответе LLM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannedTask {
    pub id: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub checks: Vec<serde_json::Value>,
    #[serde(default)]
    pub needs: serde_json::Value,
    #[serde(default)]
    pub budget: serde_json::Value,
    #[serde(default)]
    pub depends_on: Vec<String>,
}

/// Ответ LLM целиком.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannedGraph {
    pub tasks: Vec<PlannedTask>,
}

/// Лимиты графа (из env, попадают в промпт и в контракт корня).
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub max_tasks: u32,
    pub max_depth: u32,
    pub max_children: u32,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_tasks: 50,
            max_depth: 5,
            max_children: 10,
        }
    }
}

/// sha256-хекс текста задачи.
pub fn sha256_hex(text: &str) -> String {
    let mut h = Sha256::new();
    h.update(text.as_bytes());
    hex::encode(h.finalize())
}

/// Детерминированный root_id из текста задачи.
///
/// `root_id = UUIDv5(namespace = ROOT_NAMESPACE, name = sha256_hex(text))`.
/// Одинаковый текст → одинаковый root_id → повторный запуск упирается в PK.
pub fn deterministic_root_id(text: &str) -> Uuid {
    Uuid::new_v5(&ROOT_NAMESPACE, sha256_hex(text).as_bytes())
}

/// Разбор ответа LLM. Выделяет первый JSON-объект из возможной обёртки
/// (markdown-код-блок, текст вокруг).
pub fn parse_response(raw: &str) -> Result<PlannedGraph> {
    let candidate = extract_json_object(raw)
        .ok_or_else(|| PlannerError::InvalidJson("no JSON object found".into()))?;
    let graph: PlannedGraph =
        serde_json::from_str(&candidate).map_err(|e| PlannerError::InvalidJson(e.to_string()))?;
    if graph.tasks.is_empty() {
        return Err(PlannerError::InvalidJson("empty tasks".into()));
    }
    Ok(graph)
}

/// Ищет первый сбалансированный JSON-объект `{...}` в строке.
fn extract_json_object(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let start = s.find('{')?;
    let mut depth = 0i32;
    let mut in_str = false;
    let mut esc = false;
    for (i, &b) in bytes.iter().enumerate().skip(start) {
        if in_str {
            if esc {
                esc = false;
            } else if b == b'\\' {
                esc = true;
            } else if b == b'"' {
                in_str = false;
            }
            continue;
        }
        match b {
            b'"' => in_str = true,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(s[start..=i].to_string());
                }
            }
            _ => {}
        }
    }
    None
}

/// Полная локальная валидация графа. Порядок проверок по §7 ТЗ.
pub fn validate_graph(graph: &PlannedGraph, limits: &Limits) -> Result<()> {
    // b. число задач
    if graph.tasks.len() as u32 > limits.max_tasks {
        return Err(PlannerError::Validation(format!(
            "too many tasks: {} > max_tasks {}",
            graph.tasks.len(),
            limits.max_tasks
        )));
    }

    // id уникальны
    let mut ids = std::collections::HashSet::new();
    for t in &graph.tasks {
        if t.id.is_empty() {
            return Err(PlannerError::Validation("task with empty id".into()));
        }
        if !ids.insert(t.id.clone()) {
            return Err(PlannerError::Validation(format!("duplicate id: {}", t.id)));
        }
    }

    // f. depends_on ссылается на существующий id
    for t in &graph.tasks {
        for d in &t.depends_on {
            if !ids.contains(d) {
                return Err(PlannerError::Validation(format!(
                    "task {} depends_on unknown id {}",
                    t.id, d
                )));
            }
        }
    }

    // h. непустой checks у каждой задачи
    for t in &graph.tasks {
        if t.checks.is_empty() {
            return Err(PlannerError::Validation(format!(
                "task {} has empty checks",
                t.id
            )));
        }
    }

    // g. ровно один корень (depends_on пуст)
    let roots: Vec<&str> = graph
        .tasks
        .iter()
        .filter(|t| t.depends_on.is_empty())
        .map(|t| t.id.as_str())
        .collect();
    if roots.len() != 1 {
        return Err(PlannerError::Validation(format!(
            "expected exactly one root, got {}",
            roots.len()
        )));
    }

    // e. нет циклов (и заодно c/d через топосорт)
    let depth = graph_depth(graph)?;

    // c. глубина
    if depth > limits.max_depth {
        return Err(PlannerError::Validation(format!(
            "depth {} > max_depth {}",
            depth, limits.max_depth
        )));
    }

    // d. ширина (число прямых детей на узел)
    let mut children: std::collections::HashMap<&str, u32> = std::collections::HashMap::new();
    for t in &graph.tasks {
        for d in &t.depends_on {
            *children.entry(d.as_str()).or_insert(0) += 1;
        }
    }
    for (node, n) in &children {
        if *n > limits.max_children {
            return Err(PlannerError::Validation(format!(
                "node {} has {} children > max_children {}",
                node, n, limits.max_children
            )));
        }
    }

    Ok(())
}

/// Глубина графа (число уровней). Ошибка при цикле.
/// Узел-лист (никто от него не зависит) = глубина 1.
/// Глубина узла = 1 + max(глубина зависимостей).
fn graph_depth(graph: &PlannedGraph) -> Result<u32> {
    use std::collections::HashMap;
    let deps: HashMap<&str, &Vec<String>> = graph
        .tasks
        .iter()
        .map(|t| (t.id.as_str(), &t.depends_on))
        .collect();

    // memo: id -> глубина
    let mut memo: HashMap<String, u32> = HashMap::new();
    let mut visiting: std::collections::HashSet<String> = std::collections::HashSet::new();

    fn depth_of(
        id: &str,
        deps: &std::collections::HashMap<&str, &Vec<String>>,
        memo: &mut std::collections::HashMap<String, u32>,
        visiting: &mut std::collections::HashSet<String>,
    ) -> Result<u32> {
        if let Some(d) = memo.get(id) {
            return Ok(*d);
        }
        if !visiting.insert(id.to_string()) {
            return Err(PlannerError::Validation(format!(
                "cycle detected at {}",
                id
            )));
        }
        let dlist = deps
            .get(id)
            .ok_or_else(|| PlannerError::Validation(format!("unknown id {}", id)))?;
        let mut best = 0u32;
        for d in dlist.iter() {
            let dd = depth_of(d, deps, memo, visiting)?;
            best = best.max(dd);
        }
        visiting.remove(id);
        let depth = best + 1;
        memo.insert(id.to_string(), depth);
        Ok(depth)
    }

    let mut max_depth = 0u32;
    for t in &graph.tasks {
        let d = depth_of(&t.id, &deps, &mut memo, &mut visiting)?;
        max_depth = max_depth.max(d);
    }
    Ok(max_depth)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn g(tasks: serde_json::Value) -> PlannedGraph {
        serde_json::from_value(serde_json::json!({"tasks": tasks})).unwrap()
    }

    fn ok_task(id: &str, deps: Vec<&str>) -> serde_json::Value {
        serde_json::json!({
            "id": id, "kind": "code.refactor", "description": "d",
            "checks": [{"type":"cmd","run":"cargo test"}],
            "needs": {}, "budget": {"attempts":3,"max_rework":2},
            "depends_on": deps
        })
    }

    #[test]
    fn parse_valid_json() {
        let raw = r#"{"tasks":[{"id":"t1","kind":"k","checks":[{"type":"cmd"}],"depends_on":[]}]}"#;
        let gr = parse_response(raw).unwrap();
        assert_eq!(gr.tasks.len(), 1);
        assert_eq!(gr.tasks[0].id, "t1");
    }

    #[test]
    fn parse_valid_json_from_code_block() {
        let raw = "Вот план:\n```json\n{\"tasks\":[{\"id\":\"t1\",\"checks\":[{\"type\":\"cmd\"}],\"depends_on\":[]}]}\n```\nГотово.";
        let gr = parse_response(raw).unwrap();
        assert_eq!(gr.tasks.len(), 1);
    }

    #[test]
    fn parse_invalid_json() {
        assert!(parse_response("не json совсем").is_err());
        assert!(parse_response("{\"tasks\":[]}").is_err());
    }

    #[test]
    fn validate_no_cycles() {
        let gr = g(serde_json::json!([
            ok_task("t1", vec![]),
            ok_task("t2", vec!["t3"]),
            ok_task("t3", vec!["t2"])
        ]));
        assert!(matches!(
            validate_graph(&gr, &Limits::default()),
            Err(PlannerError::Validation(_))
        ));
    }

    #[test]
    fn validate_depends_exist() {
        let gr = g(serde_json::json!([
            ok_task("t1", vec![]),
            ok_task("t2", vec!["nope"])
        ]));
        assert!(validate_graph(&gr, &Limits::default()).is_err());
    }

    #[test]
    fn validate_limits() {
        let lim = Limits {
            max_tasks: 2,
            max_depth: 5,
            max_children: 10,
        };
        let gr = g(serde_json::json!([
            ok_task("t1", vec![]),
            ok_task("t2", vec!["t1"]),
            ok_task("t3", vec!["t1"])
        ]));
        assert!(validate_graph(&gr, &lim).is_err());
    }

    #[test]
    fn validate_single_root() {
        let gr = g(serde_json::json!([
            ok_task("t1", vec![]),
            ok_task("t2", vec![])
        ]));
        let e = validate_graph(&gr, &Limits::default());
        assert!(matches!(e, Err(PlannerError::Validation(_))));
    }

    #[test]
    fn validate_depth() {
        let lim = Limits {
            max_tasks: 50,
            max_depth: 2,
            max_children: 10,
        };
        let gr = g(serde_json::json!([
            ok_task("t1", vec![]),
            ok_task("t2", vec!["t1"]),
            ok_task("t3", vec!["t2"])
        ]));
        assert!(validate_graph(&gr, &lim).is_err());
    }

    #[test]
    fn deterministic_root_id_stable() {
        let a = deterministic_root_id("Рефакторинг auth");
        let b = deterministic_root_id("Рефакторинг auth");
        assert_eq!(a, b);
    }

    #[test]
    fn deterministic_root_id_differs() {
        let a = deterministic_root_id("Рефакторинг auth");
        let b = deterministic_root_id("Рефакторинг db");
        assert_ne!(a, b);
    }
}
