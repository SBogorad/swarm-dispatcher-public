//! Planner: LLM → граф задач в Postgres.
//!
//! Поток: текст задачи → детерминированный root_id → промпт → LLM →
//! парсинг → локальная валидация → одна транзакция (`with_transaction`) →
//! граф в БД. Валидация строго до открытия транзакции.

pub mod error;
pub mod llm;
pub mod parse;
pub mod prompt;

pub use error::{PlannerError, Result};
pub use llm::{LlmClient, LlmConfig};
pub use parse::{
    deterministic_root_id, parse_response, validate_graph, Limits, PlannedGraph, PlannedTask,
};
pub use prompt::{system_prompt, user_prompt};

use serde_json::json;
use swarm_core::{Budget, Needs, SwarmDispatcher, TaskContract};
use uuid::Uuid;

/// Итог работы Planner.
#[derive(Debug, Clone)]
pub struct PlanOutcome {
    pub root_id: Uuid,
    /// Сколько задач создано **в этом запуске**.
    pub tasks_created: usize,
    pub ready_to_execute: i32,
    /// true, если граф с таким root_id уже существовал (идемпотентный пропуск).
    pub already_existed: bool,
    /// Число задач в уже существующем графе (информационно).
    pub existing_tasks: usize,
}

/// Конфигурация Planner (лимиты из env).
#[derive(Debug, Clone, Copy)]
pub struct PlannerConfig {
    pub limits: Limits,
}

impl PlannerConfig {
    pub fn from_env() -> Self {
        Self {
            limits: Limits {
                max_tasks: env_u32("PLANNER_MAX_TASKS", 50),
                max_depth: env_u32("PLANNER_MAX_DEPTH", 5),
                max_children: env_u32("PLANNER_MAX_CHILDREN", 10),
            },
        }
    }
}

fn env_u32(name: &str, default: u32) -> u32 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

/// LLM-ответ → контракт корня с явными лимитами (§4 ТЗ).
/// `description` сохраняется в событиях (см. §3 ТЗ); здесь используется как
/// метка контракта — сам текст уже передан в `add_task` через события схемы.
pub fn root_contract(description: &str, limits: &Limits) -> TaskContract {
    TaskContract {
        kind: "root".into(),
        // §2.1: текст задачи доезжает до воркера.
        description: opt_description(description),
        // F-NEW-A: машиночитаемая цель. Без неё контракт `kind = "root"`
        // выглядел для модели пустой обёрткой, и корневая задача уходила в
        // `missing_input` (описание лежало только в <untrusted>).
        goal: opt_description(description),
        checks: vec![json!({"type": "cmd", "run": "true"})],
        needs: Needs::default(),
        budget: Budget {
            attempts: 3,
            max_rework: Some(2),
            max_depth: Some(limits.max_depth),
            max_children: Some(limits.max_children),
            max_tasks: Some(limits.max_tasks),
        },
    }
}

/// Контракт подзадачи (из запланированной задачи LLM).
pub fn subtask_contract(t: &PlannedTask) -> TaskContract {
    let mut budget: Budget = serde_json::from_value(t.budget.clone()).unwrap_or_default();
    if budget.attempts == 0 {
        budget.attempts = 3;
    }
    let needs: Needs = serde_json::from_value(t.needs.clone()).unwrap_or_default();
    TaskContract {
        kind: if t.kind.is_empty() {
            "task".into()
        } else {
            t.kind.clone()
        },
        // §2.2: description подзадачи из LLM-ответа.
        description: opt_description(&t.description),
        // F-NEW-A: подзадача без goal — поле остаётся None (skip_serializing).
        goal: None,
        checks: t.checks.clone(),
        needs,
        budget,
    }
}

/// `Some(text)` для непустого текста, иначе `None` (секция `<untrusted>`
/// в промпте воркера тогда не добавляется — §3).
fn opt_description(text: &str) -> Option<String> {
    if text.trim().is_empty() {
        None
    } else {
        Some(text.to_string())
    }
}

/// Planner: оркеструет LLM → валидация → БД.
pub struct Planner {
    cfg: PlannerConfig,
    llm: LlmClient,
    context: Option<String>,
    expected: Option<String>,
}

impl Planner {
    pub fn new(cfg: PlannerConfig, llm: LlmClient) -> Self {
        Self {
            cfg,
            llm,
            context: None,
            expected: None,
        }
    }

    pub fn with_context(mut self, ctx: Option<String>, expected: Option<String>) -> Self {
        self.context = ctx;
        self.expected = expected;
        self
    }

    /// Полный цикл для текста задачи. Идемпотентен по root_id.
    pub fn plan(&self, dispatcher: &mut SwarmDispatcher, task_text: &str) -> Result<PlanOutcome> {
        let root_id = deterministic_root_id(task_text);

        // §7 шаг 3: если граф уже есть — пропуск.
        //
        // `task_status` доступен не всем ролям (по step 2 грант есть только
        // у `swarm_operator`). Поэтому проверка — best-effort: если читать
        // не разрешено, продолжаем и полагаемся на unique_violation при
        // вставке корня (см. ниже).
        match graph_exists(dispatcher, root_id) {
            Ok(Some(n)) => {
                return Ok(PlanOutcome {
                    root_id,
                    tasks_created: 0,
                    ready_to_execute: 0,
                    already_existed: true,
                    existing_tasks: n,
                });
            }
            Ok(None) => {}
            Err(PlannerError::Database(e)) if e.is_insufficient_privilege() => {
                // Нет права читать — существование определим при вставке.
            }
            Err(e) => return Err(e),
        }

        // Валидация строго ДО транзакции.
        let graph = self.obtain_validated_graph(task_text)?;

        // Одна транзакция: весь граф или ничего.
        let (n, ready, existed) = self.insert_graph(dispatcher, root_id, task_text, &graph)?;
        if existed {
            return Ok(PlanOutcome {
                root_id,
                tasks_created: 0,
                ready_to_execute: 0,
                already_existed: true,
                existing_tasks: 0,
            });
        }

        Ok(PlanOutcome {
            root_id,
            tasks_created: n,
            ready_to_execute: ready,
            already_existed: false,
            existing_tasks: 0,
        })
    }

    /// Запрос к LLM с ретраями при невалидном JSON/валидации.
    fn obtain_validated_graph(&self, task_text: &str) -> Result<PlannedGraph> {
        let sys = system_prompt(&self.cfg.limits);
        let mut user = user_prompt(task_text, self.context.as_deref(), self.expected.as_deref());
        let mut last_err: Option<PlannerError> = None;

        for attempt in 0..=self.cfg.limits.max_tasks.min(self.max_retries()) {
            let _ = attempt;
            match self.llm.complete(&sys, &user) {
                Ok(raw) => match parse_response(&raw).and_then(|g| {
                    validate_graph(&g, &self.cfg.limits)?;
                    Ok(g)
                }) {
                    Ok(g) => return Ok(g),
                    Err(e) => {
                        last_err = Some(e);
                        // retry с указанием на ошибку в промпте (§7 шаг 6)
                        user = format!(
                            "{user}\n\nПредыдущий ответ был отклонён: {err}. Верни исправленный JSON.",
                            err = last_err.as_ref().unwrap()
                        );
                    }
                },
                Err(e) => {
                    last_err = Some(e);
                }
            }
        }
        Err(last_err.unwrap_or_else(|| PlannerError::Validation("no attempts".into())))
    }

    fn max_retries(&self) -> u32 {
        // max_retries из LlmConfig; ограничение сверху, чтобы не зациклиться.
        self.llm.config().max_retries
    }

    /// Вставка графа в одной транзакции.
    ///
    /// Возвращает `(число задач, ready_count, already_existed)`.
    /// `already_existed = true` — корень с таким `root_id` уже был (PK violation
    /// строго на вставке корня); это идемпотентный пропуск, не ошибка.
    fn insert_graph(
        &self,
        dispatcher: &mut SwarmDispatcher,
        root_id: Uuid,
        task_text: &str,
        graph: &PlannedGraph,
    ) -> Result<(usize, i32, bool)> {
        // Определяем корень (depends_on пуст) — он один (валидировано).
        let root = graph
            .tasks
            .iter()
            .find(|t| t.depends_on.is_empty())
            .ok_or_else(|| PlannerError::Validation("no root".into()))?;
        let root_contract = root_contract(task_text, &self.cfg.limits);

        // id LLM ("t1") → UUID. Корень получает сам root_id (по ТЗ §3),
        // подзадачи — v5 от root_id, чтобы были детерминированы в рамках графа.
        let mut id_map: std::collections::HashMap<&str, Uuid> = std::collections::HashMap::new();
        for t in &graph.tasks {
            if t.id == root.id {
                id_map.insert(t.id.as_str(), root_id);
            } else {
                id_map.insert(t.id.as_str(), Uuid::new_v5(&root_id, t.id.as_bytes()));
            }
        }
        let root_uuid = root_id;

        let out = dispatcher.with_transaction(|tx| {
            // Корень. Если он уже есть — unique_violation; это не ошибка,
            // а идемпотентный пропуск. Перехват — строго здесь, чтобы не
            // спутать с нарушением PK на других вставках (например, дубль dep).
            match tx.add_task(root_uuid, &root_contract, Some(root_uuid), 100_000, 0) {
                Ok(()) => {}
                Err(de) if de.is_unique_violation() => {
                    return Ok((0usize, 0i32, true));
                }
                Err(e) => return Err(e),
            }
            // подзадачи
            for t in &graph.tasks {
                if t.id == root.id {
                    continue;
                }
                let c = subtask_contract(t);
                tx.add_task(id_map[t.id.as_str()], &c, Some(root_uuid), 100_000, 0)?;
            }
            // зависимости
            for t in &graph.tasks {
                for d in &t.depends_on {
                    tx.add_dep(id_map[t.id.as_str()], id_map[d.as_str()])?;
                }
            }
            let ready = tx.promote_ready()?;
            Ok((graph.tasks.len(), ready, false))
        })?;

        Ok(out)
    }
}

/// Проверка существования графа. Возвращает Some(число задач) если root есть.
fn graph_exists(dispatcher: &mut SwarmDispatcher, root_id: Uuid) -> Result<Option<usize>> {
    // Прямой SELECT к tasks доступен роли swarm_planner? Нет — используем task_status.
    // Ищем в task_status по root_id.
    let rows = dispatcher.list_task_status(1000)?;
    if rows.iter().any(|r| r.id == root_id) {
        let n = rows.iter().filter(|r| r.root_id == root_id).count();
        return Ok(Some(n));
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_contract_has_limits() {
        let lim = Limits {
            max_tasks: 7,
            max_depth: 3,
            max_children: 4,
        };
        let c = root_contract("do it", &lim);
        assert_eq!(c.kind, "root");
        assert_eq!(c.budget.max_tasks, Some(7));
        assert_eq!(c.budget.max_depth, Some(3));
        assert_eq!(c.budget.max_children, Some(4));
        assert!(!c.checks.is_empty());
    }

    // §6.1 (v0.3.1): корень получает description = текст задачи.
    #[test]
    fn planner_root_contract_has_description() {
        let lim = Limits {
            max_tasks: 50,
            max_depth: 5,
            max_children: 10,
        };
        let c = root_contract("Функция на Rust: сложение двух чисел + тест", &lim);
        assert_eq!(
            c.description.as_deref(),
            Some("Функция на Rust: сложение двух чисел + тест")
        );
        // пустой текст → None (секция <untrusted> не появится)
        assert_eq!(root_contract("   ", &lim).description, None);
    }

    // §6.1 (v0.3.1): подзадача получает description из LLM-ответа.
    #[test]
    fn planner_subtask_contract_has_description() {
        let t = PlannedTask {
            id: "t2".into(),
            kind: "code.refactor".into(),
            description: "Выделить валидацию токенов".into(),
            checks: vec![json!({"type": "cmd"})],
            needs: json!({}),
            budget: json!({}),
            depends_on: vec!["t1".into()],
        };
        let c = subtask_contract(&t);
        assert_eq!(c.description.as_deref(), Some("Выделить валидацию токенов"));
    }

    #[test]
    fn subtask_contract_defaults() {
        let t = PlannedTask {
            id: "t1".into(),
            kind: "".into(),
            description: "d".into(),
            checks: vec![json!({"type": "cmd"})],
            needs: json!({}),
            budget: json!({}),
            depends_on: vec![],
        };
        let c = subtask_contract(&t);
        assert_eq!(c.kind, "task");
        assert_eq!(c.budget.attempts, 3);
    }
}
