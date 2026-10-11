//! Волна 5.1 — runtime-гейт shell-tools и ложное срабатывание `git -C`.
//!
//! FIND-01: компиляционный бан shell-tools снят (F6/F7/F11/F12 закрыты),
//! поэтому включение переведено на явный runtime-флаг `WORKER_SHELL_TOOLS=1`.
//! Эти тесты не требуют Docker/PostgreSQL: они проверяют сам гейт и политику
//! allowlist.

use swarm_worker::allowlist::Allowlist;
use swarm_worker::sandbox::{shell_tools_enabled, Sandbox, SandboxConfig};

/// Без `WORKER_SHELL_TOOLS` гейт закрыт.
#[test]
fn gate_off_by_default() {
    std::env::remove_var("WORKER_SHELL_TOOLS");
    assert!(!shell_tools_enabled(), "без флага гейт должен быть закрыт");
}

/// Включается строго значением `1`; прочие значения — запрет.
#[test]
fn gate_on_strictly_one() {
    for (val, want) in [
        ("1", true),
        ("0", false),
        ("true", false),
        ("yes", false),
        ("", false),
    ] {
        std::env::set_var("WORKER_SHELL_TOOLS", val);
        assert_eq!(
            shell_tools_enabled(),
            want,
            "WORKER_SHELL_TOOLS={val:?} должен давать {want}"
        );
    }
    std::env::remove_var("WORKER_SHELL_TOOLS");
}

/// RED-фаза: без флага `Sandbox::start` обязан отказать ДО обращения к Docker.
#[test]
fn sandbox_start_refused_without_flag() {
    std::env::remove_var("WORKER_SHELL_TOOLS");
    let cfg = SandboxConfig {
        name_prefix: "swarm-sandbox-gate".into(),
        ..Default::default()
    };
    let err = match Sandbox::start(&cfg, "deadbeef") {
        Ok(_) => panic!("без флага старт запрещён"),
        Err(e) => e,
    };
    let msg = format!("{err}");
    assert!(
        msg.contains("WORKER_SHELL_TOOLS"),
        "отказ должен ссылаться на флаг, получено: {msg}"
    );
}

/// N3: префикс без worker_id (дефолтный) отклоняется — изоляция cleanup
/// не может быть «только документирована».
///
/// Проверяем логику `start` НАПРЯМУЮ через отдельную точку входа — здесь
/// мы гарантируем, что флаг включён и при этом дефолтный префикс всё равно отказ.
/// Гонки с другими тестами нет: `--test-threads=1` в harness, а здесь мы
/// запускаем оба теста флага и префикса в ОДНОМ тесте.
#[test]
fn sandbox_start_refused_on_default_prefix() {
    std::env::set_var("WORKER_SHELL_TOOLS", "1");
    let cfg = SandboxConfig::default(); // name_prefix = "swarm-sandbox"
    let err = match Sandbox::start(&cfg, "deadbeef") {
        Ok(_) => panic!("дефолтный префикс запрещён"),
        Err(e) => e,
    };
    let msg = format!("{err}");
    assert!(
        msg.contains("worker_id") || msg.contains("name_prefix"),
        "отказ должен объяснять причину, получено: {msg}"
    );
    std::env::remove_var("WORKER_SHELL_TOOLS");
}

/// N1: `git -C <dir>` — легитимная смена каталога и НЕ должна блокироваться,
/// хотя `git -c key=val` (config-инъекция) запрещена.
#[test]
fn n1_git_dash_c_path_is_allowed_but_lowercase_c_config_is_denied() {
    let al = Allowlist::default();
    // Легитимная смена каталога — разрешена.
    assert!(
        al.check("git -C /workspace status").is_ok(),
        "git -C <path> не должен блокироваться (N1)"
    );
    // Config-инъекция через -c — запрещена.
    assert!(
        al.check("git -c core.pager=sh status").is_err(),
        "git -c key=val должен блокироваться (F11)"
    );
}
