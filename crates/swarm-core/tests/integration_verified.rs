use postgres::{Client, NoTls};
use serde_json::json;
use std::sync::{Arc, Mutex};
use swarm_core::*;
use uuid::Uuid;

/// DSN из окружения. Приложение v0.3 (ТЗ §0): убран жёстко зашитый порт 5432 —
/// тесты ходят туда, куда указывает TEST_DATABASE_URL (у нас 127.0.0.1:55440).
fn dsn() -> String {
    std::env::var("TEST_DATABASE_URL").unwrap_or_else(|_| {
        "host=localhost port=5432 user=postgres password=pw dbname=v0test".to_string()
    })
}

fn raw() -> Client {
    Client::connect(&dsn(), NoTls).unwrap()
}

/// Отказаться опустошать БД, которая не выглядит тестовой (аудит: `TRUNCATE
/// ... CASCADE` не должен уметь снести рабочую базу).
fn assert_disposable_db(dsn: &str) {
    if std::env::var("SWARM_ALLOW_ANY_DB").ok().as_deref() == Some("1") {
        return;
    }
    let name = db_name(dsn);
    assert!(
        name.starts_with("test_") || name.starts_with("v0") || name.starts_with("c2_"),
        "refusing to TRUNCATE non-disposable database {name:?} \
         (allowed prefixes: test_, v0, c2_); set SWARM_ALLOW_ANY_DB=1 to override"
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

fn reset() {
    assert_disposable_db(&dsn());
    raw()
        .batch_execute("truncate events, reviews, task_deps, artifacts, tasks, workers cascade")
        .unwrap();
}
fn d() -> SwarmDispatcher {
    SwarmDispatcher::connect(&dsn()).unwrap()
}
fn contract() -> TaskContract {
    TaskContract {
        kind: "t".into(),
        description: None,
        checks: vec![json!({"type":"cmd","run":"true"})],
        needs: Needs::default(),
        budget: Budget::default(),
    }
}
fn worker(d: &mut SwarmDispatcher, id: &str) {
    d.register_worker(
        id,
        &json!({"ram_gb": 16, "models": {"coder_class": 2}}),
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
fn full_cycle_claim_save_complete_review() {
    reset();
    let mut d = d();
    worker(&mut d, "w1");
    let t = ready_task(&mut d);
    let lease = d.claim("w1", 60).unwrap().expect("task");
    assert_eq!(lease.task_id, t);
    assert!(d.heartbeat(&lease, 60).unwrap());
    d.save_artifact(&lease, "sha256:aa", "/p", None, None)
        .unwrap();
    assert_eq!(d.complete(&lease, "sha256:aa", 5).unwrap(), "review");
    // F2: ключ проверки = coalesce(id, type||'|'||run); отчёт обязан совпасть по run.
    let checks = json!([{"type":"cmd","run":"true","status":"pass","exit_code":0}]);
    assert_eq!(
        d.submit_review(t, "sha256:aa", "rev", Verdict::Approved, &checks)
            .unwrap(),
        "done"
    );
    assert!(d.verify_chain(t).unwrap());
}

#[test]
fn sqlstate_mapping() {
    reset();
    let mut d = d();
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
    let ok = json!([{"type":"cmd","run":"true","status":"pass"}]);
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
            &json!([{"status":"fail"}])
        ),
        Err(DispatcherError::ChecksFailed)
    ));
}

#[test]
fn zombie_is_fenced_and_heartbeat_false() {
    reset();
    let mut d = d();
    worker(&mut d, "w1");
    let t = ready_task(&mut d);
    let old = d.claim("w1", 60).unwrap().unwrap();
    raw()
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

#[test]
fn concurrent_claims_are_disjoint() {
    reset();
    let mut setup = d();
    for i in 0..8 {
        worker(&mut setup, &format!("w{i}"));
    }
    for _ in 0..24 {
        setup
            .add_task(Uuid::new_v4(), &contract(), None, 1000, 0)
            .unwrap();
    }
    setup.promote_ready().unwrap();
    let got = Arc::new(Mutex::new(Vec::new()));
    let hs: Vec<_> = (0..8)
        .map(|i| {
            let got = got.clone();
            std::thread::spawn(move || {
                let mut d = d();
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

/// Контракт, который SQL принимает (kind не обязателен, число может быть дробным),
/// но который Rust не может десериализовать ПОСЛЕ того, как аренда уже выдана.
///
/// F15: проверки обязаны иметь непустой строковый type, поэтому «непонятность для
/// Rust» несёт дробный budget.attempts (2.5), а не малформед-проверка.
#[test]
fn contract_accepted_by_sql_but_not_by_rust_must_not_strand_the_task() {
    reset();
    let mut d = d();
    worker(&mut d, "w1");
    let t = Uuid::new_v4();
    raw()
        .execute(
            "select add_task($1, null, $2::jsonb, 100, 0)",
            &[
                &t,
                &json!({"checks":[{"type":"cmd","run":"true"}], "budget":{"attempts": 2.5}}),
            ],
        )
        .unwrap();
    d.promote_ready().unwrap();
    let r = d.claim("w1", 3600);
    let state: String = raw()
        .query_one("select state from tasks where id=$1", &[&t])
        .unwrap()
        .get(0);
    eprintln!(
        "claim result: {:?}; task state afterwards: {state}",
        r.as_ref().map(|_| "lease").map_err(|e| e.to_string())
    );
    assert!(
        r.is_ok(),
        "claim failed after lease was already granted -> task stuck in '{state}' for an hour"
    );
}

fn count(ids: &[Uuid]) -> i64 {
    raw()
        .query_one(
            "select count(*) from tasks where id = any($1)",
            &[&ids.to_vec()],
        )
        .unwrap()
        .get(0)
}

#[test]
fn with_transaction_rolls_back_on_error() {
    reset();
    let mut d = d();
    let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
    let r = d.with_transaction(|tx| {
        tx.add_task(a, &contract(), None, 100, 0)?;
        assert_eq!(
            count(&[a]),
            0,
            "uncommitted insert must be invisible to other connections"
        );
        let mut bad = contract();
        bad.checks.clear();
        tx.add_task(b, &bad, None, 100, 0)?; // локальная валидация -> Err
        Ok(())
    });
    assert!(matches!(r, Err(DispatcherError::ContractInvalid)));
    assert_eq!(count(&[a, b]), 0);
}

#[test]
fn with_transaction_rolls_back_on_sql_error_midway() {
    reset();
    let mut d = d();
    let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
    let r = d.with_transaction(|tx| {
        tx.add_task(a, &contract(), None, 100, 0)?;
        tx.add_task(b, &contract(), Some(a), 100, 0)?;
        tx.add_dep(b, a)?;
        tx.add_dep(a, b)?; // цикл (SQL GR001 или GR008) -> откат всех трёх
        Ok(())
    });
    assert!(r.is_err());
    assert_eq!(count(&[a, b]), 0);
}

#[test]
fn with_transaction_commits_whole_graph_atomically() {
    reset();
    let mut d = d();
    let ids: Vec<Uuid> = (0..4).map(|_| Uuid::new_v4()).collect();
    let n = d
        .with_transaction(|tx| {
            tx.add_task(ids[0], &contract(), None, 100, 0)?;
            for w in ids.windows(2) {
                tx.add_task(w[1], &contract(), Some(ids[0]), 100, 0)?;
                tx.add_dep(w[1], w[0])?;
            }
            tx.promote_ready()
        })
        .unwrap();
    assert_eq!(n, 1); // готов только корень
    assert_eq!(count(&ids), 4);
}
