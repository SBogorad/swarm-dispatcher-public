//! Ядро LLM-worker: обработка одной аренды.
//!
//! Поток (ТЗ v0.3 §1): claim → heartbeat (фон) → LLM → артефакт на диск →
//! save_artifact → complete. Никаких subprocess — это v0.4.

use crate::artifacts;
use crate::error::{Result, WorkerError};
use crate::prompt;
use crate::retry::{jitter_unit, RetryPolicy};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use swarm_core::{Lease, SwarmDispatcher};
use swarm_llm::{LlmClient, LlmError, LlmRequest};
use tracing::{error, info, warn};

/// Конфигурация обработки одной задачи.
pub struct WorkerConfig {
    pub artifacts_dir: PathBuf,
    pub lease_sec: i32,
    pub max_tokens: u32,
    pub llm_timeout_sec: u64,
    pub retry: RetryPolicy,
    /// Для тестов: сколько ждать перед вызовом LLM (имитация работы).
    pub work_sec: u64,
}

impl Default for WorkerConfig {
    fn default() -> Self {
        Self {
            artifacts_dir: PathBuf::from("/var/swarm/artifacts"),
            lease_sec: 300,
            max_tokens: 4096,
            llm_timeout_sec: 300,
            retry: RetryPolicy::default(),
            work_sec: 0,
        }
    }
}

/// Результат обработки задачи.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Задача сдана в review.
    Review { artifact_ref: String },
    /// Задача заблокирована (бюджет/ошибка LLM).
    Blocked {
        artifact_ref: String,
        reason: String,
    },
    /// Аренда потеряна в процессе (watchdog вернул задачу).
    LeaseLost,
}

/// Обработать одну аренду. `llm` — клиент (в тестах — мок).
///
/// `heartbeat_conn` — отдельное соединение для фонового heartbeat (может быть
/// `None` в тестах, где heartbeat не нужен).
struct HeartbeatGuard {
    stop: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
}

impl Drop for HeartbeatGuard {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

pub fn process_lease(
    d: &mut SwarmDispatcher,
    lease: &Lease,
    llm: &dyn LlmClient,
    cfg: &WorkerConfig,
    heartbeat_conn: Option<String>,
) -> Result<Outcome> {
    let contract = lease.contract.clone();
    info!(task_id = %lease.task_id, epoch = lease.epoch, "start work");

    // Бюджет: если уже исчерпан — не зовём LLM, сдаём с превышением.
    if lease.spent_tokens >= lease.budget_cap {
        warn!(task_id = %lease.task_id, spent = lease.spent_tokens, cap = lease.budget_cap, "budget already exhausted");
        let ref_ = fail_artifact(cfg, lease, "budget_exhausted")?;
        d.save_artifact(
            lease,
            &ref_,
            &path_str(cfg, &ref_),
            Some("text/plain"),
            None,
        )?;
        let st = d.complete(lease, &ref_, 0)?;
        return Ok(blocked_or_review(st, ref_, "budget_exhausted"));
    }

    // Фоновый heartbeat.
    let stop = Arc::new(AtomicBool::new(false));
    let hb = heartbeat_conn.map(|dsn| {
        let stop = stop.clone();
        let task_id = lease.task_id;
        let epoch = lease.epoch;
        let worker_id = lease.worker_id.clone();
        let interval = Duration::from_secs((cfg.lease_sec / 3).max(1) as u64);
        let extend = cfg.lease_sec;
        thread::spawn(move || {
            let mut d = match SwarmDispatcher::connect(&dsn) {
                Ok(d) => d,
                Err(e) => {
                    error!(error = %e, "heartbeat: connect failed");
                    return;
                }
            };
            while !stop.load(Ordering::SeqCst) {
                thread::sleep(interval);
                if stop.load(Ordering::SeqCst) {
                    break;
                }
                let l = Lease {
                    task_id,
                    contract: serde_json::Value::Null,
                    epoch,
                    budget_cap: 0,
                    spent_tokens: 0,
                    worker_id: worker_id.clone(),
                };
                match d.heartbeat(&l, extend) {
                    Ok(true) => {}
                    Ok(false) => {
                        warn!(task_id = %task_id, "lease lost, stopping heartbeat");
                        break;
                    }
                    Err(e) => error!(error = %e, "heartbeat failed"),
                }
            }
        })
    });

    if cfg.work_sec > 0 {
        thread::sleep(Duration::from_secs(cfg.work_sec));
    }

    // Вызов LLM с ретраями.
    let result = call_llm_with_retries(llm, &contract, cfg);

    let heartbeat = HeartbeatGuard {
        stop: stop.clone(),
        handle: hb,
    };

    let outcome = match result {
        Ok(resp) => {
            let (ref_, path, size) =
                artifacts::write_artifact(&cfg.artifacts_dir, resp.text.as_bytes())?;
            d.save_artifact(
                lease,
                &ref_,
                &path.to_string_lossy(),
                Some("text/plain"),
                Some(size as i64),
            )?;
            let tokens = resp.tokens_in as i64 + resp.tokens_out as i64;
            let st = d.complete(lease, &ref_, tokens)?;
            info!(task_id = %lease.task_id, new_state = %st, tokens, "completed");
            if st == "blocked" {
                blocked_or_review(st, ref_, "budget_exhausted")
            } else {
                Outcome::Review { artifact_ref: ref_ }
            }
        }
        Err(e) if is_lease_lost(&e) => Outcome::LeaseLost,
        Err(e) => {
            // Не-транзиентная или исчерпанная ошибка LLM → fail через blocked.
            let reason = format!("llm_error: {e}");
            warn!(task_id = %lease.task_id, %reason, "llm failed -> blocked");
            let ref_ = fail_artifact(cfg, lease, &reason)?;
            d.save_artifact(
                lease,
                &ref_,
                &path_str(cfg, &ref_),
                Some("text/plain"),
                None,
            )?;
            // Токены выставляем так, чтобы SQL перевёл задачу в blocked.
            let overspend = (lease.budget_cap - lease.spent_tokens).max(0) + 1;
            let st = d.complete(lease, &ref_, overspend)?;
            blocked_or_review(st, ref_, &reason)
        }
    };

    drop(heartbeat);
    Ok(outcome)
}

fn blocked_or_review(state: String, artifact_ref: String, reason: &str) -> Outcome {
    if state == "blocked" {
        Outcome::Blocked {
            artifact_ref,
            reason: reason.to_string(),
        }
    } else {
        Outcome::Review { artifact_ref }
    }
}

fn path_str(cfg: &WorkerConfig, reference: &str) -> String {
    artifacts::artifact_path(&cfg.artifacts_dir, reference)
        .to_string_lossy()
        .into_owned()
}

/// Артефакт-«ошибка»: сохраняем причину как содержимое, чтобы complete_task
/// нашёл artifact по ref (иначе AR001).
fn fail_artifact(cfg: &WorkerConfig, lease: &Lease, reason: &str) -> Result<String> {
    let body = format!(
        "{{\"error\":\"{}\",\"task_id\":\"{}\",\"epoch\":{}}}",
        reason.replace('"', "'"),
        lease.task_id,
        lease.epoch
    );
    let (ref_, _path, _size) = artifacts::write_artifact(&cfg.artifacts_dir, body.as_bytes())?;
    Ok(ref_)
}

fn is_lease_lost(e: &WorkerError) -> bool {
    matches!(
        e,
        WorkerError::Database(swarm_core::DispatcherError::LeaseLost)
    )
}

/// Вызов LLM с ретраями по политике.
fn call_llm_with_retries(
    llm: &dyn LlmClient,
    contract: &serde_json::Value,
    cfg: &WorkerConfig,
) -> Result<swarm_llm::LlmResponse> {
    let sys = prompt::system_prompt();
    let user = prompt::user_prompt(contract);
    let req = LlmRequest {
        system: sys,
        user,
        max_tokens: Some(cfg.max_tokens),
        temperature: None,
    };

    let mut attempt = 0u32;
    loop {
        match llm.complete(&req) {
            Ok(r) => return Ok(r),
            Err(e) => {
                let retry = cfg.retry.should_retry(&e) && attempt + 1 < cfg.retry.max_attempts;
                if !retry {
                    return Err(WorkerError::Llm(e));
                }
                let delay = cfg.retry.delay_ms(attempt, jitter_unit());
                warn!(attempt, delay_ms = delay, error = %e, "llm transient error, retrying");
                thread::sleep(Duration::from_millis(delay));
                attempt += 1;
            }
        }
    }
}

/// Хелпер для тестов: собрать клиент-мок из файла.
pub fn mock_llm_from_file(path: &str) -> Result<Box<dyn LlmClient>> {
    let cfg = swarm_llm::LlmConfig {
        provider: "mock".into(),
        api_key: None,
        model: "mock".into(),
        base_url: None,
        max_retries: 0,
        max_tokens: 4096,
        mock_file: Some(path.to_string()),
        timeout_sec: 5,
    };
    let c = swarm_llm::HttpLlmClient::new(cfg).map_err(WorkerError::Llm)?;
    Ok(Box::new(c))
}

/// Заглушка для проверки типа `LlmError` в тестах.
#[allow(dead_code)]
fn _assert_llm_error_is_send_sync(e: LlmError) -> LlmError {
    e
}
