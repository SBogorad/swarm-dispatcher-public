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
//! - `MAX_TOTAL_COST_USD` (по умолчанию 5.0) — глобальный лимит расходов (Issue #3)
//! - `MAX_CONSECUTIVE_ERRORS` (по умолчанию 5) — порог самоотключения воркера
//! - `MAX_LLM_CALLS` (по умолчанию 0 = без лимита) — максимум LLM-вызовов за проход (Issue #4)
//! - `MAX_DURATION_SEC` (по умолчанию 0 = без лимита) — максимум секунд на проход (Issue #4)
//! - `RUN_ONCE` (0|1, по умолчанию 0) — один проход и выход (Issue #4)
//! - `SWARM_POLL_INTERVAL_MS` (по умолчанию 2000) — базовая задержка опроса
//! - `SWARM_POLL_MAX_MS` (по умолчанию 30000) — потолок backoff при простое
//! - `SWARM_MODEL_AUTOSELECT` (0|1, по умолчанию 0) — выбор модели по цене успеха (Issue #7/#10)
//! - `SWARM_MODEL_CATALOG_FILE` — JSON-каталог моделей (иначе одна модель из `LLM_MODEL`)
//! - `SWARM_MODEL_STATS_FILE` — JSON-статистика по моделям от `scripts/model_quality_report.py`
//! - `SWARM_MODEL_MIN_SAMPLES` (по умолчанию 5) — минимум задач, чтобы доверять измерению
//! - `SWARM_MODEL_MIN_SUCCESS_RATE` (по умолчанию 0.5) — минимальная доля успеха
//! - `SWARM_MODEL_OUTPUT_RATIO` (по умолчанию 2.0) — ожидаемое выход/вход для оценки по прайсу
//! - `SWARM_MODEL_DEFAULT_TIER` (по умолчанию 1) — уровень задачи без `quality_tier` в контракте
//! - `LLM_*` — см. swarm-llm

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use swarm_core::SwarmDispatcher;
use swarm_llm::{from_env as llm_from_env, LlmClient};
use swarm_worker::{
    process_lease, BudgetPolicy, ModelCatalog, ModelSelector, ModelStat, Outcome, PollPolicy,
    RetryPolicy, RunLimits, WorkerConfig,
};
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;

fn env_u64(name: &str, dflt: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(dflt)
}

fn env_f64(name: &str, dflt: f64) -> f64 {
    std::env::var(name)
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(dflt)
}

/// Issue #7/#10: собрать селектор модели, если он включён.
///
/// Возвращает `None` (поведение по умолчанию: модель из `LLM_MODEL`), если
/// `SWARM_MODEL_AUTOSELECT != 1`. Любая проблема с файлами каталога/статистики
/// не роняет воркер: деградируем на одну модель из конфига с громким warn.
fn build_model_selector(default_model: &str) -> Option<Arc<ModelSelector>> {
    if !std::env::var("SWARM_MODEL_AUTOSELECT")
        .map(|s| s == "1")
        .unwrap_or(false)
    {
        return None;
    }

    let catalog = match std::env::var("SWARM_MODEL_CATALOG_FILE") {
        Ok(path) => match std::fs::read_to_string(&path) {
            Ok(s) => match ModelCatalog::from_json_str(&s) {
                Ok(c) => c,
                Err(e) => {
                    warn!(error = %e, path, "model catalog invalid; falling back to configured model");
                    ModelCatalog::single(default_model)
                }
            },
            Err(e) => {
                warn!(error = %e, path, "model catalog unreadable; falling back to configured model");
                ModelCatalog::single(default_model)
            }
        },
        Err(_) => {
            warn!("SWARM_MODEL_CATALOG_FILE unset; selector has only the configured model");
            ModelCatalog::single(default_model)
        }
    };

    let stats: Vec<ModelStat> = match std::env::var("SWARM_MODEL_STATS_FILE") {
        Ok(path) => std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_else(|| {
                warn!(path, "model stats unreadable; selecting by list price");
                Vec::new()
            }),
        Err(_) => Vec::new(),
    };

    let sel = ModelSelector {
        catalog,
        stats,
        min_samples: env_u64("SWARM_MODEL_MIN_SAMPLES", 5),
        min_success_rate: env_f64("SWARM_MODEL_MIN_SUCCESS_RATE", 0.5),
        expected_output_ratio: env_f64("SWARM_MODEL_OUTPUT_RATIO", 2.0),
        default_task_tier: env_u64("SWARM_MODEL_DEFAULT_TIER", 1) as u8,
    };
    info!(
        candidates = sel.catalog.models.len(),
        measured = sel.stats.len(),
        min_samples = sel.min_samples,
        "model autoselect enabled (Issue #7/#10)"
    );
    Some(Arc::new(sel))
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::from_default_env()
                .add_directive("swarm_worker=info".parse().expect("static directive is valid")),
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

    let llm_calls = Arc::new(AtomicU32::new(0));
    // Issue #7/#10: модель для дефолта (когда селектор выключен или не нашёл
    // кандидатов) — та же, что читает LLM-клиент из `LLM_MODEL`.
    let default_model = std::env::var("LLM_MODEL").unwrap_or_else(|_| "deepseek-v4.1-flash".into());
    let model_selector = build_model_selector(&default_model);
    let cfg = WorkerConfig {
        artifacts_dir: PathBuf::from(
            std::env::var("ARTIFACTS_DIR").unwrap_or_else(|_| "/var/swarm/artifacts".into()),
        ),
        lease_sec,
        max_tokens: env_u64("WORKER_MAX_TOKENS", 4096) as u32,
        llm_timeout_sec: env_u64("WORKER_LLM_TIMEOUT_SEC", 300),
        retry: RetryPolicy::default(),
        work_sec: env_u64("WORK_SECONDS", 0),
        // Issue #3: hard budget guard. MAX_TOTAL_COST_USD — общий лимит расходов
        // на процесс воркера; MAX_CONSECUTIVE_ERRORS — порог самоотключения.
        budget: BudgetPolicy {
            max_total_cost_usd: std::env::var("MAX_TOTAL_COST_USD")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(5.0),
            max_consecutive_errors: env_u64("MAX_CONSECUTIVE_ERRORS", 5) as u32,
        },
        // Issue #4: жёсткие пределы прохода. 0 = без лимита (штатный воркер
        // не меняет поведение); лимиты включаются явно для контролируемого прохода.
        run_limits: RunLimits {
            max_llm_calls: env_u64("MAX_LLM_CALLS", 0) as u32,
            max_duration_sec: env_u64("MAX_DURATION_SEC", 0),
        },
        llm_calls: Some(llm_calls.clone()),
        model_selector,
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
        info!(
            worker_id,
            "shell-tools disabled (set WORKER_SHELL_TOOLS=1 to enable)"
        );
    }

    let mut consecutive_errors = 0u32;
    // Issue #15: backoff опроса вместо busy-poll. В простое интервал растёт
    // 2с → 4с → … → 30с; при получении задачи сбрасывается. Раньше воркер
    // дёргал claim_task каждые 2 с вхолостую (2 воркера = 60 запросов/мин).
    let poll = PollPolicy::from_env(
        std::env::var("SWARM_POLL_INTERVAL_MS")
            .ok()
            .and_then(|s| s.parse().ok()),
        std::env::var("SWARM_POLL_MAX_MS")
            .ok()
            .and_then(|s| s.parse().ok()),
    );
    let mut idle_rounds = 0u32;
    // Issue #4: один проход — одна задача и выход (контролируемый прогон).
    let run_once = std::env::var("RUN_ONCE").map(|s| s == "1").unwrap_or(false);
    let run_started = Instant::now();
    while !shutdown.load(Ordering::SeqCst) {
        // Issue #4: жёсткий предел прохода проверяем ПЕРЕД очередным claim.
        // Счётчик llm_calls — общий с `process_lease` (включает ретраи).
        if let Some(stop) = cfg.run_limits.check(
            llm_calls.load(Ordering::SeqCst),
            run_started.elapsed().as_secs(),
        ) {
            warn!(reason = %stop.reason(), "run limit reached; stopping worker");
            break;
        }
        match d.claim(&worker_id, lease_sec) {
            Ok(Some(lease)) => {
                consecutive_errors = 0;
                idle_rounds = 0;
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
                // Issue #4: RUN_ONCE — одна задача и выход, без возврата в цикл.
                if run_once {
                    info!("RUN_ONCE=1: single pass complete; exiting");
                    break;
                }
            }
            Ok(None) => {
                consecutive_errors = 0;
                let delay = poll.idle_delay(idle_rounds);
                idle_rounds = idle_rounds.saturating_add(1);
                tracing::debug!(
                    idle_rounds,
                    delay_ms = delay.as_millis(),
                    "queue empty, backing off"
                );
                thread::sleep(delay);
            }
            Err(e) => {
                consecutive_errors += 1;
                idle_rounds = 0;
                error!(error = %e, attempts = consecutive_errors, "claim failed");
                thread::sleep(Duration::from_secs((consecutive_errors as u64).min(30)));
            }
        }
    }
    info!("worker stopped");
}
