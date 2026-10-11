//! Интеграционные тесты watchdog (Волна 5.2).
//!
//! Дефект: `swarm-watchdog` (84 строки) не имел ни одного теста, хотя это
//! отдельный процесс, который восстанавливает истёкшие аренды — критичный
//! путь «воркер упал -> задача не залипает».
//!
//! Проверяем поведение СОБРАННОГО бинаря как процесса:
//!  1) без DATABASE_URL — падает ненулевым кодом (fail-closed конфиг);
//!  2) с живой БД — стартует, делает итерации и корректно завершается по SIGINT
//!     (graceful shutdown через ctrlc).
//!
//! Требует TEST_DATABASE_URL. Без него второй тест выходит с явным FAIL
//! (FIND-08: не молчаливый пропуск).

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn watchdog_bin() -> String {
    env!("CARGO_BIN_EXE_swarm-watchdog").to_string()
}

/// Без DATABASE_URL watchdog обязан упасть ненулевым кодом.
#[test]
fn watchdog_requires_database_url() {
    let out = Command::new(watchdog_bin())
        .env_remove("DATABASE_URL")
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .expect("spawn watchdog");
    assert!(
        !out.status.success(),
        "без DATABASE_URL watchdog должен завершиться ненулевым кодом"
    );
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("DATABASE_URL"),
        "сообщение должно упоминать DATABASE_URL, получено: {err}"
    );
}

/// С живой БД: стартует, крутит итерации и завершается по SIGINT с кодом 0.
#[test]
fn watchdog_starts_and_stops_gracefully_on_sigint() {
    let dsn = std::env::var("TEST_DATABASE_URL").unwrap_or_else(|_| {
        panic!(
            "BLOCKED/FAIL: TEST_DATABASE_URL не задан; watchdog-тест требует живую \
             тестовую БД (prefix test_/v0/c2_)"
        )
    });

    let mut child = Command::new(watchdog_bin())
        .env("DATABASE_URL", dsn)
        .env("WATCHDOG_INTERVAL", "1")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn watchdog");

    // Дать процессу стартовать и сделать хотя бы одну итерацию.
    std::thread::sleep(Duration::from_secs(2));

    // SIGINT -> graceful shutdown.
    let pid = child.id() as i32;
    unsafe {
        libc_kill(pid, 2); // SIGINT
    }

    // Ожидаем завершения с таймаутом.
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match child.try_wait().expect("try_wait") {
            Some(status) => {
                assert!(
                    status.success(),
                    "watchdog должен завершиться с кодом 0 по SIGINT, получено: {status:?}"
                );
                return;
            }
            None if Instant::now() > deadline => {
                let _ = child.kill();
                panic!("watchdog не завершился в течение 10s после SIGINT");
            }
            None => std::thread::sleep(Duration::from_millis(200)),
        }
    }
}

// Обёртка над libc::kill без дополнительной зависимости: используем команду
// `kill` из coreutils (доступна везде, где есть bash).
unsafe fn libc_kill(pid: i32, sig: i32) {
    let mut cmd = Command::new("kill");
    cmd.arg(format!("-{sig}")).arg(pid.to_string());
    let _ = cmd.status();
}
