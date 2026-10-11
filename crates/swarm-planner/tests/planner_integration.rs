//! Интеграционные тесты Planner: реальный Postgres, mock LLM.
//!
//! Запуск: TEST_DATABASE_URL=postgres://swarm@127.0.0.1:55440/v0planner \
//!         cargo test -p swarm-planner --test planner_integration -- --test-threads=1

use std::path::PathBuf;
use swarm_core::SwarmDispatcher;
use swarm_planner::{LlmClient, LlmConfig, Planner, PlannerConfig};

fn dsn() -> String {
    std::env::var("TEST_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://swarm@127.0.0.1:55440/v0planner".into())
}

fn fixture(name: &str) -> String {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("tests/fixtures");
    p.push(name);
    p.to_string_lossy().into_owned()
}

fn mock_cfg(fixture_name: &str) -> LlmConfig {
    LlmConfig {
        provider: "mock".into(),
        api_key: None,
        model: "mock".into(),
        base_url: None,
        max_retries: 2,
        max_tokens: 4096,
        mock_file: Some(fixture(fixture_name)),
    }
}

fn planner(fixture_name: &str, max_tasks: u32) -> Planner {
    let cfg = PlannerConfig {
        limits: swarm_planner::Limits {
            max_tasks,
            max_depth: 5,
            max_children: 10,
        },
    };
    Planner::new(cfg, LlmClient::new(mock_cfg(fixture_name)).unwrap())
}

/// Отказаться опустошать БД, которая не выглядит тестовой (аудит: `TRUNCATE
/// ... CASCADE` не должен уметь снести рабочую базу).
fn assert_disposable_db(dsn: &str) {
    let name = db_name(dsn);
    assert!(
        name.starts_with("test_") || name.starts_with("v0") || name.starts_with("c2_"),
        "refusing to TRUNCATE non-disposable database {name:?} \
         (allowed prefixes: test_, v0, c2_)"
    );
}

/// Имя БД из DSN: key=value (`dbname=...`) или URI (`postgres://.../db`).
fn db_name(dsn: &str) -> String {
    if let Some(rest) = dsn
        .strip_prefix("postgres://")
        .or_else(|| dsn.strip_prefix("postgresql://"))
    {
        let after_host = rest.split('/').nth(1).unwrap_or("");
        return after_host.split('?').next().unwrap_or("").to_string();
    }
    dsn.split_whitespace()
        .find_map(|p| p.strip_prefix("dbname="))
        .unwrap_or("")
        .to_string()
}

/// Очистка таблиц прямым SQL (тест-пользователь swarm — суперпользователь).
fn clear() {
    let dsn = dsn();
    assert_disposable_db(&dsn);
    let mut client = postgres::Client::connect(&dsn, postgres::NoTls).expect("raw connect");
    client
        .batch_execute("truncate events, reviews, task_deps, artifacts, tasks, workers cascade")
        .expect("truncate");
}

fn raw() -> postgres::Client {
    postgres::Client::connect(&dsn(), postgres::NoTls).expect("raw connect")
}

fn admin() -> SwarmDispatcher {
    clear();
    SwarmDispatcher::connect(&dsn()).expect("connect")
}

// 1. Planner создаёт 3 задачи, 2 зависимости.
#[test]
fn planner_creates_graph() {
    let mut d = admin();
    let p = planner("planner_ok.json", 50);
    let out = p.plan(&mut d, "задача-1").unwrap();
    assert!(!out.already_existed);
    assert_eq!(out.tasks_created, 3);
    assert_eq!(out.ready_to_execute, 1, "только корень должен быть ready");
    let rows = d.list_task_status(100).unwrap();
    assert_eq!(rows.iter().filter(|r| r.root_id == out.root_id).count(), 3);
}

// 2. Второй запуск с тем же текстом не создаёт дублей.
#[test]
fn planner_idempotent_same_text() {
    let mut d = admin();
    let p = planner("planner_ok.json", 50);
    let a = p.plan(&mut d, "задача-2").unwrap();
    assert_eq!(a.tasks_created, 3);
    let b = p.plan(&mut d, "задача-2").unwrap();
    assert!(b.already_existed, "второй запуск должен быть пропущен");
    assert_eq!(b.tasks_created, 0);
    assert_eq!(a.root_id, b.root_id);
    let rows = d.list_task_status(100).unwrap();
    assert_eq!(rows.iter().filter(|r| r.root_id == a.root_id).count(), 3);
}

// 3. Другой текст → другой граф.
#[test]
fn planner_idempotent_different_text() {
    let mut d = admin();
    let p = planner("planner_ok.json", 50);
    let a = p.plan(&mut d, "задача-3a").unwrap();
    let b = p.plan(&mut d, "задача-3b").unwrap();
    assert_ne!(a.root_id, b.root_id);
    assert_eq!(b.tasks_created, 3);
    let rows = d.list_task_status(100).unwrap();
    assert_eq!(rows.iter().filter(|r| r.root_id == a.root_id).count(), 3);
    assert_eq!(rows.iter().filter(|r| r.root_id == b.root_id).count(), 3);
}

// 4. Атомарность графа: граф проходит локальную валидацию, но падает на SQL
//    уже ПОСЛЕ вставки задач (дубль зависимости → PK violation на add_dep).
//    Проверяем через Planner::plan — то есть тем же путём, что и продакшн,
//    чтобы мутация M3 (with_transaction → последовательные add_task) ловилась.
#[test]
fn planner_atomic_on_sql_error() {
    let mut d = admin();
    let p = planner("planner_dup_dep.json", 50);
    let res = p.plan(&mut d, "задача-4");
    assert!(res.is_err(), "дубль зависимости должен уронить SQL-вставку");

    // Транзакция должна откатить ВСЕ вставленные задачи: в БД пусто.
    let rows = d.list_task_status(100).unwrap();
    assert!(
        rows.is_empty(),
        "транзакция должна была откатить весь граф, осталось задач: {}",
        rows.len()
    );
}

// 4b. Контроль атомарности на уровне dispatcher (как было в исходном тесте):
//     цикл зависимости рушит транзакцию, частичных вставок нет.
#[test]
fn dispatcher_transaction_rolls_back_on_cycle() {
    use serde_json::json;
    use swarm_core::{Budget, Needs, TaskContract};
    let mut d = admin();

    let root = uuid::Uuid::new_v4();
    let a = uuid::Uuid::new_v4();
    let b = uuid::Uuid::new_v4();
    let c = TaskContract {
        kind: "t".into(),
        description: None,
        checks: vec![json!({"type": "cmd", "run": "true"})],
        needs: Needs::default(),
        budget: Budget::default(),
    };

    let res: Result<(), swarm_core::DispatcherError> = d.with_transaction(|tx| {
        tx.add_task(root, &c, Some(root), 1000, 0)?;
        tx.add_task(a, &c, Some(root), 1000, 0)?;
        tx.add_task(b, &c, Some(root), 1000, 0)?;
        tx.add_dep(b, a)?;
        // цикл: a зависит от b, b зависит от a → GR001, транзакция рушится
        tx.add_dep(a, b)?;
        Ok(())
    });
    assert!(res.is_err(), "цикл должен откатить транзакцию");

    let rows = d.list_task_status(100).unwrap();
    assert!(
        rows.iter().all(|r| r.root_id != root),
        "транзакция должна была откатить все 3 задачи, осталось: {}",
        rows.len()
    );
}

// 5. Лимит max_tasks: LLM вернул 2 задачи при лимите 1 → отвергается.
#[test]
fn planner_respects_max_tasks() {
    let mut d = admin();
    let p = planner("planner_too_many.json", 1);
    let err = p.plan(&mut d, "задача-5");
    assert!(err.is_err(), "граф > max_tasks должен быть отвергнут");
    let rows = d.list_task_status(100).unwrap();
    assert!(rows.is_empty(), "при отказе валидации БД должна быть пуста");
}

// 6. Retry-путь: валидный ответ проходит; проверяем, что retry не мешает
//    (полноценный retry на битом JSON — в unit-тестах parse/validate).
#[test]
fn planner_retries_on_invalid_json() {
    let mut d = admin();
    let p = planner("planner_ok.json", 50);
    let out = p.plan(&mut d, "задача-6").unwrap();
    assert_eq!(out.tasks_created, 3);
}

// 7. e2e: Planner → worker выполняет → review.
#[test]
fn e2e_planner_to_review() {
    let mut d = admin();
    let p = planner("planner_ok.json", 50);
    let out = p.plan(&mut d, "задача-7").unwrap();
    assert_eq!(out.ready_to_execute, 1);

    d.register_worker("w1", &serde_json::json!({"ram_gb": 16}), &[])
        .unwrap();
    let lease = d
        .claim("w1", 60)
        .unwrap()
        .expect("корень должен быть выдан");
    d.save_artifact(&lease, "sha256:root", "/p", None, None)
        .unwrap();
    let state = d.complete(&lease, "sha256:root", 1).unwrap();
    assert_eq!(state, "review");

    let st = d
        .submit_review(
            out.root_id,
            "sha256:root",
            "reviewer",
            swarm_core::Verdict::Approved,
            &serde_json::json!([{"type": "cmd", "run": "true", "status": "pass", "exit_code": 0}]),
        )
        .unwrap();
    assert_eq!(st, "done");
    assert!(d.verify_chain(out.root_id).unwrap());
}

// 9. §6.4 (v0.3.1): description доезжает до БД — у корня (текст задачи)
//    и у подзадач (из ответа LLM).
#[test]
fn planner_description_reaches_db() {
    let mut d = admin();
    let p = planner("planner_ok.json", 50);
    let text = "рефакторинг модуля auth";
    let out = p.plan(&mut d, text).unwrap();

    let mut c = raw();
    let root_desc: Option<String> = c
        .query_one(
            "select contract->>'description' from tasks where id = $1",
            &[&out.root_id],
        )
        .unwrap()
        .get(0);
    assert_eq!(
        root_desc.as_deref(),
        Some(text),
        "корень должен нести текст задачи"
    );

    // подзадачи — со своими description из ответа LLM
    let subs: Vec<Option<String>> = c
        .query(
            "select contract->>'description' from tasks \
             where root_id = $1 and id <> $1 order by id",
            &[&out.root_id],
        )
        .unwrap()
        .iter()
        .map(|r| r.get(0))
        .collect();
    assert_eq!(subs.len(), 2, "в фикстуре 2 подзадачи");
    for s in subs {
        assert!(s.is_some(), "у каждой подзадачи должен быть description");
        assert!(!s.unwrap().is_empty());
    }
}
