//! Ядро LLM-worker: обработка одной аренды.
//!
//! Поток (ТЗ v0.3 §1): claim → heartbeat (фон) → LLM → артефакт на диск →
//! save_artifact → complete. Никаких subprocess — это v0.4.

use crate::artifacts;
use crate::budget::{self, BudgetPolicy};
use crate::error::{Result, WorkerError};
use crate::prompt;
use crate::retry::{jitter_unit, RetryPolicy};
use crate::storage::ArtifactStore;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
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
    /// Issue #3: hard budget guard (глобальный лимит + auth circuit breaker).
    pub budget: BudgetPolicy,
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
            budget: BudgetPolicy::default(),
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

/// F5: состояние фонового heartbeat, видимое главному потоку.
/// Раньше потеря аренды/обрыв соединения только логировались внутри потока:
/// главный поток продолжал считать, что аренда жива, и тратил LLM-вызов впустую
/// (а при нестартовавшем потоке вообще терял аренду молча).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum LeaseHealth {
    Ok = 0,
    /// Postgres сообщил, что аренда потеряна (heartbeat вернул false).
    Lost = 1,
    /// Heartbeat-поток мёртв: не смог подключиться / не смог восстановиться.
    Dead = 2,
}

impl LeaseHealth {
    fn from_u8(v: u8) -> Self {
        match v {
            1 => LeaseHealth::Lost,
            2 => LeaseHealth::Dead,
            _ => LeaseHealth::Ok,
        }
    }
    fn store(self, a: &AtomicU8) {
        a.store(self as u8, Ordering::SeqCst);
    }
    fn load(a: &AtomicU8) -> Self {
        Self::from_u8(a.load(Ordering::SeqCst))
    }
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

    // Фоновый heartbeat (F5). health — канал fail-fast в главный поток.
    // started — поток подтвердил подключение (барьер против гонки:
    // иначе главный поток мог бы проверить health до того, как поток упадёт
    // при connect).
    let stop = Arc::new(AtomicBool::new(false));
    let health = Arc::new(AtomicU8::new(LeaseHealth::Ok as u8));
    let started = Arc::new(AtomicBool::new(false));
    let hb = heartbeat_conn.map(|dsn| {
        let stop = stop.clone();
        let health = health.clone();
        let started = started.clone();
        let task_id = lease.task_id;
        let epoch = lease.epoch;
        let worker_id = lease.worker_id.clone();
        let interval = Duration::from_secs((cfg.lease_sec / 3).max(1) as u64);
        let extend = cfg.lease_sec;
        thread::spawn(move || {
            // F5: нестартовавший поток = мёртвая аренда. Сигналим главному потоку,
            // что heartbeat не работает, чтобы он сделал fail-fast, а не молчал.
            let mut d = match SwarmDispatcher::connect(&dsn) {
                Ok(d) => d,
                Err(e) => {
                    error!(error = %e, "heartbeat: connect failed (fail-fast)");
                    LeaseHealth::Dead.store(&health);
                    return;
                }
            };
            started.store(true, Ordering::SeqCst);
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
                        // Постгрес говорит, что аренда потеряна: сигналим fail-fast
                        // и прекращаем бесполезные heartbeat-циклы.
                        warn!(task_id = %task_id, "lease lost, stopping heartbeat");
                        LeaseHealth::Lost.store(&health);
                        break;
                    }
                    Err(e) => {
                        // F5: транзиентный обрыв соединения — пробуем восстановиться.
                        // Раньше ошибка только логировалась, и аренда тихо истекала.
                        warn!(error = %e, "heartbeat failed, reconnecting");
                        match SwarmDispatcher::connect(&dsn) {
                            Ok(nd) => {
                                d = nd;
                            }
                            Err(e2) => {
                                error!(error = %e2, "heartbeat reconnect failed (fail-fast)");
                                LeaseHealth::Dead.store(&health);
                                break;
                            }
                        }
                    }
                }
            }
        })
    });

    if cfg.work_sec > 0 {
        thread::sleep(Duration::from_secs(cfg.work_sec));
    }

    // F5: барьер старта. Ждём, пока heartbeat-поток либо подтвердит подключение
    // (started), либо умрёт (Dead). Без барьера проверка health могла пройти
    // до того, как поток упал при connect — и воркер молча продолжал работу.
    if hb.is_some() {
        let deadline = Instant::now() + Duration::from_secs(2);
        while !started.load(Ordering::SeqCst)
            && LeaseHealth::load(&health) == LeaseHealth::Ok
            && Instant::now() < deadline
        {
            thread::sleep(Duration::from_millis(5));
        }
    }

    // F5: fail-fast — если heartbeat уже мёртв/потерян, не тратим LLM-вызов.
    match LeaseHealth::load(&health) {
        LeaseHealth::Ok => {}
        LeaseHealth::Lost => {
            warn!(task_id = %lease.task_id, "lease lost before LLM call; aborting");
            drop(hb);
            return Ok(Outcome::LeaseLost);
        }
        LeaseHealth::Dead => {
            warn!(task_id = %lease.task_id, "heartbeat dead before LLM call; aborting");
            drop(hb);
            return Ok(Outcome::LeaseLost);
        }
    }

    // Issue #3: hard budget guard. Проверяем ПЕРЕД вызовом LLM:
    //  * токен-бюджет задачи (из аренды),
    //  * глобальный денежный лимит (из cost ledger, Issue #2).
    // Fail-closed: если ledger недоступен, не тратим деньги — блокируем.
    let spent_usd = match d.total_cost_usd() {
        Ok(v) => v,
        Err(e) => {
            warn!(task_id = %lease.task_id, error = %e, "budget guard: ledger unavailable -> fail-closed");
            let reason = format!("budget_check_failed: {e}");
            let ref_ = fail_artifact(cfg, lease, &reason)?;
            d.save_artifact(lease, &ref_, &path_str(cfg, &ref_), Some("text/plain"), None)?;
            let st = d.fail_with_state(lease, Some(&ref_), &reason, "failed")?;
            drop(hb);
            return Ok(blocked_or_review(st, ref_, &reason));
        }
    };
    if let Err(block) = budget::check_before_call(
        &cfg.budget,
        spent_usd,
        lease.spent_tokens,
        lease.budget_cap,
    ) {
        let reason = block.reason();
        warn!(task_id = %lease.task_id, reason = %reason, spent_usd, "budget guard: blocking before LLM call");
        let ref_ = fail_artifact(cfg, lease, &reason)?;
        d.save_artifact(lease, &ref_, &path_str(cfg, &ref_), Some("text/plain"), None)?;
        let st = d.fail_with_state(lease, Some(&ref_), &reason, block.state())?;
        drop(hb);
        return Ok(blocked_or_review(st, ref_, &reason));
    }

    // Вызов LLM с ретраями.
    let result = call_llm_with_retries(llm, &contract, cfg);

    // F5: повторная проверка после LLM — если аренду потеряли во время работы,
    // не пытаемся сохранять артефакт/завершать (это всё равно LS001).
    if LeaseHealth::load(&health) != LeaseHealth::Ok {
        warn!(task_id = %lease.task_id, "lease lost during LLM call; discarding result");
        drop(hb);
        return Ok(Outcome::LeaseLost);
    }

    let heartbeat = HeartbeatGuard {
        stop: stop.clone(),
        handle: hb,
    };

    let outcome = match result {
        Ok(resp) => {
            // Issue #2: записываем метрики LLM-вызова в БД
            let started_at = chrono::Utc::now() - chrono::Duration::milliseconds(1000); // примерно
            let completed_at = chrono::Utc::now();
            let (prompt_cost, completion_cost, _total) = crate::llm_cost::calculate_cost(
                &resp.model,
                resp.tokens_in,
                resp.tokens_out,
            );
            match d.record_llm_call(
                lease.task_id,
                &lease.worker_id,
                lease.epoch,
                &resp.model,
                "relay.fast",
                resp.tokens_in as i64,
                resp.tokens_out as i64,
                0, // cached_tokens — пока не поддерживается в LlmResponse
                prompt_cost,
                completion_cost,
                &started_at.to_rfc3339(),
                &completed_at.to_rfc3339(),
                1000, // duration_ms — примерно, можно уточнить через Instant
                true,
                None,
                None,
            ) {
                Ok(_) => info!(task_id = %lease.task_id, model = %resp.model, tokens_in = resp.tokens_in, tokens_out = resp.tokens_out, "LLM call recorded to DB"),
                Err(e) => warn!(task_id = %lease.task_id, error = ?e, "Failed to record LLM call metrics"),
            } // не фейлим задачу, если запись метрики упала

            let (ref_, path, size) =
                artifacts::write_artifact(&cfg.artifacts_dir, resp.text.as_bytes())?;
            let storage_path = match ArtifactStore::from_env()? {
                Some(store) => store.put_verified(&ref_, &path, "text/plain")?,
                None => local_storage_uri(&path),
            };
            d.save_artifact(
                lease,
                &ref_,
                &storage_path,
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
            // Issue #2: записываем метрики неудачного LLM-вызова
            let started_at = chrono::Utc::now() - chrono::Duration::milliseconds(500);
            let completed_at = chrono::Utc::now();
            let error_code = match &e {
                WorkerError::Llm(swarm_llm::LlmError::Http { status, .. }) => Some(format!("HTTP {}", status)),
                WorkerError::Llm(swarm_llm::LlmError::Transport(_)) => Some("transport".to_string()),
                WorkerError::Llm(swarm_llm::LlmError::InvalidJson(_)) => Some("invalid_json".to_string()),
                _ => None,
            };
            match d.record_llm_call(
                lease.task_id,
                &lease.worker_id,
                lease.epoch,
                "unknown", // модель неизвестна при ошибке до ответа
                "relay.fast",
                0,
                0,
                0,
                0.0,
                0.0,
                &started_at.to_rfc3339(),
                &completed_at.to_rfc3339(),
                500,
                false,
                error_code.as_deref(),
                Some(&format!("{}", e)),
            ) {
                Ok(_) => info!(task_id = %lease.task_id, "Failed LLM call recorded to DB"),
                Err(e) => warn!(task_id = %lease.task_id, error = ?e, "Failed to record failed LLM call metrics"),
            }

            // Не-транзиентная или исчерпанная ошибка LLM → fail.
            // Issue #3: 401/403 — это сбой авторизации, а не бюджета. Повторять
            // бессмысленно (инцидент argo-memory: 4 часа пустых попыток),
            // поэтому задача уходит в 'failed' и требует ротации ключа.
            let status = match &e {
                WorkerError::Llm(swarm_llm::LlmError::Http { status, .. }) => Some(*status),
                _ => None,
            };
            let auth = budget::is_auth_failure(status);
            let reason = if auth {
                format!("auth_failed: LLM provider rejected credentials (HTTP {})", status.unwrap_or(0))
            } else {
                format!("llm_error: {e}")
            };
            let target_state = if auth { "failed" } else { "blocked" };
            warn!(task_id = %lease.task_id, %reason, state = target_state, "llm failed");
            let ref_ = fail_artifact(cfg, lease, &reason)?;
            d.save_artifact(
                lease,
                &ref_,
                &path_str(cfg, &ref_),
                Some("text/plain"),
                None,
            )?;
            // F4: честный отказ — без подделки spent_tokens. Причина попадает
            // в block_reason и в событие completed{failed:true}.
            let st = d.fail_with_state(lease, Some(&ref_), &reason, target_state)?;
            blocked_or_review(st, ref_, &reason)
        }
    };

    drop(heartbeat);
    Ok(outcome)
}

fn blocked_or_review(state: String, artifact_ref: String, reason: &str) -> Outcome {
    match state.as_str() {
        "blocked" | "budget_exhausted" | "failed" => Outcome::Blocked {
            artifact_ref,
            reason: reason.to_string(),
        },
        _ => Outcome::Review { artifact_ref },
    }
}

fn path_str(cfg: &WorkerConfig, reference: &str) -> String {
    local_storage_uri(&artifacts::artifact_path(&cfg.artifacts_dir, reference))
}

/// F1: путь артефакта, видимый потребителю.
///
/// Раньше записывался container-локальный путь (например,
/// `/var/swarm/artifacts/<ref>`), недостижимый из другого контейнера. Теперь:
///   * если задан `SWARM_ARTIFACTS_VOLUME` — возвращаем URI
///     `volume://<name>/<relpath>`, который потребитель разрешает в тот же
///     общий volume;
///   * иначе — абсолютный путь как есть (совместимость: локальный режим/
///     тесты без volume).
///
/// S3-режим (ArtifactStore) даёт собственный URI и сюда не попадает.
fn local_storage_uri(path: &std::path::Path) -> String {
    let path_str = path.to_string_lossy();
    match std::env::var("SWARM_ARTIFACTS_VOLUME") {
        Ok(vol) if !vol.trim().is_empty() => {
            // Отрезаем точку монтирования, оставляя относительный путь в volume.
            let mount = std::env::var("ARTIFACTS_DIR")
                .unwrap_or_else(|_| "/var/swarm/artifacts".to_string());
            let rel = path_str
                .strip_prefix(&mount)
                .map(|r| r.trim_start_matches('/'))
                .unwrap_or(path_str.as_ref());
            format!("volume://{}/{}", vol.trim(), rel)
        }
        _ => path_str.into_owned(),
    }
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
