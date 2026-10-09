//! Демонстрация Agent Loop с реальным LLM и реальным Docker (ТЗ v0.4 §8.3, §11.9).
//!
//! Прогоняет полный путь: task → claim → Agent Loop (LLM делает ≥2 tool calls
//! в sandbox) → артефакт на диск → complete → review → done.
//!
//! Запуск:
//!   DATABASE_URL="host=127.0.0.1 port=55440 user=swarm password=pw dbname=v0sandbox" \
//!   LLM_PROVIDER=openai-compatible LLM_BASE_URL=https://relay.fast/api/v1 \
//!   LLM_API_KEY=... LLM_MODEL=... \
//!   ARTIFACTS_DIR=/tmp/demo_artifacts \
//!   cargo run -p swarm-worker --example agent_loop_demo --locked

use std::path::PathBuf;

use swarm_core::{SwarmDispatcher, TaskContract};
use swarm_llm::from_env as llm_from_env;
use swarm_worker::agent_loop::{self, AgentLoopConfig};
use swarm_worker::allowlist::Allowlist;
use swarm_worker::audit::{count_tool_calls, PgAuditSink};
use swarm_worker::sandbox::Sandbox;
use swarm_worker::WorkerError;

fn main() {
    let dsn = std::env::var("DATABASE_URL").expect("DATABASE_URL");
    let artifacts_dir =
        PathBuf::from(std::env::var("ARTIFACTS_DIR").unwrap_or_else(|_| "/tmp/demo_artifacts".into()));
    std::fs::create_dir_all(&artifacts_dir).expect("artifacts dir");

    let mut d = SwarmDispatcher::connect(&dsn).expect("connect");
    let worker = "w1";
    d.register_worker(worker, &serde_json::json!({"ram_gb": 16}), &[]).expect("register");

    // 1. Задача: реальная мини-задача, решаемая shell-инструментами.
    let contract: TaskContract = serde_json::from_value(serde_json::json!({
        "kind": "code.generate",
        "description": "Создай библиотечный крейт demo (cargo init --lib --name demo), \
                        добавь в src/lib.rs функцию add(a,b), запусти cargo test и покажи результат.",
        "checks": [{"type": "cmd", "run": "true"}]
    }))
    .expect("contract");

    let task_id = uuid::Uuid::new_v4();
    d.with_transaction(|w| w.add_task(task_id, &contract, None, 1_000_000, 0))
        .expect("add_task");
    d.promote_ready().expect("promote");

    // 2. Аренда.
    let lease = d.claim(worker, 900).expect("claim").expect("lease");
    println!("lease: task={} epoch={}", lease.task_id, lease.epoch);

    // 3. Sandbox.
    let cfg = AgentLoopConfig {
        max_iterations: 12,
        max_tokens: 2048,
        ..Default::default()
    };
    let sb = Sandbox::start(&cfg.sandbox, &lease.task_id.to_string()[..12]).expect("sandbox start");
    println!("sandbox: {}", sb.name());

    // 4. Реальный LLM.
    let llm = llm_from_env().expect("llm from env");
    let mut sink =
        PgAuditSink::connect(&dsn, lease.task_id, worker, lease.epoch).expect("audit sink");

    let res = agent_loop::run(
        llm.as_ref(),
        &sb,
        &serde_json::to_value(&contract).unwrap(),
        &Allowlist::default(),
        &cfg,
        &mut sink,
    );

    let mut sb = sb;
    let _ = sb.stop();

    match res {
        Ok(r) => {
            println!("\n=== AGENT LOOP ===");
            println!("итераций: {}", r.iterations);
            println!("tool calls: {}", r.tool_calls);
            println!("финальный ответ:\n{}", r.final_text);

            let (reference, path, size) = swarm_worker::artifacts::write_artifact(
                &artifacts_dir,
                r.final_text.as_bytes(),
            )
            .expect("write artifact");
            d.save_artifact(&lease, &reference, &path.to_string_lossy(), Some("text/plain"), Some(size as i64))
                .expect("save_artifact");
            let st = d.complete(&lease, &reference, 0).expect("complete");
            println!("\nartifact: {reference} ({size} B) -> {path:?}");
            println!("state after complete: {st}");

            let n = count_tool_calls(&dsn, lease.task_id).unwrap_or(-1);
            println!("событий tool_call в журнале: {n}");
            println!("verify_chain: {}", d.verify_chain(lease.task_id).unwrap_or(false));

            assert!(r.tool_calls >= 2, "ожидалось ≥2 tool call, получено {}", r.tool_calls);
            println!("\nOK: agent loop выполнил ≥2 tool call с реальным LLM");
        }
        Err(e) => {
            eprintln!("AGENT LOOP FAILED: {e}");
            if let WorkerError::MaxIterations { max } = e {
                eprintln!("(лимит итераций {max} исчерпан)");
            }
            std::process::exit(1);
        }
    }
}
