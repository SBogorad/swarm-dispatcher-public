//! Альтернативные формулировки интеграционных тестов (из `integration_verified.rs`).
//!
//! Не дублируют `integration.rs` по покрытию, но полезны как независимые
//! формулировки тех же сценариев. Запуск:
//!
//!   TEST_DATABASE_URL=postgres://... \
//!     cargo test -p swarm-core --test integration_alt -- --ignored --test-threads=1

use swarm_core::{Budget, DispatcherError, Needs, SwarmDispatcher, TaskContract, Verdict};
use uuid::Uuid;

fn dsn() -> Option<String> {
    std::env::var("TEST_DATABASE_URL").ok()
}

fn raw(dsn: &str) -> postgres::Client {
    postgres::Client::connect(dsn, postgres::NoTls).unwrap()
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

fn reset(dsn: &str) {
    assert_disposable_db(dsn);
    raw(dsn)
        .batch_execute("truncate events, reviews, task_deps, artifacts, tasks, workers cascade")
        .unwrap();
}

fn contract() -> TaskContract {
    TaskContract {
        kind: "t".into(),
        description: None,
        checks: vec![serde_json::json!({"type":"cmd","run":"true"})],
        needs: Needs::default(),
        budget: Budget::default(),
    }
}

fn worker(d: &mut SwarmDispatcher, id: &str) {
    d.register_worker(
        id,
        &serde_json::json!({"ram_gb": 16, "models": {"coder_class": 2}}),
        &[],
    )
    .unwrap();
}

fn ready_task(d: &mut SwarmDispatcher) -> Uuid {
    let t = Uuid::new_v4();
    d.add_task(t, &contract(), None, 1000, 0).unwrap();
    d.promote_ready().unwrap();
    t
}

#[test]
#[ignore]
fn full_cycle_claim_save_complete_review() {
    let Some(dsn) = dsn() else { return };
    reset(&dsn);
    let mut d = SwarmDispatcher::connect(&dsn).unwrap();
    worker(&mut d, "w1");
    let t = ready_task(&mut d);
    let lease = d.claim("w1", 60).unwrap().expect("task");
    assert_eq!(lease.task_id, t);
    // Контракт приходит сырым JSON; типизированный разбор — явно.
    assert_eq!(lease.typed_contract().unwrap().kind, "t");
    assert!(d.heartbeat(&lease, 60).unwrap());
    d.save_artifact(&lease, "sha256:aa", "/p", None, None)
        .unwrap();
    assert_eq!(d.complete(&lease, "sha256:aa", 5).unwrap(), "review");
    let checks = serde_json::json!([{"type":"cmd","run":"true","status":"pass","exit_code":0}]);
    assert_eq!(
        d.submit_review(t, "sha256:aa", "rev", Verdict::Approved, &checks)
            .unwrap(),
        "done"
    );
    assert!(d.verify_chain(t).unwrap());
}

#[test]
#[ignore]
fn sqlstate_mapping() {
    let Some(dsn) = dsn() else { return };
    reset(&dsn);
    let mut d = SwarmDispatcher::connect(&dsn).unwrap();
    worker(&mut d, "w1");
    let a = ready_task(&mut d);
    // WorkerUnknown
    assert!(matches!(
        d.claim("ghost", 60),
        Err(DispatcherError::WorkerUnknown)
    ));
    // ContractInvalid (локально)
    let mut bad = contract();
    bad.checks.clear();
    assert!(matches!(
        d.add_task(Uuid::new_v4(), &bad, None, 10, 0),
        Err(DispatcherError::ContractInvalid)
    ));
    // Cycle / TaskNotFound
    let b = Uuid::new_v4();
    d.add_task(b, &contract(), Some(a), 1000, 0).unwrap();
    d.add_dep(b, a).unwrap();
    assert!(matches!(
        d.add_dep(a, b),
        Err(DispatcherError::TaskNotPending) | Err(DispatcherError::CycleDetected)
    ));
    assert!(matches!(
        d.add_dep(b, Uuid::new_v4()),
        Err(DispatcherError::TaskNotFound)
    ));
    // LeaseLost + SelfReview + ReviewStale
    let lease = d.claim("w1", 60).unwrap().unwrap();
    d.save_artifact(&lease, "sha256:aa", "/p", None, None)
        .unwrap();
    d.complete(&lease, "sha256:aa", 1).unwrap();
    assert!(matches!(
        d.complete(&lease, "sha256:aa", 1),
        Err(DispatcherError::LeaseLost)
    ));
    let ok = serde_json::json!([{"type":"cmd","run":"true","status":"pass"}]);
    assert!(matches!(
        d.submit_review(a, "sha256:aa", "w1", Verdict::Approved, &ok),
        Err(DispatcherError::SelfReview)
    ));
    assert!(matches!(
        d.submit_review(a, "sha256:zz", "rev", Verdict::Approved, &ok),
        Err(DispatcherError::ReviewStale)
    ));
    assert!(matches!(
        d.submit_review(
            a,
            "sha256:aa",
            "rev",
            Verdict::Approved,
            &serde_json::json!([{"status":"fail"}])
        ),
        Err(DispatcherError::ChecksFailed)
    ));
}

#[test]
#[ignore]
fn zombie_is_fenced_and_heartbeat_false() {
    let Some(dsn) = dsn() else { return };
    reset(&dsn);
    let mut d = SwarmDispatcher::connect(&dsn).unwrap();
    worker(&mut d, "w1");
    let t = ready_task(&mut d);
    let old = d.claim("w1", 60).unwrap().unwrap();
    raw(&dsn)
        .execute(
            "update tasks set lease_until = now() - interval '1 second' where id=$1",
            &[&t],
        )
        .unwrap();
    assert!(!d.heartbeat(&old, 60).unwrap());
    assert_eq!(d.reclaim_expired().unwrap(), 1);
    let new = d.claim("w1", 60).unwrap().unwrap();
    assert!(new.epoch > old.epoch);
    assert!(matches!(
        d.save_artifact(&old, "sha256:x", "/p", None, None),
        Err(DispatcherError::LeaseLost)
    ));
    assert!(d.heartbeat(&new, 60).unwrap());
}
