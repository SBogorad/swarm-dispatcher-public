//! F5 — heartbeat fail-fast + reconnect.
//!
//! Дефекты (pre-fix):
//!   1. Потеря аренды (`heartbeat` -> Ok(false)) только логировалась внутри
//!      фонового потока; главный поток продолжал работу и тратил LLM-вызов.
//!   2. Ошибка heartbeat (обрыв соединения) логировалась и игнорировалась —
//!      аренда тихо истекала.
//!   3. Если heartbeat-поток не стартовал (не смог подключиться), главный поток
//!      об этом не знал: аренда истекала, а воркер считал, что жива.
//!
//! Фикс: общий `LeaseHealth` (Ok/Lost/Dead) между потоком и главным; fail-fast
//! (Outcome::LeaseLost) до и после LLM-вызова; реконнект при транзиентной
//! ошибке; `Dead` при нестартовавшем/невосстановимом потоке.
//!
//! Запуск (нужен живой Postgres с фикс-схемой; имя БД test_/v0/c2_):
//!   TEST_DATABASE_URL="host=127.0.0.1 port=55440 user=postgres password=pw dbname=v0worker" \
//!     cargo test -p swarm-worker --test worker_heartbeat_f5 -- --test-threads=1

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use swarm_core::SwarmDispatcher;
use swarm_llm::{LlmClient, LlmError, LlmRequest, LlmResponse};
use swarm_worker::{process_lease, BudgetPolicy, Outcome, RetryPolicy, RunLimits, WorkerConfig};

fn dsn() -> String {
    std::env::var("TEST_DATABASE_URL").unwrap_or_else(|_| {
        "host=127.0.0.1 port=55440 user=postgres password=pw dbname=v0worker".into()
    })
}

fn db_name(dsn: &str) -> String {
    dsn.split_whitespace()
        .find_map(|p| p.strip_prefix("dbname="))
        .unwrap_or("")
        .to_string()
}

fn clear() {
    let name = db_name(&dsn());
    assert!(
        name.starts_with("test_") || name.starts_with("v0") || name.starts_with("c2_"),
        "refusing to TRUNCATE non-disposable database {name:?}"
    );
    let mut c = postgres::Client::connect(&dsn(), postgres::NoTls).expect("raw connect");
    c.batch_execute("truncate events, reviews, task_deps, artifacts, tasks, workers cascade")
        .expect("truncate");
}

fn raw() -> postgres::Client {
    postgres::Client::connect(&dsn(), postgres::NoTls).expect("raw")
}

fn tmpdir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "swarm_hb_f5_{tag}_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn cfg(dir: &PathBuf, lease_sec: i32) -> WorkerConfig {
    WorkerConfig {
        artifacts_dir: dir.clone(),
        lease_sec,
        max_tokens: 4096,
        llm_timeout_sec: 5,
        retry: RetryPolicy::default(),
        work_sec: 0,
        budget: BudgetPolicy::default(),
        run_limits: RunLimits::default(),
        llm_calls: None,
        model_selector: None,
    }
}

fn contract_json() -> serde_json::Value {
    serde_json::json!({
        "kind": "code.generate",
        "checks": [{"type": "cmd", "run": "true"}],
        "needs": {},
        "budget": {"attempts": 3, "max_rework": 2}
    })
}

fn ready_task(worker: &str) -> uuid::Uuid {
    let t = uuid::Uuid::new_v4();
    let mut c = raw();
    c.execute(
        "select register_worker($1, '{\"ram_gb\":16}'::jsonb)",
        &[&worker],
    )
    .unwrap();
    c.execute(
        "select add_task($1, null, $2::jsonb, 100000, 0)",
        &[&t, &contract_json()],
    )
    .unwrap();
    c.execute("select promote_ready()", &[]).unwrap();
    t
}

struct MockLlm {
    body: String,
    calls: AtomicUsize,
}

impl MockLlm {
    fn ok(body: &str) -> Self {
        Self {
            body: body.to_string(),
            calls: AtomicUsize::new(0),
        }
    }
    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

impl LlmClient for MockLlm {
    fn complete(&self, _req: &LlmRequest) -> Result<LlmResponse, LlmError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(LlmResponse {
            text: self.body.clone(),
            tokens_in: 1,
            tokens_out: 1,
            model: "mock".into(),
        })
    }
}

fn state_of(t: uuid::Uuid) -> String {
    raw()
        .query_one("select state from tasks where id=$1", &[&t])
        .unwrap()
        .get(0)
}

// ---------- 1. heartbeat thread cannot start -> fail-fast, LLM not called ----------

#[test]
fn f5_dead_heartbeat_fails_fast_without_llm() {
    clear();
    let mut d = SwarmDispatcher::connect(&dsn()).unwrap();
    let dir = tmpdir("dead");
    let _t = ready_task("w1");
    let lease = d.claim("w1", 3).unwrap().expect("lease"); // lease=3 -> interval=1s

    // Искусственно битый DSN для heartbeat: поток не сможет подключиться.
    let bad = "host=127.0.0.1 port=1 user=nobody password=nope dbname=nope";
    let llm = MockLlm::ok("should-not-be-used");
    let c = cfg(&dir, 3);

    let out = process_lease(&mut d, &lease, &llm, &c, Some(bad.to_string())).unwrap();
    assert!(
        matches!(out, Outcome::LeaseLost),
        "F5: нестартовавший heartbeat должен дать LeaseLost, получено {out:?}"
    );
    assert_eq!(
        llm.calls(),
        0,
        "F5: при мёртвом heartbeat LLM не должен вызываться (fail-fast)"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------- 2. healthy heartbeat -> normal review ----------

#[test]
fn f5_healthy_heartbeat_completes_normally() {
    clear();
    let mut d = SwarmDispatcher::connect(&dsn()).unwrap();
    let dir = tmpdir("ok");
    let _t = ready_task("w1");
    let lease = d.claim("w1", 9).unwrap().expect("lease"); // lease=9 -> interval=3s
    let llm = MockLlm::ok("done-body");
    let c = cfg(&dir, 9);

    let out = process_lease(&mut d, &lease, &llm, &c, Some(dsn())).unwrap();
    assert!(
        matches!(out, Outcome::Review { .. }),
        "F5: здоровый heartbeat -> Review, получено {out:?}"
    );
    assert_eq!(llm.calls(), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------- 3. lease removed underneath -> LeaseLost (fail-fast) ----------

#[test]
fn f5_lease_revoked_mid_flight_fails_fast() {
    clear();
    let mut d = SwarmDispatcher::connect(&dsn()).unwrap();
    let dir = tmpdir("revoked");
    let t = ready_task("w1");
    let lease = d.claim("w1", 3).unwrap().expect("lease"); // interval=1s

    // Отбираем аренду в watch-dog-стиле: задача уходит обратно в ready,
    // lease_owner сбрасывается -> heartbeat вернёт false.
    raw()
        .execute(
            "update tasks set state='ready', lease_owner=null, lease_until=null where id=$1",
            &[&t],
        )
        .unwrap();

    // work_sec=2 > interval(1s): heartbeat успеет обнаружить потерю до LLM.
    let mut c = cfg(&dir, 3);
    c.work_sec = 2;
    let llm = MockLlm::ok("late");
    let started = Instant::now();
    let out = process_lease(&mut d, &lease, &llm, &c, Some(dsn())).unwrap();
    let elapsed = started.elapsed();

    assert!(
        matches!(out, Outcome::LeaseLost),
        "F5: потерянная аренда -> LeaseLost, получено {out:?}"
    );
    assert!(
        elapsed.as_secs() < 30,
        "F5: fail-fast должен прерваться, а не ждать весь lease; прошло {elapsed:?}"
    );
    // Задача не должна быть завершена воркером.
    assert_ne!(state_of(t), "review");
    let _ = std::fs::remove_dir_all(&dir);
}
