//! Integration-тесты против реального Postgres.
//!
//! Запуск:
//!   TEST_DATABASE_URL=postgres://swarm:pw@localhost:5432/swarm \
//!     cargo test -p swarm-core --test integration -- --ignored --test-threads=1
//!
//! Без `TEST_DATABASE_URL` тесты пропускаются.

use swarm_core::{
    Budget, DispatcherError, Needs, SwarmDispatcher, TaskContract, Verdict,
};
use uuid::Uuid;

fn dsn() -> Option<String> {
    std::env::var("TEST_DATABASE_URL").ok()
}

fn truncate(d: &mut SwarmDispatcher) {
    // Прямой SQL: fixture-очистка. Не через методы, чтобы не зависеть от кода.
    let mut client = postgres::Client::connect(
        &std::env::var("TEST_DATABASE_URL").unwrap(),
        postgres::NoTls,
    )
    .unwrap();
    client
        .batch_execute(
            "TRUNCATE events, reviews, task_deps, artifacts, tasks, workers CASCADE",
        )
        .unwrap();
    // Принудительно возвращаем d в чистое состояние.
    *d = SwarmDispatcher::connect(&std::env::var("TEST_DATABASE_URL").unwrap()).unwrap();
}

fn contract(kind: &str) -> TaskContract {
    TaskContract {
        kind: kind.into(),
        description: None,
        checks: vec![serde_json::json!({"type": "cmd", "run": "true"})],
        needs: Needs::default(),
        budget: Budget::default(),
    }
}

#[test]
#[ignore]
fn add_task_and_list() {
    let Some(dsn) = dsn() else { return };
    let mut d = SwarmDispatcher::connect(&dsn).unwrap();
    truncate(&mut d);

    let id = Uuid::new_v4();
    d.add_task(id, &contract("test"), None, 1000, 5).unwrap();

    let rows = d.list_tasks(None, 10).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, id);
    assert_eq!(rows[0].state, "pending");
    assert_eq!(rows[0].priority, 5);
}

#[test]
#[ignore]
fn add_dep_cycle_rejected() {
    let Some(dsn) = dsn() else { return };
    let mut d = SwarmDispatcher::connect(&dsn).unwrap();
    truncate(&mut d);

    let a = Uuid::new_v4();
    let b = Uuid::new_v4();
    d.add_task(a, &contract("a"), None, 1000, 0).unwrap();
    d.add_task(b, &contract("b"), Some(a), 1000, 0).unwrap();

    d.add_dep(b, a).unwrap();
    let err = d.add_dep(a, b).unwrap_err();
    assert!(matches!(err, DispatcherError::CycleDetected), "got: {err:?}");
}

#[test]
#[ignore]
fn claim_and_fencing() {
    let Some(dsn) = dsn() else { return };
    let mut d = SwarmDispatcher::connect(&dsn).unwrap();
    truncate(&mut d);

    d.register_worker(
        "w1",
        &serde_json::json!({"ram_gb": 16, "models": {"coder_class": 2}}),
        &[],
    )
    .unwrap();

    let id = Uuid::new_v4();
    d.add_task(id, &contract("t"), None, 1000, 0).unwrap();
    d.promote_ready().unwrap();

    let lease = d.claim("w1", 300).unwrap().expect("lease");
    assert_eq!(lease.task_id, id);
    assert_eq!(lease.epoch, 1);

    // Вторая попытка claim — пусто (задача занята).
    assert!(d.claim("w1", 300).unwrap().is_none());

    // Артефакт с правильным epoch — ок.
    d.save_artifact(&lease, "sha256:aaa", "/tmp/a", None, None)
        .unwrap();

    // Артефакт с чужим epoch — LeaseLost.
    let mut stale = lease.clone();
    stale.epoch = 0;
    let err = d
        .save_artifact(&stale, "sha256:bbb", "/tmp/b", None, None)
        .unwrap_err();
    assert!(matches!(err, DispatcherError::LeaseLost));
}

#[test]
#[ignore]
fn complete_and_review_self_rejected() {
    let Some(dsn) = dsn() else { return };
    let mut d = SwarmDispatcher::connect(&dsn).unwrap();
    truncate(&mut d);

    d.register_worker("w1", &serde_json::json!({"ram_gb": 16}), &[])
        .unwrap();
    let id = Uuid::new_v4();
    d.add_task(id, &contract("t"), None, 1000, 0).unwrap();
    d.promote_ready().unwrap();
    let lease = d.claim("w1", 300).unwrap().unwrap();
    d.save_artifact(&lease, "sha256:x", "/tmp/x", None, None)
        .unwrap();
    let state = d.complete(&lease, "sha256:x", 10).unwrap();
    assert_eq!(state, "review");

    // Рецензент = автор → SelfReview.
    let checks = serde_json::json!([{"type": "cmd", "status": "pass"}]);
    let err = d
        .submit_review(id, "sha256:x", "w1", Verdict::Approved, &checks)
        .unwrap_err();
    assert!(matches!(err, DispatcherError::SelfReview));

    // Другой рецензент → done.
    let state = d
        .submit_review(id, "sha256:x", "reviewer", Verdict::Approved, &checks)
        .unwrap();
    assert_eq!(state, "done");
}

#[test]
#[ignore]
fn verify_chain_after_operations() {
    let Some(dsn) = dsn() else { return };
    let mut d = SwarmDispatcher::connect(&dsn).unwrap();
    truncate(&mut d);

    d.register_worker("w1", &serde_json::json!({"ram_gb": 16}), &[])
        .unwrap();
    let id = Uuid::new_v4();
    d.add_task(id, &contract("t"), None, 1000, 0).unwrap();
    d.promote_ready().unwrap();
    let lease = d.claim("w1", 300).unwrap().unwrap();
    d.save_artifact(&lease, "sha256:x", "/tmp/x", None, None)
        .unwrap();
    d.complete(&lease, "sha256:x", 10).unwrap();

    assert!(d.verify_chain(id).unwrap());
}

// ============================================================
// Новые тесты step 1 v0.2 (см. docs/TZ_v0.2_step1_fixes.md, §8.1)
// ============================================================

fn raw(dsn: &str) -> postgres::Client {
    postgres::Client::connect(dsn, postgres::NoTls).unwrap()
}

fn count(dsn: &str, ids: &[Uuid]) -> i64 {
    raw(dsn)
        .query_one("select count(*) from tasks where id = any($1)", &[&ids.to_vec()])
        .unwrap()
        .get(0)
}

/// Контракт, который SQL принимает (kind не обязателен, число может быть
/// дробным), но который Rust не может десериализовать ПОСЛЕ того, как аренда
/// уже выдана. Регрессия на баг из `types.rs`: `Lease.contract` — сырой JSON,
/// разбор — только явно через `typed_contract()`.
#[test]
#[ignore]
fn contract_accepted_by_sql_but_not_by_rust_must_not_strand_the_task() {
    let Some(dsn) = dsn() else { return };
    let mut d = SwarmDispatcher::connect(&dsn).unwrap();
    truncate(&mut d);
    d.register_worker("w1", &serde_json::json!({"ram_gb": 16}), &[])
        .unwrap();

    let t = Uuid::new_v4();
    raw(&dsn)
        .execute(
            "select add_task($1, null, $2::jsonb, 100, 0)",
            &[
                &t,
                &serde_json::json!({"checks":[{"a":1}], "budget":{"attempts": 2.5}}),
            ],
        )
        .unwrap();
    d.promote_ready().unwrap();
    let r = d.claim("w1", 3600);
    let state: String = raw(&dsn)
        .query_one("select state from tasks where id=$1", &[&t])
        .unwrap()
        .get(0);
    assert!(
        r.is_ok(),
        "claim failed after lease was already granted -> task stuck in '{state}'"
    );
}

/// Параллельные claim не должны выдать одну задачу дважды.
#[test]
#[ignore]
fn concurrent_claims_are_disjoint() {
    let Some(dsn) = dsn() else { return };
    let mut d = SwarmDispatcher::connect(&dsn).unwrap();
    truncate(&mut d);
    for i in 0..8 {
        d.register_worker(&format!("w{i}"), &serde_json::json!({"ram_gb": 16}), &[])
            .unwrap();
    }
    for _ in 0..24 {
        d.add_task(Uuid::new_v4(), &contract("t"), None, 1000, 0)
            .unwrap();
    }
    d.promote_ready().unwrap();

    let got = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let hs: Vec<_> = (0..8)
        .map(|i| {
            let got = got.clone();
            let dsn = dsn.clone();
            std::thread::spawn(move || {
                let mut d = SwarmDispatcher::connect(&dsn).unwrap();
                while let Some(l) = d.claim(&format!("w{i}"), 60).unwrap() {
                    got.lock().unwrap().push(l.task_id);
                }
            })
        })
        .collect();
    for h in hs {
        h.join().unwrap();
    }
    let mut v = got.lock().unwrap().clone();
    assert_eq!(v.len(), 24);
    v.sort();
    v.dedup();
    assert_eq!(v.len(), 24);
}

/// Локальная ошибка в замыкании -> rollback. Именно этот тест ловит мутацию
/// «коммитить даже при ошибке» (M2): путь идёт через код обёртки, а не Postgres.
#[test]
#[ignore]
fn with_transaction_rolls_back_on_error() {
    let Some(dsn) = dsn() else { return };
    let mut d = SwarmDispatcher::connect(&dsn).unwrap();
    truncate(&mut d);
    let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
    let r = d.with_transaction(|tx| {
        tx.add_task(a, &contract("t"), None, 100, 0)?;
        assert_eq!(
            count(&dsn, &[a]),
            0,
            "uncommitted insert must be invisible to other connections"
        );
        let mut bad = contract("t");
        bad.checks.clear();
        tx.add_task(b, &bad, None, 100, 0)?; // локальная валидация -> Err
        Ok(())
    });
    assert!(matches!(r, Err(DispatcherError::ContractInvalid)));
    assert_eq!(count(&dsn, &[a, b]), 0);
}

/// SQL-ошибка посреди графа -> откат всех трёх вставок (частичного графа нет).
#[test]
#[ignore]
fn with_transaction_rolls_back_on_sql_error_midway() {
    let Some(dsn) = dsn() else { return };
    let mut d = SwarmDispatcher::connect(&dsn).unwrap();
    truncate(&mut d);
    let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
    let r = d.with_transaction(|tx| {
        tx.add_task(a, &contract("t"), None, 100, 0)?;
        tx.add_task(b, &contract("t"), Some(a), 100, 0)?;
        tx.add_dep(b, a)?;
        tx.add_dep(a, b)?; // цикл (SQL GR001/GR008) -> откат всех трёх
        Ok(())
    });
    assert!(r.is_err());
    assert_eq!(count(&dsn, &[a, b]), 0);
}

/// Успешный граф коммитится целиком и атомарно.
#[test]
#[ignore]
fn with_transaction_commits_whole_graph_atomically() {
    let Some(dsn) = dsn() else { return };
    let mut d = SwarmDispatcher::connect(&dsn).unwrap();
    truncate(&mut d);
    let ids: Vec<Uuid> = (0..4).map(|_| Uuid::new_v4()).collect();
    let n = d
        .with_transaction(|tx| {
            tx.add_task(ids[0], &contract("t"), None, 100, 0)?;
            for w in ids.windows(2) {
                tx.add_task(w[1], &contract("t"), Some(ids[0]), 100, 0)?;
                tx.add_dep(w[1], w[0])?;
            }
            tx.promote_ready()
        })
        .unwrap();
    assert_eq!(n, 1); // готов только корень
    assert_eq!(count(&dsn, &ids), 4);
}
