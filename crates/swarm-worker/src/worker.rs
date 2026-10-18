//! Ядро LLM-worker: обработка одной аренды.
//!
//! Поток (ТЗ v0.3 §1): claim → heartbeat (фон) → LLM → артефакт на диск →
//! save_artifact → complete. Никаких subprocess — это v0.4.

use crate::artifacts;
use crate::budget::{self, BudgetPolicy, RunLimits};
use crate::error::{Result, WorkerError};
use crate::model_selector::ModelSelector;
use crate::prompt;
use crate::retry::{jitter_unit, RetryPolicy};
use crate::storage::ArtifactStore;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU8, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use swarm_core::{Lease, SwarmDispatcher};
use swarm_llm::{LlmClient, LlmError, LlmRequest};
use tracing::{debug, error, info, warn};

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
    /// Issue #4: жёсткие пределы прохода (max_llm_calls / max_duration_sec).
    pub run_limits: RunLimits,
    /// Issue #4: счётчик реально сделанных LLM-вызовов (включая ретраи).
    /// Разделяется с главным циклом (`main.rs`), который останавливает проход.
    pub llm_calls: Option<Arc<AtomicU32>>,
    /// Issue #7/#10: выбор модели по измеренной цене успеха. `None` — модель
    /// берётся из конфига LLM-клиента (поведение по умолчанию не меняется).
    pub model_selector: Option<Arc<ModelSelector>>,
    /// Read-only исходники репозитория для промпта (`SWARM_REPO_DIR`).
    ///
    /// `None` — репозиторий не смонтирован: поведение прежнее (воркер видит
    /// только контракт). Живой урок `agent_memory#1`: без этого задача «поправь
    /// файл X» уходила в `blocked (missing_input)`.
    pub repo: Option<crate::repo_context::RepoContext>,
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
            run_limits: RunLimits::default(),
            llm_calls: None,
            model_selector: None,
            repo: None,
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

/// F9: сон, прерываемый по флагу `stop`. Возвращает `true`, если сон прерван.
///
/// Шаг 500 мс: `HeartbeatGuard::drop` ставит флаг и делает `join`, поэтому
/// завершение задачи не ждёт полный интервал heartbeat (был до `lease_sec/3`).
/// Тот же приём применён в `swarm-watchdog/src/main.rs`.
fn sleep_interruptible(stop: &AtomicBool, total: Duration) -> bool {
    let step = Duration::from_millis(500);
    let start = Instant::now();
    while start.elapsed() < total {
        if stop.load(Ordering::SeqCst) {
            return true;
        }
        let remaining = total.saturating_sub(start.elapsed());
        thread::sleep(step.min(remaining));
    }
    stop.load(Ordering::SeqCst)
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
                // F9: прерываемый сон. Раньше был `thread::sleep(interval)` —
                // после завершения задачи `HeartbeatGuard::drop` делал `join` и
                // ждал до `lease_sec/3` (при 300 с — до 100 с) впустую.
                // Шаги по 500 мс дают остановку за ≤500 мс (тот же приём, что в
                // swarm-watchdog/src/main.rs).
                if sleep_interruptible(&stop, interval) {
                    break;
                }
                let l = Lease {
                    task_id,
                    contract: serde_json::Value::Null,
                    epoch,
                    budget_cap: 0,
                    spent_tokens: 0,
                    worker_id: worker_id.clone(),
                    // Heartbeat аренду не создаёт — поля обратной связи не нужны.
                    rework: 0,
                    last_review: None,
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
            d.save_artifact(
                lease,
                &ref_,
                &path_str(cfg, &ref_),
                Some("text/plain"),
                None,
            )?;
            let st = d.fail_with_state(lease, Some(&ref_), &reason, "failed")?;
            drop(hb);
            return Ok(blocked_or_review(st, ref_, &reason));
        }
    };
    // F8: свежий бюджет задачи (локальный + по дереву). Снимок из аренды
    // устаревает после любого расхода, в т.ч. сделанного по тому же дереву.
    // Fail-closed: недоступен бюджет — не тратим деньги.
    let (fresh_spent, fresh_cap, root_spent, root_cap) = match d.task_budget_state(lease.task_id) {
        Ok(v) => v,
        Err(e) => {
            warn!(task_id = %lease.task_id, error = %e, "budget guard: task budget unavailable -> fail-closed");
            let reason = format!("budget_check_failed: {e}");
            let ref_ = fail_artifact(cfg, lease, &reason)?;
            d.save_artifact(
                lease,
                &ref_,
                &path_str(cfg, &ref_),
                Some("text/plain"),
                None,
            )?;
            let st = d.fail_with_state(lease, Some(&ref_), &reason, "failed")?;
            drop(hb);
            return Ok(blocked_or_review(st, ref_, &reason));
        }
    };
    if let Err(block) = budget::check_before_call(
        &cfg.budget,
        spent_usd,
        fresh_spent,
        fresh_cap,
        root_spent,
        root_cap,
    ) {
        let reason = block.reason();
        warn!(task_id = %lease.task_id, reason = %reason, spent_usd, "budget guard: blocking before LLM call");
        let ref_ = fail_artifact(cfg, lease, &reason)?;
        d.save_artifact(
            lease,
            &ref_,
            &path_str(cfg, &ref_),
            Some("text/plain"),
            None,
        )?;
        let st = d.fail_with_state(lease, Some(&ref_), &reason, block.state())?;
        drop(hb);
        return Ok(blocked_or_review(st, ref_, &reason));
    }

    // Вызов LLM с ретраями.
    //
    // Issue #2: время измеряется реально (`Instant` вокруг вызова), а не
    // фабрикуется. Раньше в ledger писалось `duration_ms = 1000` и
    // `started_at = now - 1s` («примерно») — это делало метрики недостоверными.
    //
    // Итерация 3 (DAG handoff): вход от родительских задач. Запрашиваем
    // артефакты зависимостей ровно держателем живой аренды. Ошибка RPC не
    // фатальна: контракт без handoff — деградация, а не срыв задачи (ветка
    // `FailedPrecondition` в основной системе не деградирует молча, но здесь
    // это отдельный SELECT, его потеря не должна ронять воркер).
    let parents = match d.task_parent_artifacts(lease.task_id, &lease.worker_id, lease.epoch) {
        Ok(p) => p,
        Err(e) => {
            warn!(task_id = %lease.task_id, error = %e, "handoff: parent artifacts unavailable");
            Vec::new()
        }
    };
    if !parents.is_empty() {
        info!(task_id = %lease.task_id, count = parents.len(), "handoff: parent artifacts loaded");
    }
    // Итерация 3 (Вопрос 3 аудита Gemini Argon): метаданных недостаточно роли
    // `code.verify` — ей нужен проверяемый текст. Читаем превью родительских
    // артефактов с общего тома best-effort: нет файла или не сошёлся sha256 →
    // `None`, и секция отдаёт только метаданные (как раньше).
    // Pstack `reflect`/`correct`: память роя между графами. Контейнер воркера
    // эфемерен (`--read-only`, tmpfs) и своего `MEMORY.md` не имеет, поэтому
    // единственный носитель опыта — `agent_memory` в PostgreSQL. Best-effort:
    // потеря памяти деградирует качество, но не должна ронять задачу.
    let lessons = match d.task_recall_memory(lease.task_id, &lease.worker_id, lease.epoch) {
        Ok(l) => l,
        Err(e) => {
            warn!(task_id = %lease.task_id, error = %e, "memory: lessons unavailable");
            Vec::new()
        }
    };
    if !lessons.is_empty() {
        info!(task_id = %lease.task_id, count = lessons.len(), "memory: lessons recalled");
    }
    // Исходники, упомянутые в постановке: без них задача «поправь файл X»
    // уходила в `blocked (missing_input)` — воркер не видел ни файла, ни
    // возможности построить корректный unified diff (урок `agent_memory#1`).
    let repo_files = match cfg.repo.as_ref() {
        Some(repo) => {
            let files = repo.collect(&contract);
            if !files.is_empty() {
                info!(
                    task_id = %lease.task_id,
                    count = files.len(),
                    "repo: source files attached"
                );
            }
            files
        }
        None => Vec::new(),
    };
    let previews = load_parent_previews(cfg, &parents);
    let call_started_at = chrono::Utc::now();
    let call_started = Instant::now();
    let result = call_llm_with_retries(
        llm,
        cfg,
        LlmPromptInput {
            contract: &contract,
            rework: lease.rework,
            last_review: lease.last_review.as_ref(),
            parents: &parents,
            previews: &previews,
            lessons: &lessons,
            repo: &repo_files,
        },
    );
    let call_duration_ms = call_started.elapsed().as_millis() as i64;
    let call_completed_at = chrono::Utc::now();

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
            // Issue #2: записываем метрики LLM-вызова в БД (реальное время вызова).
            let (prompt_cost, completion_cost, _total) =
                crate::llm_cost::calculate_cost(&resp.model, resp.tokens_in, resp.tokens_out);
            // Issue #2: неизвестная модель считается по консервативной оценке —
            // это сигнал, а не молчаливая подмена цены.
            if !crate::llm_cost::is_known_model(&resp.model) {
                warn!(
                    task_id = %lease.task_id,
                    model = %resp.model,
                    "unknown model price: cost is an estimate, add it to llm_cost.rs"
                );
            }
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
                &call_started_at.to_rfc3339(),
                &call_completed_at.to_rfc3339(),
                call_duration_ms,
                true,
                None,
                None,
                // Issue #2: успешный вызов отвечал 200.
                Some(200),
            ) {
                Ok(_) => {
                    info!(task_id = %lease.task_id, model = %resp.model, tokens_in = resp.tokens_in, tokens_out = resp.tokens_out, "LLM call recorded to DB")
                }
                Err(e) => {
                    warn!(task_id = %lease.task_id, error = ?e, "Failed to record LLM call metrics")
                }
            } // не фейлим задачу, если запись метрики упала

            // Issue #5: пустой ответ — не полезный артефакт.
            //
            // Раньше «пустая работа» (HTTP 200, но пустой/пробельный текст)
            // писалась как артефакт-результат и уходила в `review`, т.е.
            // засчитывалась как выполненная задача. Теперь это честный отказ:
            // состояние `blocked`, `block_reason = empty_response`, а сам
            // артефакт — маркер ошибки (не результат). Расход уже записан в
            // `llm_calls` выше, поэтому бюджет остаётся честным.
            if resp.text.trim().is_empty() {
                let reason = "empty_response: LLM вернул пустой ответ";
                warn!(task_id = %lease.task_id, %reason, "llm returned empty response");
                let ref_ = fail_artifact(cfg, lease, reason)?;
                d.save_artifact(
                    lease,
                    &ref_,
                    &path_str(cfg, &ref_),
                    Some("text/plain"),
                    None,
                )?;
                let st = d.fail_with_state(lease, Some(&ref_), reason, "blocked")?;
                return Ok(blocked_or_review(st, ref_, reason));
            }

            // F-NEW-C: структурный отказ модели `{"error": "missing_input"}`
            // — это дефект КОНТРАКТА, а не исполнителя. Реворк не может его
            // исправить (контракт от повтора не меняется), поэтому раньше задача
            // крутила max_rework бессмысленных попыток, сжигая токены.
            // Переводим сразу в `blocked` с явной причиной — без реворков.
            if let Some(spec_err) = contract_error_marker(&resp.text) {
                let reason = format!("contract_error: {spec_err}");
                warn!(task_id = %lease.task_id, %reason, "LLM вернул отказ по контракту — без реворков");
                let ref_ = fail_artifact(cfg, lease, &reason)?;
                d.save_artifact(
                    lease,
                    &ref_,
                    &path_str(cfg, &ref_),
                    Some("text/plain"),
                    None,
                )?;
                let st = d.fail_with_state(lease, Some(&ref_), &reason, "blocked")?;
                return Ok(blocked_or_review(st, ref_, &reason));
            }

            // Счётчики хунков модель считает «на глаз»: объявленное в
            // `@@ -l,s +l,s @@` расходится с телом, и ревьюер отклоняет
            // верный патч. Пересчитываем до записи — артефакт уходит на
            // ревью уже согласованным.
            let artifact_text = crate::diff_repair::recount_hunks(&resp.text);
            let (ref_, path, size) =
                artifacts::write_artifact(&cfg.artifacts_dir, artifact_text.as_bytes())?;
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
            // Issue #20: complete_task при перерасходе возвращает `budget_exhausted`
            // (а не `blocked`). blocked_or_review сам различает review/блокировку,
            // поэтому не полагаемся на конкретную строку состояния.
            blocked_or_review(st, ref_, "budget_exhausted")
        }
        Err(e) if is_lease_lost(&e) => Outcome::LeaseLost,
        Err(e) => {
            // Issue #2: записываем метрики неудачного LLM-вызова (реальное время вызова).
            let error_code = match &e {
                WorkerError::Llm(swarm_llm::LlmError::Http { status, .. }) => {
                    Some(format!("HTTP {}", status))
                }
                WorkerError::Llm(swarm_llm::LlmError::Transport(_)) => {
                    Some("transport".to_string())
                }
                WorkerError::Llm(swarm_llm::LlmError::InvalidJson(_)) => {
                    Some("invalid_json".to_string())
                }
                _ => None,
            };
            // Issue #2: HTTP-код ошибки — явной колонкой. Для transport/timeout/
            // parse HTTP-ответа нет — честный NULL, а не выдуманный 0.
            let http_status: Option<i32> = match &e {
                WorkerError::Llm(swarm_llm::LlmError::Http { status, .. }) => Some(*status as i32),
                _ => None,
            };
            // FIND-CODE-03: имя модели для аудита. Раньше здесь был хардкод
            // "unknown", из-за чего `llm_calls.model` и витрины llm_costs_daily /
            // llm_model_quality теряли, какая ИМЕННО модель даёт ошибки. Теперь
            // берём выбранную селектором модель либо модель по умолчанию клиента.
            let attempted_model = select_model(cfg, &contract, lease.rework)
                .unwrap_or_else(|| llm.default_model().to_string());
            match d.record_llm_call(
                lease.task_id,
                &lease.worker_id,
                lease.epoch,
                &attempted_model,
                "relay.fast",
                0,
                0,
                0,
                0.0,
                0.0,
                &call_started_at.to_rfc3339(),
                &call_completed_at.to_rfc3339(),
                call_duration_ms,
                false,
                error_code.as_deref(),
                Some(&format!("{}", e)),
                http_status,
            ) {
                Ok(_) => info!(task_id = %lease.task_id, "Failed LLM call recorded to DB"),
                Err(e) => {
                    warn!(task_id = %lease.task_id, error = ?e, "Failed to record failed LLM call metrics")
                }
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
                format!(
                    "auth_failed: LLM provider rejected credentials (HTTP {})",
                    status.unwrap_or(0)
                )
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

// F-NEW-C: распознать структурный отказ модели по контракту.
//
// Модель обязана вернуть строго JSON `{"error": "missing_input", ...}`,
// когда постановки задачи нет ни в контракте, ни в описании. Такой отказ
// указывает на дефект контракта/планировщика, а не на ошибку исполнения,
// поэтому его нельзя «исправлять» реворком с тем же контрактом.
//
// Возвращает details отказа, если ответ является таким маркером.
pub fn contract_error_marker(text: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(text.trim()).ok()?;
    let err = v.get("error")?.as_str()?;
    // Расширяемо: любые *_input-ошибки входа трактуются как дефект контракта.
    if err == "missing_input" || err.ends_with("_input") {
        let details = v
            .get("details")
            .and_then(|d| d.as_str())
            .unwrap_or("")
            .to_string();
        return Some(if details.is_empty() {
            err.to_string()
        } else {
            format!("{err}: {details}")
        });
    }
    None
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
///
/// FIND-CODE-02: `rework` и `last_review` — контур обратной связи. При
/// `rework > 0` в промпт попадает диагностика последнего отказа ревьюера,
/// иначе воркер перегенерирует то же решение вслепую.
///
/// Итерация 3: `parents` — артефакты зависимостей (DAG handoff); пустой
/// список сохраняет прежнее поведение промпта.
/// Потолок превью родительского артефакта (байты UTF-8).
///
/// Было 4 КБ. Живой прогон 2026-10-17 (`9482c71a`) показал, что этого мало:
/// Handoff родителя — это дифф плюс секции `## Decision Trail` / `## Handoff`,
/// и на 4 КБ обрезка приходилась посреди unified diff. Воркер видел половину
/// хунка без контекстных строк и уходил в `missing_input`, сжигая задачу.
/// 16 КБ вмещают типичный дифф целиком и остаются умеренными для промпта.
/// Тело читается best-effort и режется по границе символа.
const PARENT_PREVIEW_MAX_BYTES: usize = 16 * 1024;

/// Итерация 3 (Вопрос 3 аудита Gemini Argon): превью содержимого родителей.
///
/// Возвращает срез той же длины, что `parents`: `Some(text)` — если файл
/// артефакта есть на общем томе и его sha256 сошёлся, иначе `None`.
///
/// Почему это здесь, а не в SQL-контракте `013`: тянуть тела артефактов через
/// Postgres — лишний трафик и раздувание плана. У воркера том уже примонтирован,
/// и метаданные из RPC дают точный указатель, куда смотреть.
///
/// Почему проверяем sha256: имя файла — хэш содержимого, но файл мог быть
/// подменён/повреждён. Непроверенное превью отдавать модели нельзя.
fn load_parent_previews(
    cfg: &WorkerConfig,
    parents: &[swarm_core::ParentArtifactRef],
) -> Vec<Option<String>> {
    parents
        .iter()
        .map(|p| {
            let path = artifacts::artifact_path(&cfg.artifacts_dir, &p.artifact_ref);
            if let Err(e) = artifacts::verify_artifact(&path, &p.artifact_ref) {
                // Нет файла или хэш не сошёлся — не ошибка задачи: секция
                // handoff деградирует до метаданных (поведение до этого PR).
                debug!(
                    parent = %p.parent_task_id,
                    path = %path.display(),
                    error = %e,
                    "handoff: parent artifact preview unavailable"
                );
                return None;
            }
            match std::fs::read(&path) {
                Ok(bytes) => {
                    let mut end = bytes.len().min(PARENT_PREVIEW_MAX_BYTES);
                    while end > 0 && std::str::from_utf8(&bytes[..end]).is_err() {
                        end -= 1;
                    }
                    let text = String::from_utf8_lossy(&bytes[..end]).into_owned();
                    if bytes.len() > end {
                        Some(format!("{text}\n… (превью обрезано)"))
                    } else {
                        Some(text)
                    }
                }
                Err(e) => {
                    debug!(
                        parent = %p.parent_task_id,
                        path = %path.display(),
                        error = %e,
                        "handoff: parent artifact preview read failed"
                    );
                    None
                }
            }
        })
        .collect()
}

/// Вход промпта одним значением: шесть независимых параметров в сигнатуре
/// давали 8 аргументов и упирались в `clippy::too_many_arguments`.
struct LlmPromptInput<'a> {
    contract: &'a serde_json::Value,
    rework: i32,
    last_review: Option<&'a serde_json::Value>,
    parents: &'a [swarm_core::ParentArtifactRef],
    previews: &'a [Option<String>],
    lessons: &'a [swarm_core::MemoryLesson],
    repo: &'a [crate::repo_context::RepoFile],
}

fn call_llm_with_retries(
    llm: &dyn LlmClient,
    cfg: &WorkerConfig,
    input: LlmPromptInput<'_>,
) -> Result<swarm_llm::LlmResponse> {
    let sys = prompt::system_prompt();
    let user = prompt::user_prompt_with_repo(
        input.contract,
        input.rework,
        input.last_review,
        input.parents,
        input.previews,
        input.lessons,
        input.repo,
    );
    let selected_model = select_model(cfg, input.contract, input.rework);

    let req = LlmRequest {
        system: sys,
        user,
        max_tokens: Some(cfg.max_tokens),
        temperature: None,
        model: selected_model,
    };

    let mut attempt = 0u32;
    loop {
        // Issue #4: считаем КАЖДЫЙ вызов LLM (включая ретраи) — главный цикл
        // останавливает проход по `max_llm_calls`.
        if let Some(counter) = &cfg.llm_calls {
            counter.fetch_add(1, Ordering::SeqCst);
        }
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

/// Issue #7/#10: выбор модели по измеренной цене успешного результата.
/// `None` — модель берётся из конфига клиента (поведение по умолчанию).
fn select_model(cfg: &WorkerConfig, contract: &serde_json::Value, rework: i32) -> Option<String> {
    cfg.model_selector.as_ref().and_then(|s| {
        // `choose_with_rework`, а не `choose`: при `rework > 0` эффективный
        // уровень задачи поднимается, и повтор не отдаётся той же модели,
        // которая уже ошиблась. Раньше здесь стоял `choose` (rework = 0),
        // поэтому механизм селектора существовал, но не срабатывал.
        let sel = s.choose_with_rework(contract, rework)?;
        info!(
            model = %sel.model,
            basis = ?sel.basis,
            reason = %sel.reason,
            "model selected"
        );
        Some(sel.model)
    })
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;

    // F9: сон обязан прерываться по флагу, а не досыпать полный интервал.
    // До фикса был `thread::sleep(interval)`; `HeartbeatGuard::drop` делал
    // `join` и ждал до `lease_sec/3` (при 300 с — до 100 с) после завершения
    // задачи. Тест доказывает, что прерывание срабатывает за <1 с.
    #[test]
    fn sleep_interruptible_wakes_on_stop() {
        let stop = Arc::new(AtomicBool::new(false));
        let s2 = stop.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            s2.store(true, Ordering::SeqCst);
        });
        let start = Instant::now();
        // Просим поспать 10 с, но флаг встанет через 100 мс.
        let interrupted = sleep_interruptible(&stop, Duration::from_secs(10));
        let elapsed = start.elapsed();
        assert!(interrupted, "сон должен сообщить о прерывании");
        assert!(
            elapsed < Duration::from_secs(1),
            "прерывание за <1 с, а не за 10 с (прошло {elapsed:?})"
        );
    }

    // F9: без флага сон досыпает полностью и возвращает false.
    #[test]
    fn sleep_interruptible_returns_false_without_stop() {
        let stop = AtomicBool::new(false);
        let start = Instant::now();
        let interrupted = sleep_interruptible(&stop, Duration::from_millis(300));
        assert!(!interrupted);
        assert!(start.elapsed() >= Duration::from_millis(290));
    }

    // F-NEW-C: структурный отказ модели распознаётся как дефект контракта.
    #[test]
    fn contract_error_marker_detects_missing_input() {
        let t = r#"{"error": "missing_input", "details": "нет постановки"}"#;
        let m = contract_error_marker(t).expect("должен распознать missing_input");
        assert!(m.contains("missing_input"));
        assert!(m.contains("нет постановки"));
    }

    #[test]
    fn contract_error_marker_tolerates_whitespace() {
        let t = "  {\n  \"error\": \"missing_input\"\n}  ";
        let m = contract_error_marker(t).expect("должен распознать с пробелами");
        assert_eq!(m, "missing_input");
    }

    // Обычный результат (не маркер ошибки) не должен считаться дефектом контракта.
    #[test]
    fn contract_error_marker_ignores_normal_artifact() {
        assert!(contract_error_marker("def foo(): pass").is_none());
        assert!(contract_error_marker(r#"{"result": "готово"}"#).is_none());
        // иной JSON-error не трогаем
        assert!(contract_error_marker(r#"{"error": "timeout"}"#).is_none());
    }
}
