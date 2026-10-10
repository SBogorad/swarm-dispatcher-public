//! Системный промпт. Лимиты подставляются из env (не хардкод).

use crate::parse::Limits;

/// Системный промпт с подстановкой лимитов из env.
///
/// Лимиты фиксируются на уровне LLM, а не только пост-валидацией.
pub fn system_prompt(limits: &Limits) -> String {
    format!(
        r#"Ты — планировщик задач. Получаешь большую задачу и разбиваешь её на граф
подзадач (контрактов). Ответ должен быть валидным JSON.

Схема:
{{
  "tasks": [
    {{
      "id": "t1",
      "kind": "code.refactor",
      "description": "...",
      "checks": [{{"type": "cmd", "run": "cargo test"}}],
      "needs": {{"min_ram_gb": 8, "coder_class": 2}},
      "budget": {{"attempts": 3, "max_rework": 2}},
      "depends_on": []
    }}
  ]
}}

Правила:
- Не более {max_tasks} задач.
- Глубина ≤ {max_depth}.
- Ширина ≤ {max_children}.
- У каждой задачи ≥ 1 check.
- depends_on ссылается на id из этого же ответа.
- Первая задача (root) имеет depends_on: []."#,
        max_tasks = limits.max_tasks,
        max_depth = limits.max_depth,
        max_children = limits.max_children,
    )
}

/// Пользовательский промпт (текст задачи + опциональный контекст).
pub fn user_prompt(task_text: &str, context: Option<&str>, expected: Option<&str>) -> String {
    let mut s = String::new();
    if let Some(c) = context {
        s.push_str(&format!("Контекст: {c}\n"));
    }
    if let Some(e) = expected {
        s.push_str(&format!("Ожидаемый результат: {e}\n"));
    }
    s.push_str(&format!("Задача: {task_text}"));
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_contains_limits() {
        let lim = Limits {
            max_tasks: 7,
            max_depth: 3,
            max_children: 4,
        };
        let p = system_prompt(&lim);
        assert!(p.contains("Не более 7 задач"));
        assert!(p.contains("Глубина ≤ 3"));
        assert!(p.contains("Ширина ≤ 4"));
    }

    #[test]
    fn user_prompt_optional_parts() {
        assert_eq!(user_prompt("X", None, None), "Задача: X");
        let p = user_prompt("X", Some("Rust"), Some("PR"));
        assert!(p.contains("Контекст: Rust"));
        assert!(p.contains("Ожидаемый результат: PR"));
    }
}
