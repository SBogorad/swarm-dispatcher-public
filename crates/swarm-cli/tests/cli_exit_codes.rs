//! Интеграционные тесты CLI (Волна 5.2): exit-коды и fail-closed review.
//!
//! Дефект, который эти тесты закрывают: `swarm-cli` (205 строк) и
//! `swarm-watchdog` (84 строки) не имели НИ ОДНОГО теста, хотя именно CLI
//! минтит review и exit-коды, которые читают скрипты-вызывающие.
//!
//! Тесты запускают СОБРАННЫЙ бинарь как процесс и проверяют реальный
//! exit-код/вывод (не внутренние функции) — это то, что видит потребитель.
//!
//! Требует TEST_DATABASE_URL с детерминированной БД (disposable, prefix
//! c2_/v0/test_) и применённой схемы + roles. Без DSN тесты падают явно
//! (FIND-08: не молчаливый пропуск).

use std::process::Command;

fn dsn() -> String {
    std::env::var("TEST_DATABASE_URL").unwrap_or_else(|_| {
        panic!(
            "BLOCKED/FAIL: TEST_DATABASE_URL не задан; CLI-тесты требуют живую \
             тестовую БД (prefix test_/v0/c2_)"
        )
    })
}

/// Путь к собранному бинарю CLI (cargo передаёт его в env для integration-тестов).
fn cli_bin() -> String {
    env!("CARGO_BIN_EXE_swarm-cli").to_string()
}

fn run_cli(args: &[&str]) -> (i32, String, String) {
    let out = Command::new(cli_bin())
        .args(args)
        .env("DATABASE_URL", dsn())
        .output()
        .expect("failed to spawn swarm-cli");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
    )
}

/// Без DATABASE_URL CLI обязан упасть с ненулевым кодом и понятным сообщением.
#[test]
fn cli_requires_database_url() {
    let out = Command::new(cli_bin())
        .arg("status")
        .env_remove("DATABASE_URL")
        .output()
        .expect("spawn");
    assert!(
        !out.status.success(),
        "без DATABASE_URL должен быть ненулевой код"
    );
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("DATABASE_URL"),
        "сообщение должно упоминать DATABASE_URL, получено: {err}"
    );
}

/// `status` на живой БД: успех, код 0, печатает состояние.
#[test]
fn cli_status_succeeds_on_live_db() {
    let (code, stdout, stderr) = run_cli(&["status"]);
    assert_eq!(code, 0, "status должен вернуть 0; stderr={stderr}");
    assert!(!stdout.trim().is_empty(), "status должен что-то напечатать");
}

/// Fail-closed: `review --verdict approve` БЕЗ --checks обязан отказать
/// (CLI не выдумывает проходной набор проверок).
/// Проверяем на несуществующем task_id: отказ должен наступить ДО обращения
/// к БД за ревью — по причине отсутствия checks.
#[test]
fn cli_review_approve_without_checks_is_rejected() {
    let fake = "00000000-0000-0000-0000-000000000000";
    let (code, _stdout, stderr) = run_cli(&["review", fake, "sha256:deadbeef", "approve"]);
    assert_ne!(code, 0, "approve без --checks должен дать ненулевой код");
    assert!(
        stderr.contains("--checks"),
        "сообщение должно требовать --checks, получено: {stderr}"
    );
}

/// `--checks` должен быть валидным JSON-массивом.
#[test]
fn cli_review_rejects_non_array_checks() {
    let fake = "00000000-0000-0000-0000-000000000000";
    let (code, _stdout, stderr) = run_cli(&[
        "review",
        fake,
        "sha256:deadbeef",
        "approve",
        "--checks",
        "{\"not\":\"an array\"}",
    ]);
    assert_ne!(code, 0, "не-массив --checks должен быть отклонён");
    assert!(
        stderr.contains("array"),
        "сообщение должно говорить про JSON array, получено: {stderr}"
    );
}

/// `reopen` на несуществующей задаче: ошибка -> ненулевой код (не молчаливый успех).
#[test]
fn cli_reopen_unknown_task_fails() {
    let fake = "00000000-0000-0000-0000-000000000000";
    let (code, _stdout, stderr) = run_cli(&["reopen", fake, "op"]);
    assert_ne!(
        code, 0,
        "reopen неизвестной задачи должен упасть; stderr={stderr}"
    );
}

/// `unblock` на несуществующей задаче: ошибка -> ненулевой код.
#[test]
fn cli_unblock_unknown_task_fails() {
    let fake = "00000000-0000-0000-0000-000000000000";
    let (code, _stdout, stderr) = run_cli(&["unblock", fake, "op"]);
    assert_ne!(
        code, 0,
        "unblock неизвестной задачи должен упасть; stderr={stderr}"
    );
}
