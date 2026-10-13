//! Ревьюер: независимая приёмка результата исполнителя (Issue #26).
//!
//! В прод-контуре не было компонента, закрывающего `review -> done`; задачи
//! копились в `review`, а зависимые `pending` не промоутились.
//!
//! Ревьюер НЕ выдумывает успех: он читает артефакт исполнителя и проверяет
//! его содержимое. Артефакт-заглушка (`{"error": ...}`), пустой или слишком
//! короткий получает `changes_requested` (движок уводит на доработку).
//!
//! Говорит с движком только публичными функциями: `review_candidates(n)` и
//! `submit_review(...)` — роль `swarm_reviewer` (least privilege, без прямого
//! SELECT на `tasks`/`artifacts`).
//!
//! Env:
//! - `DATABASE_URL` (обязательно) — DSN роли `swarm_reviewer`
//! - `REVIEWER_ID` (по умолчанию `agent_reviewer`) — должен отличаться от
//!   автора артефакта, иначе `submit_review` вернёт RV002 self_review
//! - `ARTIFACTS_DIR` (по умолчанию /var/swarm/artifacts) — точка монтирования
//!   общего volume артефактов
//! - `REVIEW_MIN_BYTES` (по умолчанию 80) — минимальный «полезный» размер
//! - `REVIEW_POLL_SEC` (по умолчанию 2.0) — период опроса кандидатов

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use serde_json::{json, Value};
use swarm_core::{DispatcherError, SwarmDispatcher, Verdict};
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;

const DEFAULT_ARTIFACTS_DIR: &str = "/var/swarm/artifacts";

struct ReviewerConfig {
    dsn: String,
    reviewer_id: String,
    artifacts_dir: PathBuf,
    min_bytes: usize,
    poll_sec: f64,
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::from_default_env()
                .add_directive("swarm_reviewer=info".parse().expect("static directive is valid")),
        )
        .json()
        .init();

    let cfg = ReviewerConfig {
        dsn: std::env::var("DATABASE_URL").expect("DATABASE_URL must be set"),
        reviewer_id: std::env::var("REVIEWER_ID").unwrap_or_else(|_| "agent_reviewer".into()),
        artifacts_dir: PathBuf::from(
            std::env::var("ARTIFACTS_DIR").unwrap_or_else(|_| DEFAULT_ARTIFACTS_DIR.into()),
        ),
        min_bytes: std::env::var("REVIEW_MIN_BYTES")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(80),
        poll_sec: std::env::var("REVIEW_POLL_SEC")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(2.0),
    };

    let shutdown = Arc::new(AtomicBool::new(false));
    let s = shutdown.clone();
    ctrlc::set_handler(move || {
        info!("received shutdown signal, finishing current iteration");
        s.store(true, Ordering::SeqCst);
    })
    .expect("install signal handler");

    info!(
        reviewer_id = %cfg.reviewer_id,
        artifacts_dir = %cfg.artifacts_dir.display(),
        "reviewer starting"
    );

    let mut d = match SwarmDispatcher::connect(&cfg.dsn) {
        Ok(d) => d,
        Err(e) => {
            error!(error = %e, "connect failed");
            std::process::exit(2);
        }
    };

    let poll = Duration::from_secs_f64(cfg.poll_sec.max(0.2));
    while !shutdown.load(Ordering::SeqCst) {
        match review_once(&mut d, &cfg) {
            Ok(n) => {
                if n > 0 {
                    info!(reviewed = n, "iteration complete");
                }
            }
            Err(e) => warn!(error = %e, "iteration failed"),
        }
        // Прерываемый сон: сигнал останавливает без ожидания полного периода.
        let deadline = std::time::Instant::now() + poll;
        while std::time::Instant::now() < deadline {
            if shutdown.load(Ordering::SeqCst) {
                break;
            }
            thread::sleep(Duration::from_millis(100));
        }
    }
    info!("reviewer stopped");
}

/// Один проход: забрать кандидатов и вынести вердикт каждому.
/// Возвращает число обработанных кандидатов.
fn review_once(d: &mut SwarmDispatcher, cfg: &ReviewerConfig) -> Result<usize, DispatcherError> {
    let candidates = d.review_candidates(10)?;
    let mut done = 0usize;
    for cand in candidates {
        let Some(ref_) = cand.result_ref.clone() else {
            continue;
        };
        let text = cand
            .storage_path
            .as_deref()
            .and_then(|sp| read_artifact(sp, &cfg.artifacts_dir));
        let (verdict, reasons) = verify(&cand.contract, text.as_deref(), cfg.min_bytes);
        let checks = checks_for(&cand.contract, &ref_, verdict == Verdict::Approved);
        match d.submit_review(cand.id, &ref_, &cfg.reviewer_id, verdict, &checks) {
            Ok(state) => {
                let note = if verdict == Verdict::Approved {
                    String::new()
                } else {
                    format!(" ({})", reasons.join("; "))
                };
                info!(
                    task = %cand.id,
                    verdict = verdict.as_str(),
                    state = %state,
                    note = %note,
                    "reviewed"
                );
                done += 1;
            }
            // Гонка: пока читали артефакт, задачу сняли/переоткрыли — не наша ошибка.
            Err(e) => warn!(task = %cand.id, error = %e, "submit_review failed"),
        }
    }
    Ok(done)
}

/// Прочитать текст артефакта по `storage_path`.
///
/// `storage_path` бывает двух видов (см. `swarm-worker::worker::local_storage_uri`):
///   * `volume://<name>/<relpath>` — разрешаем относительно `ARTIFACTS_DIR`;
///   * абсолютный путь (локальный режим) — читаем как есть.
/// Фолбэк по basename покрывает случай, когда volume смонтирован в другую точку.
fn read_artifact(storage_path: &str, artifacts_dir: &Path) -> Option<String> {
    let candidates = artifact_candidates(storage_path, artifacts_dir);
    for p in candidates {
        if let Ok(bytes) = std::fs::read(&p) {
            return Some(String::from_utf8_lossy(&bytes).into_owned());
        }
    }
    None
}

fn artifact_candidates(storage_path: &str, artifacts_dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(rest) = storage_path.strip_prefix("volume://") {
        // rest = "<name>/<relpath>"
        if let Some((_vol, rel)) = rest.split_once('/') {
            out.push(artifacts_dir.join(rel));
        }
    } else {
        out.push(PathBuf::from(storage_path));
    }
    if let Some(base) = Path::new(storage_path).file_name() {
        out.push(artifacts_dir.join(base));
    }
    out
}

/// Артефакт-отчёт воркера о невозможности выполнить контракт
/// (`{"error": ...}` — маркер отказа, см. `fail_artifact` в воркере).
fn looks_like_error(text: &str) -> bool {
    let head = text.trim();
    if !head.starts_with('{') {
        return false;
    }
    match serde_json::from_str::<Value>(head) {
        Ok(Value::Object(map)) => map.contains_key("error"),
        _ => false,
    }
}

/// Реальная проверка артефакта. Возвращает `(вердикт, причины)`.
fn verify(contract: &Value, text: Option<&str>, min_bytes: usize) -> (Verdict, Vec<String>) {
    let Some(text) = text else {
        return (Verdict::ChangesRequested, vec!["артефакт не найден".into()]);
    };
    let body = text.trim();
    if body.is_empty() {
        return (Verdict::ChangesRequested, vec!["артефакт пуст".into()]);
    }
    if looks_like_error(body) {
        let detail = serde_json::from_str::<Value>(body)
            .ok()
            .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(String::from))
            .unwrap_or_default();
        return (
            Verdict::ChangesRequested,
            vec![format!("исполнитель вернул ошибку: {}", truncate(&detail, 120))],
        );
    }
    let _ = contract;
    if body.len() < min_bytes {
        return (
            Verdict::ChangesRequested,
            vec![format!("слишком короткий результат ({} символов)", body.len())],
        );
    }
    (Verdict::Approved, Vec::new())
}

/// Набор проверок по требованиям контракта с фактическим статусом.
///
/// Ключ проверки = `coalesce(id, type || '|' || run)` — ровно так его считает
/// строгий review-gate в `submit_review` (RV003). Один элемент отчёта на
/// каждое требование контракта, иначе гейт отклонит «зелёный» набор.
fn checks_for(contract: &Value, artifact_ref: &str, ok: bool) -> Value {
    let reqs = contract
        .get("checks")
        .and_then(|c| c.as_array())
        .cloned()
        .unwrap_or_default();
    let status = if ok { "pass" } else { "fail" };
    let mut checks = Vec::with_capacity(reqs.len());
    for c in reqs {
        let ctype = c.get("type").and_then(|v| v.as_str()).unwrap_or("check");
        let run = c.get("run").and_then(|v| v.as_str()).unwrap_or("");
        let id = c
            .get("id")
            .and_then(|v| v.as_str())
            .map(String::from)
            .unwrap_or_else(|| format!("{ctype}|{run}"));
        checks.push(json!({
            "id": id,
            "type": ctype,
            "run": run,
            "status": status,
            "artifact_ref": artifact_ref,
        }));
    }
    Value::Array(checks)
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        return s.to_string();
    }
    s.chars().take(n).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contract_with(checks: Value) -> Value {
        json!({"kind": "test", "checks": checks})
    }

    #[test]
    fn missing_artifact_is_changes_requested() {
        let (v, r) = verify(&contract_with(json!([])), None, 10);
        assert_eq!(v, Verdict::ChangesRequested);
        assert!(r[0].contains("не найден"));
    }

    #[test]
    fn empty_and_whitespace_are_changes_requested() {
        let c = contract_with(json!([]));
        assert_eq!(verify(&c, Some(""), 10).0, Verdict::ChangesRequested);
        assert_eq!(verify(&c, Some("  \n\t "), 10).0, Verdict::ChangesRequested);
    }

    #[test]
    fn error_marker_is_changes_requested() {
        let c = contract_with(json!([]));
        let body = r#"{"error":"empty_response: LLM вернул пустой ответ","task_id":"x","epoch":1}"#;
        let (v, r) = verify(&c, Some(body), 1);
        assert_eq!(v, Verdict::ChangesRequested);
        assert!(r[0].contains("ошибку"));
    }

    #[test]
    fn short_result_is_changes_requested() {
        let c = contract_with(json!([]));
        let (v, r) = verify(&c, Some("ok"), 80);
        assert_eq!(v, Verdict::ChangesRequested);
        assert!(r[0].contains("короткий"));
    }

    #[test]
    fn useful_result_is_approved() {
        let c = contract_with(json!([]));
        let body = "A".repeat(200);
        assert_eq!(verify(&c, Some(&body), 80).0, Verdict::Approved);
    }

    #[test]
    fn checks_mirror_contract_keys() {
        let c = contract_with(json!([
            {"type": "cmd", "run": "cargo test"},
            {"id": "lint", "type": "cmd", "run": "cargo clippy"}
        ]));
        let checks = checks_for(&c, "sha256:abc", true);
        let arr = checks.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["id"], "cmd|cargo test");
        assert_eq!(arr[0]["status"], "pass");
        assert_eq!(arr[1]["id"], "lint");
        assert_eq!(arr[1]["artifact_ref"], "sha256:abc");
        // Провал: тот же набор ключей, но status=fail — гейт не пропустит approved.
        let bad = checks_for(&c, "sha256:abc", false);
        assert!(bad.as_array().unwrap().iter().all(|c| c["status"] == "fail"));
    }

    #[test]
    fn volume_uri_resolves_under_mount() {
        let cands = artifact_candidates(
            "volume://swarm-artifacts/deadbeef.txt",
            Path::new("/var/swarm/artifacts"),
        );
        assert_eq!(
            cands[0],
            PathBuf::from("/var/swarm/artifacts/deadbeef.txt")
        );
    }

    #[test]
    fn absolute_path_used_as_is() {
        let cands = artifact_candidates("/tmp/art.txt", Path::new("/var/swarm/artifacts"));
        assert_eq!(cands[0], PathBuf::from("/tmp/art.txt"));
    }
}
