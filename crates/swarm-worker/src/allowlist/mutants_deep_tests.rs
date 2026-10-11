//! Тесты мутантов L9/L11/L12 — разделяющие кейсы «оператор-литерал в кавычках».
//!
//! УТОЧНЕНИЕ ПОСЛЕ ПРОГОНОВ НА ЖИВОМ КОДЕ (важно для карты мутантов):
//! кейсы вида `echo ok & echo evil` (оператор ВНЕ кавычек) мутанта L9/L11/L12
//! НЕ убивают: там символ вне кавычек, и строка отклоняется разбором argv даже
//! после удаления оператора из `FORBIDDEN_OPERATORS`. Убивает только кейс, где
//! оператор — ЛИТЕРАЛ В КАВЫЧКАХ внутри вложенной строки (`bash -c 'echo "a & b"'`):
//! разбор его пропускает (символ внутри кавычек), удерживает лишь слой списка.
//!
//! НЕ ПУТАТЬ с предложением Gemini («l9_background_and_operator_is_rejected_early»):
//! его формулировка утверждала, что операторы вне кавычек убивают мутанта — это
//! неверно, такие кейсы не различают слой и дают ложное чувство покрытия.
use super::Allowlist;

fn no(al: &Allowlist, c: &str) {
    assert!(
        al.check(c).is_err(),
        "команда обязана быть отклонена: {c:?}"
    );
}
fn ok(al: &Allowlist, c: &str) {
    assert!(al.check(c).is_ok(), "команда обязана быть разрешена: {c:?}");
}

/// L9: мутант убирает `&` из `FORBIDDEN_OPERATORS`.
/// Убивает только оператор-ЛИТЕРАЛ в кавычках — разбор его пропускает.
#[test]
fn l9_ampersand_killed_only_by_quoted_literal() {
    let al = Allowlist::default();
    // различающий кейс: `&` внутри вложенной строки, но в кавычках
    no(&al, "bash -c 'echo \"a & b\"'");
    no(&al, "bash -c \"echo 'a && b'\"");
}

/// L11: мутант убирает `|`.
#[test]
fn l11_pipe_killed_only_by_quoted_literal() {
    let al = Allowlist::default();
    no(&al, "bash -c 'echo \"a | b\"'");
    no(&al, "bash -c \"echo 'x || y'\"");
}

/// L12: мутант убирает `>` (и `<`).
#[test]
fn l12_redirection_killed_only_by_quoted_literal() {
    let al = Allowlist::default();
    no(&al, "bash -c 'echo \"a > b\"'");
    no(&al, "bash -c 'echo \"a < b\"'");
}

/// Контроль политики: опасные случаи (вне кавычек) отклоняются, как и раньше —
/// но подчёркиваем, что это НЕ доказательство убийства мутанта L9/L11/L12.
#[test]
fn dangerous_out_of_quote_forms_still_rejected() {
    let al = Allowlist::default();
    no(&al, "echo ok & echo evil");
    no(&al, "cargo test & rm -rf target");
    no(&al, "cat Cargo.toml | sh");
    no(&al, "echo evil > /workspace/.cargo/config.toml");
}

/// Положительный контроль: литералы БЕЗ операторов не должны ложно отсекаться
/// сверх политики. `echo "a b"` (пробел в кавычках) проходит.
#[test]
fn benign_quoted_literals_still_pass() {
    let al = Allowlist::default();
    ok(&al, "echo 'a b'");
    ok(&al, "git commit -m 'fix: a b'");
}

/// Арбитр песочницы: обход через флаги конфигурации cargo.
#[test]
fn cargo_extended_flags_and_aliases_are_rejected() {
    let al = Allowlist::default();
    no(
        &al,
        "cargo --config target.x86_64-unknown-linux-gnu.runner='sh' test",
    );
    no(&al, "cargo -Z build-std build");
    ok(&al, "cargo build --release");
    ok(&al, "cargo test --workspace");
    ok(&al, "cargo clippy -- -D warnings");
}
