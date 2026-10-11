//! Интеграционные тесты shell tools + Docker sandbox (ТЗ v0.4 §8.2).
//!
//! Требуется: реальный Docker (образ `agent-sandbox:1.85`) и реальный Postgres
//! с полным порядком инициализации, включая патч аудита:
//!   schema_v0.sql → step2/roles.sql → step2b/roles_2b.sql
//!   → step2/roles_patch_planner_status.sql → step2/roles_patch_tool_call_audit.sql
//!
//! Запуск:
//!   TEST_DATABASE_URL="host=127.0.0.1 port=55440 user=swarm password=*** dbname=v0sandbox" \
//!     cargo test -p swarm-worker --test worker_sandbox -- --test-threads=1
//!
//! Тесты меняют состояние Docker (создают/удаляют контейнеры `swarm-sandbox-*`).

use std::collections::VecDeque;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use swarm_core::SwarmDispatcher;
use swarm_llm::{LlmClient, LlmError, LlmRequest, LlmResponse};
use swarm_worker::agent_loop::{self, AgentLoopConfig, NoopSink};
use swarm_worker::allowlist::Allowlist;
use swarm_worker::audit::{count_tool_calls, PgAuditSink};
use swarm_worker::sandbox::{cleanup_stale, Sandbox, SandboxConfig};
use swarm_worker::WorkerError;

fn dsn() -> String {
    std::env::var("TEST_DATABASE_URL").unwrap_or_else(|_| {
        "host=127.0.0.1 port=55440 user=swarm password=pw dbname=v0sandbox".into()
    })
}

fn image() -> String {
    std::env::var("SANDBOX_IMAGE").unwrap_or_else(|_| "agent-sandbox:1.85".into())
}

fn sb_cfg(timeout_sec: u64) -> SandboxConfig {
    SandboxConfig {
        image: image(),
        cmd_timeout_sec: timeout_sec,
        // FIND-01/Волна 5.1 + N3: префикс с worker-идентификатором обязателен
        // для изоляции cleanup_stale и для снятия runtime-гейта.
        name_prefix: "swarm-sandbox-test".into(),
        ..Default::default()
    }
}

/// Включить shell-tools для тестов (иначе `Sandbox::start` откажет).
///
/// SAFETY: тесты идут с `--test-threads=1` (см. докстринг файла).
fn enable_shell_tools() {
    std::env::set_var("WORKER_SHELL_TOOLS", "1");
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

fn clear() {
    let dsn = dsn();
    assert_disposable_db(&dsn);
    let mut c = postgres::Client::connect(&dsn, postgres::NoTls).expect("raw connect");
    c.batch_execute("truncate events, reviews, task_deps, artifacts, tasks, workers cascade")
        .expect("truncate");
}

fn admin() -> SwarmDispatcher {
    clear();
    SwarmDispatcher::connect(&dsn()).expect("connect")
}

/// Есть ли контейнер с таким именем (включая остановленные).
fn docker_has(name: &str) -> bool {
    let out = Command::new("docker")
        .args([
            "ps",
            "-a",
            "--filter",
            &format!("name={name}"),
            "--format",
            "{{.Names}}",
        ])
        .output()
        .expect("docker ps");
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .any(|l| l.trim() == name)
}

fn short() -> String {
    // уникальный суффикс, чтобы тесты не пересекались
    format!(
        "{:x}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
            % 0xffff_ffff
    )
}

/// LLM, отдающий заранее заданную очередь ответов.
struct ScriptedLlm {
    steps: Mutex<VecDeque<String>>,
    calls: AtomicUsize,
    requests: Mutex<Vec<String>>,
}

impl ScriptedLlm {
    fn new(steps: Vec<&str>) -> Self {
        Self {
            steps: Mutex::new(steps.into_iter().map(str::to_string).collect()),
            calls: AtomicUsize::new(0),
            requests: Mutex::new(Vec::new()),
        }
    }
}

impl LlmClient for ScriptedLlm {
    fn complete(&self, req: &LlmRequest) -> Result<LlmResponse, LlmError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.requests.lock().unwrap().push(req.user.clone());
        let mut s = self.steps.lock().unwrap();
        let text = s
            .pop_front()
            .ok_or_else(|| LlmError::Config("no mock steps".into()))?;
        Ok(LlmResponse {
            text,
            tokens_in: 1,
            tokens_out: 1,
            model: "scripted".into(),
        })
    }
}

/// LLM, который всегда возвращает один и тот же tool call (для проверки лимита).
struct LoopLlm(&'static str);

impl LlmClient for LoopLlm {
    fn complete(&self, _req: &LlmRequest) -> Result<LlmResponse, LlmError> {
        Ok(LlmResponse {
            text: self.0.to_string(),
            tokens_in: 1,
            tokens_out: 1,
            model: "loop".into(),
        })
    }
}

fn task_contract() -> serde_json::Value {
    serde_json::json!({
        "kind": "code.generate",
        "description": "создать крейт и прогнать тест",
        "checks": [{"type": "cmd", "run": "true"}]
    })
}

// ---------------------------------------------------------------- §8.2

// 1. sandbox_starts_and_stops (+ M5: корень read-only)
#[test]
#[ignore = "требует живых Docker + PostgreSQL; см. докстринг файла"]
fn sandbox_starts_and_stops() {
    enable_shell_tools();
    let cfg = sb_cfg(60);
    let tag = short();
    let mut sb = Sandbox::start(&cfg, &tag).expect("start");
    let name = sb.name().to_string();
    assert!(
        docker_has(&name),
        "контейнер {name} должен быть виден в docker ps"
    );

    // M5 (ТЗ §9): без --read-only запись в корень прошла бы.
    let ro = sb.exec("touch /etc/swarm_should_not_write").expect("exec");
    assert!(!ro.success(), "корень должен быть read-only: {}", ro.stdout);

    sb.stop().expect("stop");
    assert!(!docker_has(&name), "после stop контейнера быть не должно");
}

// 2. worker_runs_cargo_test
#[test]
#[ignore = "требует живых Docker + PostgreSQL; см. докстринг файла"]
fn worker_runs_cargo_test() {
    enable_shell_tools();
    let cfg = sb_cfg(120);
    let sb = Sandbox::start(&cfg, &short()).expect("start");
    let init = sb.exec("cargo init --lib --name demo").expect("exec init");
    assert!(
        init.success(),
        "cargo init: {} {}",
        init.stdout,
        init.stderr
    );
    let test = sb.exec("cargo test").expect("exec test");
    assert!(
        test.success(),
        "cargo test: exit={} {}",
        test.exit_code,
        test.stderr
    );
    assert!(!test.output_sha256.is_empty());
}

// 3. worker_rm_rejected
#[test]
#[ignore = "требует живых Docker + PostgreSQL; см. докстринг файла"]
fn worker_rm_rejected() {
    enable_shell_tools();
    let al = Allowlist::default();
    let e = al.check("rm -rf /").unwrap_err();
    match e {
        WorkerError::AllowlistRejected { ref reason, .. } => {
            assert!(
                reason.contains("deny") || reason.contains("allowlist"),
                "{reason}"
            );
        }
        other => panic!("ожидался AllowlistRejected, got {other:?}"),
    }
    // и через agent loop: отказ приходит как результат, не как падение
    let cfg = AgentLoopConfig {
        sandbox: sb_cfg(30),
        ..Default::default()
    };
    let sb = Sandbox::start(&cfg.sandbox, &short()).expect("start");
    let llm = ScriptedLlm::new(vec![
        r#"{"tool":"run_shell","args":{"command":"rm -rf /"}}"#,
        r#"{"done":true,"result":"отказ получен"}"#,
    ]);
    let mut sink = NoopSink;
    let res = agent_loop::run(
        &llm,
        &sb,
        &task_contract(),
        &Allowlist::default(),
        &cfg,
        &mut sink,
    )
    .expect("loop");
    assert_eq!(res.final_text, "отказ получен");
}

// 4. worker_timeout_handled
#[test]
#[ignore = "требует живых Docker + PostgreSQL; см. докстринг файла"]
fn worker_timeout_handled() {
    enable_shell_tools();
    let cfg = sb_cfg(1);
    let sb = Sandbox::start(&cfg, &short()).expect("start");
    let e = sb.exec("sleep 100").unwrap_err();
    match e {
        WorkerError::CommandTimeout { timeout_sec, .. } => assert_eq!(timeout_sec, 1),
        other => panic!("ожидался CommandTimeout, got {other:?}"),
    }
}

// 5. worker_network_isolated
#[test]
#[ignore = "требует живых Docker + PostgreSQL; см. докстринг файла"]
fn worker_network_isolated() {
    enable_shell_tools();
    let cfg = sb_cfg(30);
    let sb = Sandbox::start(&cfg, &short()).expect("start");
    // git есть в образе и в allowlist; без сети ls-remote обязан провалиться.
    let out = sb
        .exec("git ls-remote https://github.com/rust-lang/rust")
        .expect("exec");
    assert!(
        !out.success(),
        "при --network none сеть не должна работать: {}",
        out.stdout
    );
}

// 6. agent_loop_iterates
#[test]
#[ignore = "требует живых Docker + PostgreSQL; см. докстринг файла"]
fn agent_loop_iterates() {
    enable_shell_tools();
    let cfg = AgentLoopConfig {
        sandbox: sb_cfg(120),
        ..Default::default()
    };
    let sb = Sandbox::start(&cfg.sandbox, &short()).expect("start");
    let llm = ScriptedLlm::new(vec![
        r#"{"tool":"run_shell","args":{"command":"cargo init --lib --name demo"}}"#,
        r#"{"tool":"run_shell","args":{"command":"ls -la"}}"#,
        r#"{"tool":"read_file","args":{"path":"Cargo.toml"}}"#,
        r#"{"done":true,"result":"три вызова выполнены"}"#,
    ]);
    let mut sink = NoopSink;
    let res = agent_loop::run(
        &llm,
        &sb,
        &task_contract(),
        &Allowlist::default(),
        &cfg,
        &mut sink,
    )
    .expect("loop");
    assert_eq!(res.tool_calls, 3, "должно быть 3 tool call");
    assert_eq!(res.iterations, 4);
    assert_eq!(res.final_text, "три вызова выполнены");
}

// 7. agent_loop_executes_dsml_calls (DeepSeek native format)
#[test]
#[ignore = "требует живых Docker + PostgreSQL; см. докстринг файла"]
fn agent_loop_executes_dsml_calls() {
    enable_shell_tools();
    let cfg = AgentLoopConfig {
        sandbox: sb_cfg(30),
        ..Default::default()
    };
    let sb = Sandbox::start(&cfg.sandbox, &short()).expect("start");
    let llm = ScriptedLlm::new(vec![
        "<｜｜DSML｜｜ calls>\n<｜｜DSML｜｜ invoke name=\"run_shell\"><｜｜DSML｜｜ parameter name=\"args\" string=\"false\">{\"command\":\"echo first\"}</｜｜DSML｜｜ parameter></｜｜DSML｜｜ invoke><｜｜DSML｜｜ invoke name=\"run_shell\"><｜｜DSML｜｜ parameter name=\"args\" string=\"false\">{\"command\":\"echo second\"}</｜｜DSML｜｜ parameter></｜｜DSML｜｜ invoke></｜｜DSML｜｜ calls>",
        r#"{"done":true,"result":"DSML calls executed"}"#,
    ]);
    let mut sink = NoopSink;
    let res = agent_loop::run(
        &llm,
        &sb,
        &task_contract(),
        &Allowlist::default(),
        &cfg,
        &mut sink,
    )
    .expect("Agent Loop should execute DSML tool calls");
    assert_eq!(res.tool_calls, 2);
    assert_eq!(res.iterations, 2);
    assert_eq!(res.final_text, "DSML calls executed");
}

// 8. Batch stops at first policy rejection; later calls are not executed.
#[test]
#[ignore = "требует живых Docker + PostgreSQL; см. докстринг файла"]
fn agent_loop_stops_batch_after_reject() {
    enable_shell_tools();
    let cfg = AgentLoopConfig {
        sandbox: sb_cfg(30),
        ..Default::default()
    };
    let sb = Sandbox::start(&cfg.sandbox, &short()).expect("start");
    let llm = ScriptedLlm::new(vec![
        concat!(
            "<｜｜DSML｜｜ calls>",
            "<｜｜DSML｜｜ invoke name=\"run_shell\"><｜｜DSML｜｜ parameter name=\"command\" string=\"true\">echo before</｜｜DSML｜｜ parameter></｜｜DSML｜｜ invoke>",
            "<｜｜DSML｜｜ invoke name=\"run_shell\"><｜｜DSML｜｜ parameter name=\"command\" string=\"true\">rm -rf /</｜｜DSML｜｜ parameter></｜｜DSML｜｜ invoke>",
            "<｜｜DSML｜｜ invoke name=\"run_shell\"><｜｜DSML｜｜ parameter name=\"command\" string=\"true\">echo after</｜｜DSML｜｜ parameter></｜｜DSML｜｜ invoke>",
            "</｜｜DSML｜｜ calls>"
        ),
        r#"{"done":true,"result":"model recovered"}"#,
    ]);
    let mut sink = NoopSink;
    let res = agent_loop::run(
        &llm,
        &sb,
        &task_contract(),
        &Allowlist::default(),
        &cfg,
        &mut sink,
    )
    .expect("loop should return to LLM after rejecting a batch item");
    assert_eq!(
        res.tool_calls, 2,
        "calls after the rejected item must not run"
    );
    assert_eq!(
        res.iterations, 2,
        "LLM should receive rejection and answer next"
    );
    assert_eq!(res.final_text, "model recovered");
    assert_eq!(llm.calls.load(Ordering::SeqCst), 2);
    let requests = llm.requests.lock().unwrap();
    assert!(
        requests[1].contains("REJECTED:"),
        "model must receive the policy rejection: {}",
        requests[1]
    );
    assert!(requests[1].contains("остальные вызовы не выполнялись"));
}

// 9. agent_loop_max_iterations
#[test]
#[ignore = "требует живых Docker + PostgreSQL; см. докстринг файла"]
fn agent_loop_max_iterations() {
    enable_shell_tools();
    let cfg = AgentLoopConfig {
        max_iterations: 3,
        sandbox: sb_cfg(30),
        ..Default::default()
    };
    let sb = Sandbox::start(&cfg.sandbox, &short()).expect("start");
    let llm = LoopLlm(r#"{"tool":"run_shell","args":{"command":"echo loop"}}"#);
    let mut sink = NoopSink;
    let e = agent_loop::run(
        &llm,
        &sb,
        &task_contract(),
        &Allowlist::default(),
        &cfg,
        &mut sink,
    )
    .unwrap_err();
    assert!(
        matches!(e, WorkerError::MaxIterations { max: 3 }),
        "got {e:?}"
    );
}

// 8. tool_call_events_logged
#[test]
#[ignore = "требует живых Docker + PostgreSQL; см. докстринг файла"]
fn tool_call_events_logged() {
    enable_shell_tools();
    let mut d = admin();
    d.register_worker("w1", &serde_json::json!({"ram_gb": 16}), &[])
        .unwrap();
    let mut tx_id = uuid::Uuid::nil();
    d.with_transaction(|w| {
        tx_id = uuid::Uuid::new_v4();
        w.add_task(tx_id, &contract(), None, 100000, 0)
    })
    .unwrap();
    d.promote_ready().unwrap();
    let lease = d.claim("w1", 60).unwrap().expect("lease");

    let cfg = AgentLoopConfig {
        sandbox: sb_cfg(60),
        ..Default::default()
    };
    let sb = Sandbox::start(&cfg.sandbox, &short()).expect("start");
    let mut sink =
        PgAuditSink::connect(&dsn(), lease.task_id, "w1", lease.epoch).expect("audit connect");
    let llm = ScriptedLlm::new(vec![
        r#"{"tool":"run_shell","args":{"command":"echo one"}}"#,
        r#"{"tool":"run_shell","args":{"command":"echo two"}}"#,
        r#"{"done":true,"result":"ok"}"#,
    ]);
    agent_loop::run(
        &llm,
        &sb,
        &task_contract(),
        &Allowlist::default(),
        &cfg,
        &mut sink,
    )
    .expect("loop");

    let n = count_tool_calls(&dsn(), lease.task_id).expect("count");
    assert_eq!(n, 2, "в events должно быть 2 события tool_call");

    // Цепочка событий остаётся валидной.
    assert!(
        d.verify_chain(lease.task_id).unwrap(),
        "хэш-цепочка должна быть валидной"
    );
}

// 9. sandbox_cleaned_after_crash
#[test]
#[ignore = "требует живых Docker + PostgreSQL; см. докстринг файла"]
fn sandbox_cleaned_after_crash() {
    enable_shell_tools();
    let cfg = sb_cfg(30);
    let tag = short();
    let sb = Sandbox::start(&cfg, &tag).expect("start");
    let name = sb.name().to_string();
    // Имитация краха воркера: контейнер не остановлен.
    std::mem::forget(sb);
    assert!(docker_has(&name), "контейнер остался после 'краха'");

    let removed = cleanup_stale(&cfg, &[]).expect("cleanup");
    assert!(
        removed >= 1,
        "cleanup должен был удалить хотя бы один контейнер"
    );
    assert!(!docker_has(&name), "контейнер {name} должен быть удалён");
}

fn contract() -> swarm_core::TaskContract {
    serde_json::from_value(task_contract()).expect("contract")
}
