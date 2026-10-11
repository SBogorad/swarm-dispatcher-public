//! LLM-worker: CLI-цикл.
//!
//! Env:
//! - `WORKER_ID` (обязательно) — должен соответствовать роли `swarm_worker_<id>` (step 2b)
//! - `DATABASE_URL` (обязательно) — DSN воркера
//! - `LEASE_SEC` (по умолчанию 300)
//! - `ARTIFACTS_DIR` (по умолчанию /var/swarm/artifacts)
//! - `CRASH_AFTER_CLAIM` (0|1, по умолчанию 0)
//! - `CAPABILITIES` (JSON, по умолчанию {"ram_gb":16,"models":{"coder_class":2}})
//! - `AFFINITY_TAGS` (через запятую, по умолчанию пусто)
//! - `WORKER_MAX_TOKENS` (по умолчанию 4096)
//! - `WORKER_LLM_TIMEOUT_SEC` (по умолчанию 300)
//! - `LLM_*` — см. swarm-llm

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use swarm_core::SwarmDispatcher;
use swarm_llm::{from_env as llm_from_env, LlmClient};
use swarm_worker::{process_lease, Outcome, RetryPolicy, WorkerConfig};
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;

fn env_u64(name: &str, dflt: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(dflt)
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::from_default_env().add_directive("swarm_worker=info".parse().unwrap()),
        )
        .json()
        .init();

    let worker_id = std::env::var("WORKER_ID").expect("WORKER_ID required");
    let dsn = std::env::var("DATABASE_URL").expect("DATABASE_URL required");
    let lease_sec: i32 = env_u64("LEASE_SEC", 300) as i32;
    let crash: bool = std::env::var("CRASH_AFTER_CLAIM")
        .map(|s| s == "1")
        .unwrap_or(false);
    let capabilities: serde_json::Value = std::env::var("CAPABILITIES")
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| serde_json::json!({"ram_gb": 16, "models": {"coder_class": 2}}));
    let tags: Vec<String> = std::env::var("AFFINITY_TAGS")
        .ok()
        .map(|s| {
            s.split(',')
                .map(|x| x.trim().to_string())
                .filter(|x| !x.is_empty())
                .collect()
        })
        .unwrap_or_default();

    let cfg = WorkerConfig {
        artifacts_dir: PathBuf::from(
            std::env::var("ARTIFACTS_DIR").unwrap_or_else(|_| "/var/swarm/artifacts".into()),
        ),
        lease_sec,
        max_tokens: env_u64("WORKER_MAX_TOKENS", 4096) as u32,
        llm_timeout_sec: env_u64("WORKER_LLM_TIMEOUT_SEC", 300),
        retry: RetryPolicy::default(),
        work_sec: env_u64("WORK_SECONDS", 0),
    };

    let llm: Box<dyn LlmClient> = match llm_from_env() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: LLM config: {e}");
            std::process::exit(2);
        }
    };

    let shutdown = Arc::new(AtomicBool::new(false));
    let s = shutdown.clone();
    ctrlc::set_handler(move || {
        info!("received shutdown signal");
        s.store(true, Ordering::SeqCst);
    })
    .expect("install signal handler");

    let mut d = SwarmDispatcher::connect(&dsn).expect("connect to Postgres");

    loop {
        match d.register_worker(&worker_id, &capabilities, &tags) {
            Ok(()) => break,
            Err(e) => {
                warn!(error = %e, "register_worker failed, retrying");
                thread::sleep(Duration::from_secs(2));
            }
        }
    }
    info!(worker_id, "worker registered");

    // FIND-01 / Волна 5.1: явный след согласия в логах. Без WORKER_SHELL_TOOLS=1
    // sandbox-контейнеры запускать запрещено; фиксируем состояние громко, чтобы
    // включение было видно в аудите, а не было молчаливым дефолтом.
    if swarm_worker::shell_tools_enabled() {
        warn!(
            worker_id,
            "WORKER_SHELL_TOOLS=1: shell-tools ENABLED (sandbox execution allowed)"
        );
    } else {
        info!(worker_id, "shell-tools disabled (set WORKER_SHELL_TOOLS=1 to enable)");
    }

    let mut consecutive_errors = 0u32;
    while !shutdown.load(Ordering::SeqCst) {
        match d.claim(&worker_id, lease_sec) {
            Ok(Some(lease)) => {
                consecutive_errors = 0;
                if crash {
                    warn!(task_id = %lease.task_id, "CRASH_AFTER_CLAIM=1, exiting");
                    std::process::exit(1);
                }
                match process_lease(&mut d, &lease, llm.as_ref(), &cfg, Some(dsn.clone())) {
                    Ok(Outcome::Review { artifact_ref }) => {
                        info!(task_id = %lease.task_id, %artifact_ref, "task -> review")
                    }
                    Ok(Outcome::Blocked { reason, .. }) => {
                        warn!(task_id = %lease.task_id, %reason, "task -> blocked")
                    }
                    Ok(Outcome::LeaseLost) => {
                        warn!(task_id = %lease.task_id, "lease lost")
                    }
                    Err(e) => error!(error = %e, task_id = %lease.task_id, "task failed"),
                }
            }
            Ok(None) => {
                consecutive_errors = 0;
                thread::sleep(Duration::from_secs(2));
            }
            Err(e) => {
                consecutive_errors += 1;
                error!(error = %e, attempts = consecutive_errors, "claim failed");
                thread::sleep(Duration::from_secs((consecutive_errors as u64).min(30)));
            }
        }
    }
    info!("worker stopped");
}
