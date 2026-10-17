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
//! - `REVIEW_REQUIRE_DECISION_TRAIL` (по умолчанию `0`) — строгий режим рубрики:
//!   требовать секцию `## Decision Trail` с проверяемыми `evidence`.
//!   По умолчанию ВЫКЛ, чтобы unit-тесты без env и старые артефакты не
//!   ломались; боевой и демо-контуры включают его через compose (`: "1"`).

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
    require_decision_trail: bool,
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::from_default_env().add_directive(
                "swarm_reviewer=info"
                    .parse()
                    .expect("static directive is valid"),
            ),
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
        // Строгий режим рубрики: «1»/"true"/"yes" → включён.
        require_decision_trail: std::env::var("REVIEW_REQUIRE_DECISION_TRAIL")
            .map(|v| {
                let v = v.trim().to_ascii_lowercase();
                v == "1" || v == "true" || v == "yes"
            })
            .unwrap_or(false),
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
        let (verdict, reasons) = verify_with_trail(
            &cand.contract,
            text.as_deref(),
            cfg.min_bytes,
            cfg.require_decision_trail,
        );
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
///
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
///
/// Итерация 2: двухшаговый фильтр по мотивам `interrogate/references/rubric.md`
/// и `deslop`.
///
/// Шаг А: структурный отказ контракта (`missing_input` / `contract_error` /
/// `unachievable_contract`) — это дефект ПОСТАНОВКИ, а не исполнителя.
/// Реворк его не исправит (контракт от повтора не меняется), поэтому такой
/// артефакт получает `Rejected` → задача сразу уходит в `blocked`, без
/// сжигания `max_rework`. Прочие `{"error": ...}` остаются
/// `ChangesRequested` — обратная совместимость.
///
/// Шаг Б: маркеры недоделанной работы (`todo!()`, `unimplemented!()`, ...)
/// дают `ChangesRequested` с префиксом `[RUBRIC:UNSLOP]`; `detail` доедет до
/// воркера через `task_rework_feedback`.
fn verify(contract: &Value, text: Option<&str>, min_bytes: usize) -> (Verdict, Vec<String>) {
    verify_with_trail(contract, text, min_bytes, false)
}

/// Полная проверка артефакта с управляемым строгим режимом рубрики.
///
/// `require_trail = true` (env `REVIEW_REQUIRE_DECISION_TRAIL=1`) дополнительно
/// требует секцию `## Decision Trail` с проверяемыми `evidence`.
fn verify_with_trail(
    contract: &Value,
    text: Option<&str>,
    min_bytes: usize,
    require_trail: bool,
) -> (Verdict, Vec<String>) {
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
        // Шаг А: структурный отказ контракта → Rejected (сразу blocked).
        if is_contract_error(&detail) {
            return (
                Verdict::Rejected,
                vec![format!(
                    "[RUBRIC:CONTRACT] контракт неисполним: {}",
                    truncate(&detail, 120)
                )],
            );
        }
        return (
            Verdict::ChangesRequested,
            vec![format!(
                "исполнитель вернул ошибку: {}",
                truncate(&detail, 120)
            )],
        );
    }
    // Шаг Б: рубрикатор анти-слопа.
    if let Some(marker) = unfinished_stub_marker(body) {
        return (
            Verdict::ChangesRequested,
            vec![format!(
                "[RUBRIC:UNSLOP] artifact contains unfinished stub marker: {marker}"
            )],
        );
    }
    let _ = contract;
    if body.len() < min_bytes {
        return (
            Verdict::ChangesRequested,
            vec![format!(
                "слишком короткий результат ({} символов)",
                body.len()
            )],
        );
    }
    // Строгий режим рубрики: разбор решений с доказательствами.
    if require_trail {
        if let Err(why) = validate_decision_trail(body) {
            return (
                Verdict::ChangesRequested,
                vec![format!("[RUBRIC:TRAIL] {why}")],
            );
        }
    }
    (Verdict::Approved, Vec::new())
}

/// Структурный отказ контракта — дефект постановки, а не исполнения.
fn is_contract_error(error: &str) -> bool {
    let e = error.trim().to_ascii_lowercase();
    e == "missing_input" || e == "contract_error" || e == "unachievable_contract"
}

/// Маркеры недоделанной работы в артефакте (`deslop` / `no-comments`).
///
/// Возвращает найденный маркер для отчёта ревьюера.
fn unfinished_stub_marker(body: &str) -> Option<&'static str> {
    const MARKERS: [&str; 9] = [
        "unimplemented!()",
        "todo!()",
        "TODO: implement",
        "FIXME: implement",
        "<INSERT_CODE_HERE>",
        // Итерация 4: маркеры из `interrogate/references/rubric.md` и
        // `unslop/SKILL.md` — заглушки, которые проходят длину, но не работу.
        "NotImplementedError",
        "PLACEHOLDER",
        "lorem ipsum",
        "not implemented yet",
    ];
    MARKERS.iter().find(|m| body.contains(**m)).copied()
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

// ---------------------------------------------------------------------------
// Строгий режим рубрики: `## Decision Trail` (канон `show-me-your-work`).
// ---------------------------------------------------------------------------

/// Стоп-лист «пустых» доказательств: значения, которые ничего не доказывают.
const EVIDENCE_STOPLIST: [&str; 10] = [
    "", "-", "n/a", "none", "todo", "tbd", "?", "ok", "yes", "did",
];

/// Секция `## Decision Trail` из артефакта (до следующего заголовка `## `).
fn decision_trail_section(body: &str) -> Option<&str> {
    const HEADER: &str = "## Decision Trail";
    let start = body.find(HEADER)? + HEADER.len();
    let rest = &body[start..];
    let end = rest.find("\n## ").map(|i| i + 1).unwrap_or(rest.len());
    Some(&rest[..end])
}

/// Проверить структуру `## Decision Trail`: минимум одна строка решения,
/// у которой `evidence` — реальный проверяемый указатель.
///
/// Возвращает число проверенных строк решений.
fn validate_decision_trail(body: &str) -> Result<usize, String> {
    let section =
        decision_trail_section(body).ok_or_else(|| "нет секции `## Decision Trail`".to_string())?;
    let mut rows = 0usize;
    for line in section.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        // Markdown-таблица `| a | b |` либо TSV `a\tb`.
        let cells: Vec<&str> = if line.contains('|') {
            line.trim_matches('|').split('|').map(str::trim).collect()
        } else if line.contains('\t') {
            line.split('\t').map(str::trim).collect()
        } else {
            continue;
        };
        if cells.len() < 5 {
            continue;
        }
        // Пропускаем шапку (phase/ts/evidence) и строку-разделитель.
        let first = cells[0].to_ascii_lowercase();
        if first == "phase"
            || first == "ts"
            || cells.iter().any(|c| c.eq_ignore_ascii_case("evidence"))
            || cells.iter().all(|c| is_separator_cell(c))
        {
            continue;
        }
        // Канон: `phase | decision | why | evidence | result` (5 столбцов);
        // TSV из `show-me-your-work` добавляет `ts` (6). В обоих случаях
        // `evidence` — предпоследняя ячейка.
        let evidence = cells[cells.len() - 2];
        if !evidence_is_pointer(evidence) {
            return Err(format!(
                "строка `{}`: evidence `{}` не является проверяемым указателем",
                truncate(cells[1], 40),
                truncate(evidence, 40)
            ));
        }
        rows += 1;
    }
    if rows == 0 {
        return Err("секция `## Decision Trail` без строк решений".into());
    }
    Ok(rows)
}

/// Ячейка строки-разделителя markdown-таблицы (`---`, `:--:`, пусто).
fn is_separator_cell(c: &str) -> bool {
    !c.is_empty() && c.chars().all(|ch| ch == '-' || ch == ':' || ch == ' ')
}

/// `evidence` — реальный проверяемый указатель, а не отписка.
///
/// Указатель (канон `show-me-your-work/SKILL.md`):
///   * путь к файлу или `file:line` (есть `/`, расширение, либо `имя:число`);
///   * ссылка на артефакт (`sha256:`, `PR #`, `#`, SHA коммита >= 7 hex);
///   * проверяемая команда или результат прогона (`cargo `, `pytest`, `rc=0`, ...).
fn evidence_is_pointer(cell: &str) -> bool {
    let e = cell.trim();
    let low = e.to_ascii_lowercase();
    if EVIDENCE_STOPLIST.contains(&low.as_str()) || e.chars().count() < 3 {
        return false;
    }
    // Путь к файлу или `file:line`.
    if e.contains('/') {
        return true;
    }
    const EXTS: [&str; 9] = [
        ".rs", ".sql", ".py", ".sh", ".md", ".toml", ".yml", ".yaml", ".json",
    ];
    if EXTS
        .iter()
        .any(|x| low.ends_with(x) || low.contains(&format!("{x}:")))
    {
        return true;
    }
    // `имя:число` (строка/столбец).
    if e.split_once(':').is_some_and(|(name, num)| {
        !name.is_empty() && num.chars().next().is_some_and(|c| c.is_ascii_digit())
    }) {
        return true;
    }
    // Ссылка на артефакт: хэш, PR, коммит.
    if low.contains("sha256:") || low.contains("pr #") || e.contains('#') || has_hex_token(e, 7) {
        return true;
    }
    // Проверяемая команда или результат прогона.
    const CMDS: [&str; 6] = ["cargo ", "pytest", "test", "select ", "rc=0", "exit 0"];
    CMDS.iter().any(|c| low.contains(c))
}

/// Есть ли в строке hex-токен длиной >= `min_len` (SHA коммита).
fn has_hex_token(s: &str, min_len: usize) -> bool {
    s.split(|c: char| !c.is_ascii_hexdigit())
        .any(|t| t.len() >= min_len && t.chars().all(|c| c.is_ascii_hexdigit()))
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

    // Итерация 2, шаг А: структурный отказ контракта → Rejected (сразу blocked),
    // а не ChangesRequested — реворк с тем же контрактом бессмысленен.
    #[test]
    fn contract_error_is_rejected() {
        let c = contract_with(json!([]));
        for err in ["missing_input", "contract_error", "unachievable_contract"] {
            let body = format!(r#"{{"error":"{err}","details":"нет постановки"}}"#);
            let (v, r) = verify(&c, Some(&body), 1);
            assert_eq!(v, Verdict::Rejected, "{err} должен давать Rejected");
            assert!(
                r[0].contains("[RUBRIC:CONTRACT]"),
                "нет метки рубрики: {r:?}"
            );
        }
    }

    // Транзиентные ошибки остаются ChangesRequested (обратная совместимость).
    #[test]
    fn transient_error_is_not_rejected() {
        let c = contract_with(json!([]));
        let body = r#"{"error":"timeout"}"#;
        let (v, _) = verify(&c, Some(body), 1);
        assert_eq!(v, Verdict::ChangesRequested);
    }

    // Итерация 2, шаг Б: маркеры недоделанной работы → ChangesRequested с меткой.
    #[test]
    fn stub_markers_are_changes_requested() {
        let c = contract_with(json!([]));
        let body = format!(
            "fn f() {{ todo!() }}\n{}",
            "// padding to exceed min_bytes threshold".repeat(3)
        );
        let (v, r) = verify(&c, Some(&body), 80);
        assert_eq!(v, Verdict::ChangesRequested);
        assert!(r[0].contains("[RUBRIC:UNSLOP]"), "нет метки рубрики: {r:?}");
        assert!(r[0].contains("todo!()"), "маркер не назван: {r:?}");
    }

    #[test]
    fn each_stub_marker_is_detected() {
        let c = contract_with(json!([]));
        for marker in [
            "unimplemented!()",
            "todo!()",
            "TODO: implement",
            "FIXME: implement",
            "<INSERT_CODE_HERE>",
        ] {
            let body = format!("{marker}\n{}", "x".repeat(200));
            let (v, r) = verify(&c, Some(&body), 80);
            assert_eq!(v, Verdict::ChangesRequested, "{marker} не пойман");
            assert!(r[0].contains("[RUBRIC:UNSLOP]"), "нет метки для {marker}");
        }
    }

    // Чистый валидный артефакт без маркеров → Approved.
    #[test]
    fn clean_artifact_is_approved_with_rubric() {
        let c = contract_with(json!([]));
        let body = "fn add(a: i32, b: i32) -> i32 { a + b }\n\n#[test]\nfn t() { assert_eq!(add(1, 2), 3); }".to_string();
        assert!(body.len() >= 80);
        let (v, r) = verify(&c, Some(&body), 80);
        assert_eq!(v, Verdict::Approved);
        assert!(r.is_empty(), "нет причин у одобренного: {r:?}");
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
        assert!(bad
            .as_array()
            .unwrap()
            .iter()
            .all(|c| c["status"] == "fail"));
    }

    #[test]
    fn volume_uri_resolves_under_mount() {
        let cands = artifact_candidates(
            "volume://swarm-artifacts/deadbeef.txt",
            Path::new("/var/swarm/artifacts"),
        );
        assert_eq!(cands[0], PathBuf::from("/var/swarm/artifacts/deadbeef.txt"));
    }

    #[test]
    fn absolute_path_used_as_is() {
        let cands = artifact_candidates("/tmp/art.txt", Path::new("/var/swarm/artifacts"));
        assert_eq!(cands[0], PathBuf::from("/tmp/art.txt"));
    }

    // ---------- Итерация 4: строгий режим рубрики (Decision Trail) ----------

    fn trail_body(section: &str) -> String {
        format!(
            "Отчёт по задаче.\n\n{section}\n\n## Handoff\nготово\n{}",
            "x".repeat(120)
        )
    }

    #[test]
    fn strict_mode_off_accepts_artifact_without_trail() {
        // Обратная совместимость: без env старые артефакты проходят.
        let c = contract_with(json!([]));
        let body = format!("Просто текст без секций.\n{}", "y".repeat(120));
        let (v, r) = verify_with_trail(&c, Some(&body), 80, false);
        assert_eq!(v, Verdict::Approved, "причины: {r:?}");
    }

    #[test]
    fn strict_mode_requires_trail_section() {
        let c = contract_with(json!([]));
        let body = format!("Текст без Decision Trail.\n{}", "y".repeat(120));
        let (v, r) = verify_with_trail(&c, Some(&body), 80, true);
        assert_eq!(v, Verdict::ChangesRequested);
        assert!(r[0].contains("[RUBRIC:TRAIL]"), "нет метки: {r:?}");
        assert!(r[0].contains("нет секции"), "{r:?}");
    }

    #[test]
    fn strict_mode_accepts_markdown_trail_with_pointer() {
        let c = contract_with(json!([]));
        let body = trail_body(
            "## Decision Trail\n\
             | phase | decision | why | evidence | result |\n\
             |---|---|---|---|---|\n\
             | fix | починил гонку | аренда не терялась | crates/swarm-core/src/dispatcher.rs:122 | tests green |",
        );
        let (v, r) = verify_with_trail(&c, Some(&body), 80, true);
        assert_eq!(v, Verdict::Approved, "причины: {r:?}");
    }

    #[test]
    fn strict_mode_accepts_tsv_trail_with_sha() {
        // Канон `show-me-your-work`: TSV с `ts` (6 столбцов), evidence — sha.
        let c = contract_with(json!([]));
        let body = trail_body(
            "## Decision Trail\n\
             ts\tphase\tdecision\twhy\tevidence\tresult\n\
             2026-10-17T00:00:00Z\tfix\tпочинил\tнадо\tsha256:aa11bb22\ttests green",
        );
        let (v, r) = verify_with_trail(&c, Some(&body), 80, true);
        assert_eq!(v, Verdict::Approved, "причины: {r:?}");
    }

    #[test]
    fn strict_mode_rejects_empty_evidence() {
        let c = contract_with(json!([]));
        for weak in ["", "-", "n/a", "todo", "did", "ok"] {
            let body = trail_body(&format!(
                "## Decision Trail\n| phase | decision | why | evidence | result |\n|---|---|---|---|---|\n| fix | сделал | так надо | {weak} | done |"
            ));
            let (v, r) = verify_with_trail(&c, Some(&body), 80, true);
            assert_eq!(v, Verdict::ChangesRequested, "evidence={weak:?} прошёл");
            assert!(r[0].contains("не является проверяемым указателем"), "{r:?}");
        }
    }

    #[test]
    fn strict_mode_rejects_trail_without_rows() {
        let c = contract_with(json!([]));
        let body = trail_body("## Decision Trail\n| phase | decision | why | evidence | result |\n|---|---|---|---|---|\n");
        let (v, r) = verify_with_trail(&c, Some(&body), 80, true);
        assert_eq!(v, Verdict::ChangesRequested);
        assert!(r[0].contains("без строк решений"), "{r:?}");
    }

    #[test]
    fn evidence_pointer_recognises_each_canonical_form() {
        // Путь / file:line / расширение.
        for ok in [
            "crates/swarm-core/src/dispatcher.rs",
            "crates/swarm-core/src/dispatcher.rs:122",
            "schema/schema_v0.sql",
            "scripts/acceptance_harness.sh",
            "docs/KNOWN_LIMITATIONS.md",
        ] {
            assert!(evidence_is_pointer(ok), "путь отвергнут: {ok}");
        }
        // Артефакт: хэш, PR, коммит.
        for ok in [
            "sha256:aa11bb22cc33",
            "PR #75",
            "0a8fa6f",
            "commit deadbeef",
        ] {
            assert!(evidence_is_pointer(ok), "артефакт отвергнут: {ok}");
        }
        // Команда или результат прогона.
        for ok in [
            "cargo test -p swarm-core",
            "pytest -q tests/",
            "rc=0",
            "exit 0",
        ] {
            assert!(evidence_is_pointer(ok), "команда отвергнута: {ok}");
        }
        // Отписки и мусор.
        for bad in [
            "", "-", "n/a", "none", "todo", "tbd", "?", "ok", "yes", "did",
        ] {
            assert!(!evidence_is_pointer(bad), "отписка прошла: {bad:?}");
        }
        assert!(!evidence_is_pointer("я проверил всё руками"));
    }

    #[test]
    fn new_stub_markers_are_detected() {
        let c = contract_with(json!([]));
        for marker in [
            "NotImplementedError",
            "PLACEHOLDER",
            "lorem ipsum",
            "not implemented yet",
        ] {
            let body = format!("{marker}\n{}", "x".repeat(200));
            let (v, r) = verify(&c, Some(&body), 80);
            assert_eq!(v, Verdict::ChangesRequested, "{marker} не пойман");
            assert!(r[0].contains("[RUBRIC:UNSLOP]"), "нет метки для {marker}");
        }
    }
}
