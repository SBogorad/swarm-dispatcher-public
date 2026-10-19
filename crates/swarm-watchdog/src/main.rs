//! Сторож: раз в N секунд вызывает `reclaim_expired` и `promote_ready`.
//!
//! Отдельный процесс. Падение воркера не останавливает восстановление аренд.
//! Несколько копий безопасны: обе функции работают через FOR UPDATE SKIP LOCKED.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use swarm_core::{DispatcherError, SwarmDispatcher};
use tracing::{debug, error, info};
use tracing_subscriber::EnvFilter;

const DEFAULT_INTERVAL_SEC: u64 = 30;
const MAX_BACKOFF_SEC: u64 = 30;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::from_default_env().add_directive(
                "swarm_watchdog=info"
                    .parse()
                    .expect("static directive is valid"),
            ),
        )
        .json()
        .init();

    let dsn = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let interval = std::env::var("WATCHDOG_INTERVAL")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_INTERVAL_SEC);

    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown_handler = shutdown.clone();
    ctrlc::set_handler(move || {
        info!("received shutdown signal, finishing current iteration");
        shutdown_handler.store(true, Ordering::SeqCst);
    })
    .expect("install signal handler");

    info!(interval_sec = interval, "watchdog starting");

    let mut backoff = 1u64;
    while !shutdown.load(Ordering::SeqCst) {
        match run_iteration(&dsn) {
            Ok((reclaimed, promoted)) => {
                if reclaimed > 0 || promoted > 0 {
                    info!(reclaimed, promoted, "iteration complete");
                } else {
                    debug!("no changes");
                }
                backoff = 1;
                sleep_until(&shutdown, interval);
            }
            Err(e) => {
                error!(error = %e, backoff_sec = backoff, "iteration failed");
                sleep_until(&shutdown, backoff);
                backoff = (backoff * 2).min(MAX_BACKOFF_SEC);
            }
        }
    }
    info!("watchdog stopped");
}

/// Одна итерация: reclaim + promote. Открывает новое соединение каждый раз —
/// так падение не оставляет «залипшее» подключение.
///
/// Гард версии схемы проверяется ЗДЕСЬ, а не только на старте процесса: сторож
/// живёт долго и переживает выкатку новых миграций. Проверка на каждом
/// подключении ловит момент, когда схема уехала вперёд, а бинарь остался старым
/// (или наоборот). Пустое `SWARM_EXPECTED_SCHEMA` отключает проверку (тесты).
fn run_iteration(dsn: &str) -> Result<(i32, i32), DispatcherError> {
    let mut d = SwarmDispatcher::connect(dsn)?;
    let expected = std::env::var("SWARM_EXPECTED_SCHEMA").unwrap_or_default();
    d.ensure_schema_version(&expected)?;
    let reclaimed = d.reclaim_expired()?;
    let promoted = d.promote_ready()?;
    Ok((reclaimed, promoted))
}

/// Спит с проверкой флага shutdown. Возвращается сразу после сигнала.
fn sleep_until(shutdown: &AtomicBool, secs: u64) {
    let step = Duration::from_millis(500);
    let total = Duration::from_secs(secs);
    let start = std::time::Instant::now();
    while start.elapsed() < total {
        if shutdown.load(Ordering::SeqCst) {
            return;
        }
        thread::sleep(step);
    }
}
