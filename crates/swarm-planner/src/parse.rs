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
    /// Итерация 2: опциональная машиночитаемая цель подзадачи.
    #[serde(default)]
    pub goal: String,
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

/// F7: жёсткие пределы контрактов от LLM (clamping).
///
/// Модель не должна произвольно раздувать `attempts` / ставить недостижимые
/// `min_ram_gb` / `coder_class`. Верхние границы выбраны с запасом к разумным
/// значениям, но жёстко ограничивают абсурд (`attempts: 1_000_000_000`).
pub mod contract_limits {
    /// Максимальное число попыток на задачу.
    pub const MAX_ATTEMPTS: u32 = 100;
    /// Максимальное число доработок (max_rework).
    pub const MAX_REWORK: u32 = 50;
    /// Максимальный требуемый RAM (GiB) — выше этого значения задачу никто
    /// не возьмёт, значит это либо ошибка, либо DoS-попытка.
    pub const MAX_MIN_RAM_GB: f64 = 1024.0;
    /// Максимальный coder_class (класс модели исполнителя).
    pub const MAX_CODER_CLASS: f64 = 100.0;
    /// Максимальное число проверок в контракте одной задачи.
    pub const MAX_CHECKS_PER_TASK: usize = 32;
}

/// F7: реестр допустимых типов проверок (`checks[].type`).
///
/// Модель не может назначить произвольный/фиктивный тип проверки. Любой тип
/// вне реестра отклоняется на валидации контракта (CT001), а не молча
/// принимается. `cmd` — единственный исполняемый тип в текущей модели угроз;
/// остальные — будущие (тесты/юнит/линт/ручные), но список закрыт.
///
/// N5 (DeepSeek): этот список ОБЯЗАН совпадать со SQL-реестром в
/// `schema/schema_v0.sql` (`validate_contract`). `unit` добавлен в оба
/// одновременно — F7 сузил реестр и сломал обратную совместимость со
/// `step2/test_review_gate.py` (контракт `{"type":"unit"}`).
pub const ALLOWED_CHECK_TYPES: &[&str] = &["cmd", "test", "unit", "lint", "manual"];

/// F7: является ли тип проверки допустимым (регистронезависимо, с trim).
pub fn is_allowed_check_type(t: &str) -> bool {
    let t = t.trim().to_ascii_lowercase();
    ALLOWED_CHECK_TYPES.iter().any(|a| *a == t)
}

/// Итерация 3 (`wayfinder` / `to-tickets`): канонический каталог видов задач.
///
/// Совпадает с каталогом из системного промпта планировщика и с каталогом
/// плейбуков воркера ([`swarm_worker::playbooks::playbook_for_kind`]):
/// вид задачи выбирает протокол исполнения, поэтому реестр закрыт.
pub const CANONICAL_KINDS: &[&str] = &[
    "code.investigate",
    "code.architect",
    "code.bugfix",
    "code.feature",
    "code.refactor",
    "code.perf",
    "code.verify",
    "root",
];

/// Legacy-набор `kind`, который обязан продолжать приниматься.
///
/// F7 и фикстуры (`planner_ok.json`, `002_ok.json`) исторически используют
/// короткие формы; отвергать их — сломать обратную совместимость.
pub const LEGACY_KINDS: &[&str] = &[
    "code",
    "test",
    "doc",
    "analysis",
    "audit",
    "design",
    "bugfix",
    "feature",
    "refactor",
    "investigate",
    "perf",
    "task",
];

/// Итерация 3: допустим ли `kind` — канонический или legacy (регистр/trim).
pub fn is_canonical_or_legacy_kind(kind: &str) -> bool {
    let k = kind.trim().to_ascii_lowercase();
    CANONICAL_KINDS.contains(&k.as_str()) || LEGACY_KINDS.contains(&k.as_str())
}

/// Маркеры незавершённой работы, недопустимые в команде проверки.
///
/// `checks[].run` — исполняемая команда; заглушка в ней означает, что задача
/// спланирована как «сделаю потом» и не может быть проверена (по мотивам
/// `check-plan.mjs` из `pstack`).
pub const CHECK_STUB_MARKERS: &[&str] = &["TODO", "FIXME", "<INSERT_", "unimplemented!", "todo!"];

/// Итерация 3: найти маркер-заглушку в команде проверки.
pub fn check_stub_marker(run: &str) -> Option<&'static str> {
    CHECK_STUB_MARKERS
        .iter()
        .find(|m| run.contains(**m))
        .copied()
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

/// Итерация 4 (F17): `root_id` с учётом ОБЛАСТИ планирования.
///
/// При пустых (или чисто пробельных) `context`/`expected` результат
/// ПОБАЙТОВО совпадает с [`deterministic_root_id`] — 100% обратная
/// совместимость со старыми графами и тестами.
///
/// При непустом scope `root_id` отличается: один и тот же текст задачи в
/// разном контексте — разные графы. Роли `context` и `expected` не
/// смешиваются (раздельные префиксы `ctx\0` / `exp\0`), а NUL-разделитель
/// исключает инъекцию: `context="a\0expected=b"` не совпадёт с парой
/// `context="a"`, `expected="b"`.
pub fn deterministic_root_id_with_scope(
    text: &str,
    context: Option<&str>,
    expected: Option<&str>,
) -> Uuid {
    let c = context.map(str::trim).filter(|s| !s.is_empty());
    let e = expected.map(str::trim).filter(|s| !s.is_empty());
    if c.is_none() && e.is_none() {
        return deterministic_root_id(text);
    }
    let mut material = String::new();
    material.push_str("ctx\0");
    material.push_str(c.unwrap_or(""));
    material.push_str("\0exp\0");
    material.push_str(e.unwrap_or(""));
    material.push('\0');
    material.push_str(text);
    Uuid::new_v5(&ROOT_NAMESPACE, sha256_hex(&material).as_bytes())
}

/// Итерация 4 (вопрос 5 аудита): нормализовать мульти-корневой граф к единому
/// корню ВМЕСТО отказа и отката на примитивный `rule-based` путь.
///
/// Дефект: `validate_graph` отвергал план с более чем одной задачей без
/// зависимостей, хотя `insert_graph` всё равно перезаписывает корень
/// синтетическим `root_contract`. На flash-моделях это давало ~40% отказов.
///
/// Нормализация: первый безкорневой узел остаётся корнем, все остальные
/// безкорневые получают его в `depends_on`. Задач не добавляется и не
/// удаляется — число корней становится ровно 1.
///
/// Возвращает `true`, если граф был изменён.
pub fn normalize_multi_root(graph: &mut PlannedGraph) -> bool {
    let roots: Vec<usize> = graph
        .tasks
        .iter()
        .enumerate()
        .filter(|(_, t)| t.depends_on.is_empty())
        .map(|(i, _)| i)
        .collect();
    if roots.len() <= 1 {
        return false;
    }
    let root_id = graph.tasks[roots[0]].id.clone();
    for &i in &roots[1..] {
        graph.tasks[i].depends_on.push(root_id.clone());
    }
    true
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

    // h. непустой checks у каждой задачи + F7: реестр допустимых типов и предел числа проверок
    for t in &graph.tasks {
        if t.checks.is_empty() {
            return Err(PlannerError::Validation(format!(
                "task {} has empty checks",
                t.id
            )));
        }
        if t.checks.len() > contract_limits::MAX_CHECKS_PER_TASK {
            return Err(PlannerError::Validation(format!(
                "task {} has {} checks > max {}",
                t.id,
                t.checks.len(),
                contract_limits::MAX_CHECKS_PER_TASK
            )));
        }
        // F7: каждый элемент checks — объект с допустимым типом из реестра.
        for c in &t.checks {
            let ty = c.get("type").and_then(|v| v.as_str()).ok_or_else(|| {
                PlannerError::Validation(format!("task {} has check without string type", t.id))
            })?;
            if !is_allowed_check_type(ty) {
                return Err(PlannerError::Validation(format!(
                    "task {} has disallowed check type {:?} (allowed: {:?})",
                    t.id, ty, ALLOWED_CHECK_TYPES
                )));
            }

            // Итерация 3 (`check-plan`): команда проверки не должна быть
            // заглушкой — иначе задачу невозможно верифицировать, и она
            // уедет в `blocked`, сжигая попытки.
            if let Some(run) = c.get("run").and_then(|v| v.as_str()) {
                if let Some(marker) = check_stub_marker(run) {
                    return Err(PlannerError::Validation(format!(
                        "task {} check run contains stub marker {:?}: {:?}",
                        t.id, marker, run
                    )));
                }
            }
        }

        // Итерация 3: реестр видов задач (канонический каталог + legacy).
        // Пустой `kind` допустим и трактуется как `task` в `subtask_contract`.
        if !t.kind.trim().is_empty() && !is_canonical_or_legacy_kind(&t.kind) {
            return Err(PlannerError::Validation(format!(
                "task {} has unknown kind {:?} (canonical: {:?}; legacy allowed)",
                t.id, t.kind, CANONICAL_KINDS
            )));
        }
    }

    // F7: диапазоны budget/needs — модель не должна раздувать attempts до
    // миллиарда или ставить недостижимый min_ram_gb.
    for t in &graph.tasks {
        if let Some(a) = t.budget.get("attempts").and_then(|v| v.as_u64()) {
            if a == 0 || a > contract_limits::MAX_ATTEMPTS as u64 {
                return Err(PlannerError::Validation(format!(
                    "task {} budget.attempts {} out of range 1..={}",
                    t.id,
                    a,
                    contract_limits::MAX_ATTEMPTS
                )));
            }
        }
        if let Some(r) = t.budget.get("max_rework").and_then(|v| v.as_u64()) {
            if r > contract_limits::MAX_REWORK as u64 {
                return Err(PlannerError::Validation(format!(
                    "task {} budget.max_rework {} > {}",
                    t.id,
                    r,
                    contract_limits::MAX_REWORK
                )));
            }
        }
        if let Some(ram) = t.needs.get("min_ram_gb").and_then(|v| v.as_f64()) {
            if !ram.is_finite() || ram < 0.0 || ram > contract_limits::MAX_MIN_RAM_GB {
                return Err(PlannerError::Validation(format!(
                    "task {} needs.min_ram_gb {} out of range 0..={}",
                    t.id,
                    ram,
                    contract_limits::MAX_MIN_RAM_GB
                )));
            }
        }
        if let Some(cc) = t.needs.get("coder_class").and_then(|v| v.as_f64()) {
            if !cc.is_finite() || cc < 0.0 || cc > contract_limits::MAX_CODER_CLASS {
                return Err(PlannerError::Validation(format!(
                    "task {} needs.coder_class {} out of range 0..={}",
                    t.id,
                    cc,
                    contract_limits::MAX_CODER_CLASS
                )));
            }
        }
    }

    // g. ровно один корень (depends_on пуст).
    //
    // Инвариант ПОСЛЕ нормализации: сырой ответ LLM с несколькими корнями
    // чинит `normalize_multi_root` до вызова этой функции (см. Итерацию 4,
    // вопрос 5 аудита). Здесь остаётся жёсткая проверка как страховка —
    // вставляемый в БД граф обязан иметь ровно один корень.
    let roots: Vec<&str> = graph
        .tasks
        .iter()
        .filter(|t| t.depends_on.is_empty())
        .map(|t| t.id.as_str())
        .collect();
    if roots.len() != 1 {
        return Err(PlannerError::Validation(format!(
            "expected exactly one root, got {} (normalize_multi_root должен был свести к одному)",
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

    // ---------- Итерация 4: root_id с областью планирования (F17) ----------

    #[test]
    fn scoped_root_id_without_scope_equals_plain() {
        // 100% обратная совместимость: пустой/пробельный scope не меняет id.
        let text = "Рефакторинг auth";
        let plain = deterministic_root_id(text);
        assert_eq!(deterministic_root_id_with_scope(text, None, None), plain);
        assert_eq!(
            deterministic_root_id_with_scope(text, Some("  "), Some("")),
            plain
        );
    }

    #[test]
    fn scoped_root_id_differs_on_context() {
        let text = "Рефакторинг auth";
        let a = deterministic_root_id_with_scope(text, Some("ctxA"), None);
        let b = deterministic_root_id_with_scope(text, Some("ctxB"), None);
        assert_ne!(a, b);
        assert_ne!(a, deterministic_root_id(text));
    }

    #[test]
    fn scoped_root_id_context_and_expected_do_not_collide() {
        // Роли context и expected не смешиваются: один и тот же текст в
        // разных полях даёт разные id.
        let text = "Рефакторинг auth";
        let as_context = deterministic_root_id_with_scope(text, Some("a"), None);
        let as_expected = deterministic_root_id_with_scope(text, None, Some("a"));
        assert_ne!(as_context, as_expected);
    }

    #[test]
    fn scoped_root_id_immune_to_separator_injection() {
        // NUL-разделитель: context="a\0expected=b" не совпадает с
        // парой context="a", expected="b".
        let text = "Рефакторинг auth";
        let injected = deterministic_root_id_with_scope(text, Some("a\0expected=b"), None);
        let pair = deterministic_root_id_with_scope(text, Some("a"), Some("b"));
        assert_ne!(injected, pair);
    }

    // ---------- Итерация 4: нормализация мульти-корня (вопрос 5) ----------

    #[test]
    fn normalize_multi_root_welds_to_single_root() {
        let mut gr = g(serde_json::json!([
            ok_task("t1", vec![]),
            ok_task("t2", vec![]),
            ok_task("t3", vec!["t1"])
        ]));
        assert!(normalize_multi_root(&mut gr));
        // Ровно один корень, и это первый безкорневой узел.
        let roots: Vec<&str> = gr
            .tasks
            .iter()
            .filter(|t| t.depends_on.is_empty())
            .map(|t| t.id.as_str())
            .collect();
        assert_eq!(roots, vec!["t1"]);
        // t2 подвешен под t1, t3 не тронут.
        let t2 = gr.tasks.iter().find(|t| t.id == "t2").unwrap();
        assert_eq!(t2.depends_on, vec!["t1".to_string()]);
        let t3 = gr.tasks.iter().find(|t| t.id == "t3").unwrap();
        assert_eq!(t3.depends_on, vec!["t1".to_string()]);
        // После нормализации граф валиден.
        assert!(validate_graph(&gr, &Limits::default()).is_ok());
    }

    #[test]
    fn normalize_multi_root_noop_on_single_root() {
        let mut gr = g(serde_json::json!([
            ok_task("t1", vec![]),
            ok_task("t2", vec!["t1"])
        ]));
        assert!(!normalize_multi_root(&mut gr));
        assert!(validate_graph(&gr, &Limits::default()).is_ok());
    }

    #[test]
    fn normalize_multi_root_three_roots_all_welded() {
        let mut gr = g(serde_json::json!([
            ok_task("a", vec![]),
            ok_task("b", vec![]),
            ok_task("c", vec![])
        ]));
        assert!(normalize_multi_root(&mut gr));
        let roots = gr.tasks.iter().filter(|t| t.depends_on.is_empty()).count();
        assert_eq!(roots, 1, "остался ровно один корень");
        assert!(validate_graph(&gr, &Limits::default()).is_ok());
    }

    // ---------- F7: контроль контрактов планировщика ----------

    fn task_with(
        id: &str,
        checks: serde_json::Value,
        needs: serde_json::Value,
        budget: serde_json::Value,
    ) -> serde_json::Value {
        serde_json::json!({
            "id": id, "kind": "code.refactor", "description": "d",
            "checks": checks, "needs": needs, "budget": budget, "depends_on": []
        })
    }

    #[test]
    fn f7_rejects_disallowed_check_type() {
        let gr = g(serde_json::json!([task_with(
            "t1",
            serde_json::json!([{"type":"exec","run":"rm -rf /"}]),
            serde_json::json!({}),
            serde_json::json!({"attempts":3})
        )]));
        let e = validate_graph(&gr, &Limits::default());
        assert!(
            matches!(e, Err(PlannerError::Validation(_))),
            "фиктивный тип должен быть отклонён: {e:?}"
        );
    }

    #[test]
    fn f7_rejects_check_without_type() {
        let gr = g(serde_json::json!([task_with(
            "t1",
            serde_json::json!([{"run":"cargo test"}]),
            serde_json::json!({}),
            serde_json::json!({"attempts":3})
        )]));
        assert!(validate_graph(&gr, &Limits::default()).is_err());
    }

    #[test]
    fn f7_rejects_absurd_attempts() {
        let gr = g(serde_json::json!([task_with(
            "t1",
            serde_json::json!([{"type":"cmd","run":"true"}]),
            serde_json::json!({}),
            serde_json::json!({"attempts": 1_000_000_000u64})
        )]));
        let e = validate_graph(&gr, &Limits::default());
        assert!(
            matches!(e, Err(PlannerError::Validation(_))),
            "attempts 1e9 должен быть отклонён: {e:?}"
        );
    }

    #[test]
    fn f7_rejects_unreachable_min_ram() {
        let gr = g(serde_json::json!([task_with(
            "t1",
            serde_json::json!([{"type":"cmd","run":"true"}]),
            serde_json::json!({"min_ram_gb": 1_000_000}),
            serde_json::json!({"attempts":3})
        )]));
        let e = validate_graph(&gr, &Limits::default());
        assert!(
            matches!(e, Err(PlannerError::Validation(_))),
            "min_ram_gb 1e6 должен быть отклонён: {e:?}"
        );
    }

    #[test]
    fn f7_accepts_reasonable_contract() {
        let gr = g(serde_json::json!([task_with(
            "t1",
            serde_json::json!([{"type":"cmd","run":"cargo test"}]),
            serde_json::json!({"min_ram_gb": 8.0, "coder_class": 2.0}),
            serde_json::json!({"attempts":3,"max_rework":2})
        )]));
        assert!(validate_graph(&gr, &Limits::default()).is_ok());
    }

    #[test]
    fn f7_rejects_too_many_checks() {
        let checks: Vec<serde_json::Value> = (0..40)
            .map(|_| serde_json::json!({"type":"cmd","run":"true"}))
            .collect();
        let gr = g(serde_json::json!([task_with(
            "t1",
            serde_json::Value::Array(checks),
            serde_json::json!({}),
            serde_json::json!({"attempts":3})
        )]));
        assert!(validate_graph(&gr, &Limits::default()).is_err());
    }

    // Итерация 3 (`check-plan`): заглушка в команде проверки — невалидный план.
    #[test]
    fn iteration3_rejects_stub_marker_in_check_run() {
        for run in [
            "cargo test # TODO",
            "echo FIXME",
            "echo <INSERT_CMD>",
            "cargo run -- unimplemented!",
        ] {
            let gr = g(serde_json::json!([task_with(
                "t1",
                serde_json::json!([{"type":"cmd","run": run}]),
                serde_json::json!({}),
                serde_json::json!({"attempts":3})
            )]));
            assert!(
                validate_graph(&gr, &Limits::default()).is_err(),
                "заглушка {run:?} прошла валидацию"
            );
        }
    }

    #[test]
    fn iteration3_accepts_clean_checks() {
        let gr = g(serde_json::json!([task_with(
            "t1",
            serde_json::json!([{"type":"cmd","run":"cargo test --locked"}]),
            serde_json::json!({}),
            serde_json::json!({"attempts":3})
        )]));
        assert!(validate_graph(&gr, &Limits::default()).is_ok());
    }

    // Итерация 3: реестр видов задач (канон + legacy), старые фикстуры целы.
    #[test]
    fn iteration3_kind_registry() {
        for k in CANONICAL_KINDS.iter().chain(LEGACY_KINDS.iter()) {
            let gr = g(serde_json::json!([task_with(
                "t1",
                serde_json::json!([{"type":"cmd","run":"true"}]),
                serde_json::json!({}),
                serde_json::json!({"attempts":3})
            )
            .as_object_mut()
            .map(|o| {
                o.insert("kind".into(), serde_json::json!(k));
                o.clone()
            })
            .unwrap()]));
            assert!(
                validate_graph(&gr, &Limits::default()).is_ok(),
                "kind {k} отвергнут"
            );
        }
        // Неизвестный kind — отклоняем.
        let bad = g(serde_json::json!([task_with(
            "t1",
            serde_json::json!([{"type":"cmd","run":"true"}]),
            serde_json::json!({}),
            serde_json::json!({"attempts":3})
        )
        .as_object_mut()
        .map(|o| {
            o.insert("kind".into(), serde_json::json!("orm.query"));
            o.clone()
        })
        .unwrap()]));
        assert!(validate_graph(&bad, &Limits::default()).is_err());
    }
}
