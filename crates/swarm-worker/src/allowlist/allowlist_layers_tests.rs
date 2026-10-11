//! Тесты слоёв allowlist: каждый убивает ровно один одиночный мутант.
//! Подключение: `#[cfg(test)] mod allowlist_layers_tests;` рядом с allowlist.rs
//! (или в `mod tests`). Используется только публичный API: Allowlist::{default, from_env_str, check}.
//!
//! Мутанты (проверены запуском на живом allowlist.rs, по одному):
//!   L1  убрана проверка EXPLICIT_DENY            -> убивает l1_*
//!   L7  убрана проверка членства в allowlist     -> убивает l7_*
//!   L8  убран запрет перевода строки '\n'        -> убивает l8_*
//!   L10 убраны запреты '$(' и '${'               -> убивает l10_*
//!
//! Про L6/L9/L11/L12 (уточнено после проб на живом коде, обе стороны правы):
//! они безопасно ДУБЛИРУЮТСЯ разбором для опасных случаев (оператор вне
//! кавычек: `bash -c "echo a & rm x"` отклоняется и без слоя списка — уже на
//! токенизации), но различаются для ОПЕРАТОРА-ЛИТЕРАЛА В КАВЫЧКАХ
//! (`bash -c 'echo "a & b"'`): разбор его пропускает, удерживает лишь слой
//! FORBIDDEN_OPERATORS. Убить L9/L11/L12 можно только такими кейсами (см.
//! mutants_deep_tests.rs), и это закрепляет КОНСЕРВАТИВНУЮ политику: честное
//! `echo "a > b"` тоже отклоняется. Политика выбрана осознанно (гейт security,
//! ложный отказ дешевле обхода). L6 (глубина) — оставлен как есть: без
//! обратного слэша вложенность кавычек не глубже двух.
use super::Allowlist;

fn ok(al: &Allowlist, c: &str) {
    assert!(al.check(c).is_ok(), "должно быть разрешено: {c:?}");
}
fn no(al: &Allowlist, c: &str) {
    assert!(al.check(c).is_err(), "должно быть отклонено: {c:?}");
}

/// Положительный контроль: набор «всё отклоняем» эти тесты не пройдёт.
#[test]
fn controls_legit_commands_pass() {
    let al = Allowlist::default();
    for c in [
        "echo hello",
        "cat Cargo.toml",
        "ls -la",
        "cargo build",
        "cargo test",
        "git status",
        "git diff",
        "bash -c \"echo ok\"",
    ] {
        ok(&al, c);
    }
}

/// L7: команды, которых нет ни в deny-списке, ни в allowlist. Убирает только проверка членства.
#[test]
fn l7_unlisted_programs_are_rejected_by_membership() {
    let al = Allowlist::default();
    for c in [
        "python3 -c pass",
        "find . -name x",
        "perl -e 1",
        "node -e 1",
        "ruby -e 1",
        "awk 1 file",
    ] {
        no(&al, c);
    }
}

/// L1: deny-список обязан побеждать allowlist. Включаем `rm` в allowlist явно:
/// без слоя EXPLICIT_DENY команда прошла бы.
#[test]
fn l1_explicit_deny_beats_allowlist() {
    let al = Allowlist::from_env_str("rm,cat,ls,echo");
    no(&al, "rm -rf x");
    ok(&al, "cat Cargo.toml"); // контроль: allowlist при этом работает
}

/// L8: перевод строки внутри `bash -c` исполнился бы как вторая команда.
#[test]
fn l8_newline_inside_bash_c_is_rejected() {
    let al = Allowlist::default();
    no(&al, "bash -c \"echo a\nrm -rf x\"");
    no(&al, "bash -c 'echo a\rrm -rf x'");
}

/// L10: подстановка внутри двойных кавычек во вложенной строке: парсер её пропускает,
/// удерживает только список операторов.
#[test]
fn l10_command_substitution_in_nested_quotes_is_rejected() {
    let al = Allowlist::default();
    no(&al, "bash -c 'echo \"$(rm -rf x)\"'");
    no(&al, "bash -c 'echo \"${HOME}\"'");
    no(&al, "bash -c 'echo \"`id`\"'");
}

/// Арбитр для белого списка подкоманд git/cargo (v0.5.2). Красный на v0.5.1 (git config проходит),
/// зелёный после правки. Это поведенческий тест, не проверка текста.
#[test]
fn git_and_cargo_config_injection_vectors_are_rejected() {
    let al = Allowlist::default();
    for c in [
        "git config core.pager sh",
        "git config --global alias.x '!id'",
        "git -c core.pager=sh log",
        "cargo --config build.rustc-wrapper=sh build",
        "cargo -Z unstable-options build",
    ] {
        no(&al, c);
    }
    // контроль: безопасные подкоманды живы
    for c in [
        "git status",
        "git log",
        "git diff",
        "git add .",
        "git commit -m x",
        "cargo build",
        "cargo test",
    ] {
        ok(&al, c);
    }
}
