use serde_json::json;
use swarm_core::MemoryLesson;
use swarm_worker::prompt::{sanitize_untrusted, user_prompt_with_repo};

fn lesson() -> MemoryLesson {
    MemoryLesson {
        kind: "root".to_string(),
        lesson: "проверяй ссылки".to_string(),
        rework_count: 1,
        created_at: "2026-10-18T00:00:00Z".to_string(),
    }
}

#[test]
fn rework_reason_is_the_last_section() {
    let contract = json!({ "kind": "root", "description": "сделай что-то" });
    let review = json!({
        "verdict": "changes_requested",
        "reviewer": "rubric",
        "checks": [
            { "id": "trail", "status": "fail", "detail": "строка X без указателя" }
        ]
    });
    let lessons = [lesson()];
    let prompt = user_prompt_with_repo(&contract, 1, Some(&review), &[], &[], &lessons, &[]);
    let rework = prompt
        .find("Это повторная попытка")
        .expect("секция причины отказа отсутствует");
    let memory = prompt
        .find("Уроки прошлых графов того же вида работ")
        .expect("секция уроков отсутствует");
    assert!(
        rework > memory,
        "rework ({rework}) должен идти после уроков ({memory})"
    );
}

#[test]
fn sanitize_keeps_code_intact() {
    let code = "fn f(x: Vec<T>) -> u8";
    assert_eq!(sanitize_untrusted(code), code);
}

#[test]
fn empty_lessons_leave_no_memory_section() {
    let contract = json!({ "kind": "root", "description": "первый граф" });
    let with_empty = user_prompt_with_repo(&contract, 0, None, &[], &[], &[], &[]);
    assert!(
        !with_empty.contains("Уроки прошлых графов того же вида работ"),
        "секция памяти не должна появляться при пустом списке уроков"
    );
    let baseline = user_prompt_with_repo(&contract, 0, None, &[], &[], &[], &[]);
    assert_eq!(with_empty, baseline);
}
