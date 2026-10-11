//! Allowlist команд для sandbox (ТЗ v0.4 §4).
//!
//! **Deny by default.** Воркер запускает только команды, чьё первое слово есть
//! в allowlist. Allowlist задаётся через env `WORKER_ALLOWED_COMMANDS`
//! (comma-separated) и **никогда** не расширяется из LLM.
//!
//! Дополнительно (сверх ТЗ, защита в глубину):
//! - явный deny-список (`rm`, `sudo`, `curl`, …) — даже если попадёт в env;
//! - запрет shell-операторов (`>`, `|`, `;`, `&&`, `` ` ``, `$(`) — иначе
//!   `echo x; rm -rf /` обошёл бы проверку первого слова;
//! - запрет `..` в любом месте команды (выход из /workspace);
//! - **рекурсивная проверка** `bash -c "<команда>"` / `sh -c` — иначе
//!   `bash -c "rm -rf /"` прошёл бы по первому слову `bash`.

use crate::error::{Result, WorkerError};

/// Allowlist по умолчанию (ТЗ v0.4 §4).
pub const DEFAULT_ALLOWED: &[&str] = &[
    "bash", "cargo", "git", "cat", "ls", "echo", "mkdir", "touch",
];

/// Явно запрещённые команды — даже если попадут в env.
pub const EXPLICIT_DENY: &[&str] = &[
    "rm", "rmdir", "sudo", "su", "doas", "curl", "wget", "nc", "ncat", "netcat", "ssh", "scp",
    "sftp", "telnet", "dd", "mkfs", "shutdown", "reboot", "halt", "poweroff", "mount", "umount",
    "chmod", "chown", "chgrp", "kill", "pkill", "killall", "eval", "exec", "source", "apt",
    "apt-get", "dpkg", "pacman",
];

/// Shell-операторы, запрещённые в v0.4 (до analysis pipelines, ТЗ §4).
const FORBIDDEN_OPERATORS: &[&str] = &["|", ">", "<", ";", "&", "`", "$(", "${", "\n", "\r", "\\"];

/// Оболочки, для которых делается рекурсивная проверка `-c`.
const SHELLS: &[&str] = &["bash", "sh", "dash", "zsh"];

/// F11: флаги/опции, которыми `git`/`cargo` можно заставить выполнить
/// произвольный код в обход deny-списка и allowlist.
///
/// `git -c alias.x='!rm -rf /' x`, `git --exec-path=/tmp ...`,
/// `cargo --config build.rustc-wrapper=sh ...`, `cargo -Z...`. Даже при
/// наличии `git`/`cargo` в allowlist такие опции открывают RCE.
const DENIED_GIT_OPTS: &[&str] = &[
    "-c",
    "--config",
    "--exec-path",
    "--upload-pack",
    "--receive-pack",
];
const DENIED_CARGO_OPTS: &[&str] = &["--config", "-Z", "--rustc", "--wrapper"];

/// F11-доп (Волна 5.2): строгий whitelist ПОДКОМАНД `git`.
///
/// Чёрного списка опций недостаточно: `git config core.pager "sh -c ..."`
/// проходит проверку опций (второй токен `config` — не опция), а `git config`
/// умеет исполнять `alias.*=!...` и переопределять pager/hooks, то есть даёт
/// RCE в обход deny-списка команд. Поэтому переходим от blacklist к whitelist:
/// разрешены только заведомо безопасные read-only подкоманды, всё остальное
/// отклоняется по умолчанию (deny-by-default, как и весь sandbox).
const ALLOWED_GIT_SUBCMDS: &[&str] = &[
    "status",
    "diff",
    "log",
    "show",
    "rev-parse",
    "describe",
    "ls-files",
    "blame",
    "branch",
    "tag",
    "remote",
    "stash",
    "add",
    "commit",
    "checkout",
    "switch",
    "restore",
    "merge",
    "rebase",
    "cherry-pick",
    "revert",
    "reset",
    "push",
    "fetch",
    "pull",
    "clone",
    "init",
    "apply",
    "format-patch",
    "am",
];

/// F11-доп: строгий whitelist ПОДКОМАНД `cargo`.
///
/// `cargo` подкоманд-исполняемых обходов почти не имеет (его вектор — опции
/// `--config/-Z/--rustc/--wrapper`, уже закрытые), но whitelist убирает риск
/// будущих/неизвестных подкоманд вроде `cargo xtask` (произвольный код из
/// репозитория).
const ALLOWED_CARGO_SUBCMDS: &[&str] = &[
    "build", "check", "test", "run", "bench", "clippy", "fmt", "doc", "metadata", "tree", "clean",
    "update", "fetch", "add", "remove", "fix",
];

/// Максимальная глубина рекурсии для `bash -c "..."`.
const MAX_SHELL_DEPTH: usize = 3;

/// Allowlist команд.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Allowlist {
    allowed: Vec<String>,
}

impl Default for Allowlist {
    fn default() -> Self {
        Self::from_list(DEFAULT_ALLOWED)
    }
}

impl Allowlist {
    /// Собрать из списка (проверка нормализует регистр).
    pub fn from_list(items: &[&str]) -> Self {
        Self {
            allowed: items
                .iter()
                .map(|s| s.trim().to_lowercase())
                .filter(|s| !s.is_empty())
                .collect(),
        }
    }

    /// Собрать из строки env (comma-separated). Пусто → default.
    pub fn from_env_str(s: &str) -> Self {
        let items: Vec<&str> = s
            .split(',')
            .map(str::trim)
            .filter(|x| !x.is_empty())
            .collect();
        if items.is_empty() {
            Self::default()
        } else {
            Self::from_list(&items)
        }
    }

    /// Из окружения `WORKER_ALLOWED_COMMANDS` (или default).
    pub fn from_env() -> Self {
        match std::env::var("WORKER_ALLOWED_COMMANDS") {
            Ok(v) => Self::from_env_str(&v),
            Err(_) => Self::default(),
        }
    }

    /// Разрешено ли первое слово (без рекурсии). Публично для тестов.
    pub fn allows(&self, command: &str) -> bool {
        self.allowed.iter().any(|a| a == command)
    }

    /// Список разрешённых команд (для промпта Agent Loop).
    pub fn list(&self) -> Vec<String> {
        self.allowed.clone()
    }

    /// Полная проверка команды. `Ok(())` — можно запускать.
    pub fn check(&self, command: &str) -> Result<()> {
        self.check_depth(command, 0)
    }

    fn check_depth(&self, command: &str, depth: usize) -> Result<()> {
        let cmd = command.trim();
        if cmd.is_empty() {
            return Err(WorkerError::AllowlistRejected {
                command: command.to_string(),
                reason: "пустая команда".into(),
            });
        }
        if depth > MAX_SHELL_DEPTH {
            return Err(WorkerError::AllowlistRejected {
                command: command.to_string(),
                reason: "слишком глубокая вложенность shell -c".into(),
            });
        }

        // 1. Shell-операторы.
        for op in FORBIDDEN_OPERATORS {
            if cmd.contains(op) {
                return Err(WorkerError::AllowlistRejected {
                    command: command.to_string(),
                    reason: format!("запрещённый shell-оператор: {op:?}"),
                });
            }
        }

        // 2. Обход каталога + токенизация ЕДИНЫМ парсером (G2):
        //    quote-aware argv-парсер, тот же, что применяет sandbox.exec.
        //    Раньше allowlist шёл через split_whitespace, а sandbox — через
        //    собственный quote-aware парсер; расхождение на quoting было
        //    okном для обхода. Теперь источник истины один.
        let tokens: Vec<String> = parse_command_argv(cmd)?;
        if tokens.iter().any(|t| t.contains("..")) {
            return Err(WorkerError::AllowlistRejected {
                command: command.to_string(),
                reason: "путь с '..' запрещён".into(),
            });
        }

        let first = tokens[0].to_lowercase();

        // 3. Явный deny (в т.ч. абсолютный путь: /bin/rm → rm).
        let base = first.rsplit('/').next().unwrap_or(&first).to_string();
        if first.contains('/') && !trusted_executable(&first) {
            return Err(WorkerError::AllowlistRejected {
                command: command.to_string(),
                reason: "не доверенный путь executable".into(),
            });
        }
        if EXPLICIT_DENY.contains(&base.as_str()) {
            return Err(WorkerError::AllowlistRejected {
                command: command.to_string(),
                reason: format!("команда '{base}' в явном deny-списке"),
            });
        }

        // 4. Allowlist по первому слову.
        if !self.allows(&base) {
            return Err(WorkerError::AllowlistRejected {
                command: command.to_string(),
                reason: format!("'{base}' нет в allowlist"),
            });
        }

        // 5. Рекурсия для `bash -c "<внутренняя>"`.
        if SHELLS.contains(&base.as_str()) {
            let refs: Vec<&str> = tokens.iter().map(String::as_str).collect();
            let Some(inner) = extract_dash_c(&refs) else {
                return Err(WorkerError::AllowlistRejected {
                    command: command.to_string(),
                    reason: "shell разрешён только с однозначным -c".into(),
                });
            };
            return self.check_depth(&inner, depth + 1);
        }

        // 6. F11: конфигурационные опции git/cargo, позволяющие исполнить
        //    произвольный код в обход deny-списка.
        check_vcs_tool_options(&base, &tokens, command)?;
        // 7. F11-доп (Волна 5.2): строгий whitelist подкоманд. Опций мало:
        //    `git config core.pager "sh -c ..."` опций не содержит, но даёт RCE.
        check_vcs_tool_subcommands(&base, &tokens, command)?;

        Ok(())
    }
}

/// F11-доп (Волна 5.2): whitelist подкоманд `git`/`cargo`.
///
/// Закрывает вектор, который остался после F11: `git config` — это подкоманда,
/// а не опция, поэтому `check_vcs_tool_options` её не видит. Между тем
/// `git config core.pager "sh -c '...'"` или `git config alias.x '!...'`
/// исполняют произвольный код. Здесь разрешаем только известные безопасные
/// подкоманды (deny-by-default), а неизвестные/опасные (`config`, `daemon`,
/// `credential`, `filter-branch`, `submodule`, `xtask`…) отклоняем.
fn check_vcs_tool_subcommands(base: &str, tokens: &[String], command: &str) -> Result<()> {
    let allowed: &[&str] = match base {
        "git" => ALLOWED_GIT_SUBCMDS,
        "cargo" => ALLOWED_CARGO_SUBCMDS,
        _ => return Ok(()),
    };
    // Первый токен — сама команда; ищем подкоманду (первое слово без '-'),
    // пропуская глобальные опции И их аргументы (`git -C dir status`).
    let mut skip_next = false;
    let mut sub: Option<&str> = None;
    for t in tokens.iter().skip(1) {
        let t = t.as_str();
        if skip_next {
            skip_next = false;
            continue;
        }
        if t.starts_with('-') {
            // Опции, принимающие значение отдельным токеном.
            if matches!(t, "-C" | "-c" | "--git-dir" | "--work-tree" | "--namespace") {
                skip_next = true;
            }
            continue;
        }
        sub = Some(t);
        break;
    }
    let Some(sub) = sub else {
        // Нет подкоманды (например, `git` без аргументов) — не наша забота.
        return Ok(());
    };
    // Отбрасываем возможный путь: подкоманда вида `./evil` отклоняется.
    let sub_lc = sub.to_ascii_lowercase();
    if sub.contains('/') || !allowed.iter().any(|a| *a == sub_lc.as_str()) {
        return Err(WorkerError::AllowlistRejected {
            command: command.to_string(),
            reason: format!("подкоманда {base} '{sub}' не в whitelist"),
        });
    }
    Ok(())
}

/// F11: запрет опасных опций `git`/`cargo` (config/exec-path/-Z/...).
///
/// Такие опции позволяют выполнить произвольный код (`git -c alias.x=!sh`,
/// `cargo --config build.rustc-wrapper=...`), обходя deny-список команд.
/// Проверяем и точное совпадение опции, и форму `--opt=value`, и `-cvalue`.
fn check_vcs_tool_options(base: &str, tokens: &[String], command: &str) -> Result<()> {
    let denied: &[&str] = match base {
        "git" => DENIED_GIT_OPTS,
        "cargo" => DENIED_CARGO_OPTS,
        _ => return Ok(()),
    };
    // Первый токен — сама команда; смотрим остальные.
    for t in tokens.iter().skip(1) {
        // Длинные опции сравниваем без учёта регистра (у них регистр незначим).
        // КОРОТКИЕ флаги — С УЧЁТОМ РЕГИСТРА (N1): иначе `git -C <dir>`
        // (сменить каталог, легитимно) ложно совпадал с запрещённым `-c`
        // после приведения к нижнему регистру и отклонялся.
        let raw_opt = t.split('=').next().unwrap_or(t);
        let opt = raw_opt.to_ascii_lowercase();
        for d in denied {
            let is_short = d.len() == 2 && d.starts_with('-') && !d.starts_with("--");
            let matched = if is_short {
                // `-c` запрещён, но `-C` разрешён; `-cfoo` (слитная форма) запрещён.
                raw_opt == *d || (raw_opt.starts_with(d) && raw_opt.len() > d.len())
            } else {
                opt == d.to_ascii_lowercase()
            };
            if matched {
                return Err(WorkerError::AllowlistRejected {
                    command: command.to_string(),
                    reason: format!("запрещённая опция {base} '{t}'"),
                });
            }
        }
    }
    Ok(())
}

/// Единый argv-парсер команд (G2).
///
/// Разбирает небольшой argv-язык: пробелы разделяют токены, одинарные и
/// двойные кавычки группируют; shell-операторы и escape **отвергаются**, а не
/// интерпретируются. Один и тот же парсер использует и allowlist-политика, и
/// `sandbox.exec` — чтобы не было двух расходящихся мнений о том, где границы
/// токенов.
///
/// Возвращает `Err`, если обнаружен запрещённый символ (`; | & < > ` $ \`),
/// незакрытая кавычка или пустая команда.
pub fn parse_command_argv(command: &str) -> Result<Vec<String>> {
    let mut argv = Vec::new();
    let mut token = String::new();
    let mut quote = None;
    let mut started = false;
    for ch in command.chars() {
        match quote {
            Some(q) if ch == q => quote = None,
            Some(_) => token.push(ch),
            None if ch == '\'' || ch == '"' => {
                quote = Some(ch);
                started = true;
            }
            None if ch.is_whitespace() => {
                if started {
                    argv.push(std::mem::take(&mut token));
                    started = false;
                }
            }
            None if matches!(ch, ';' | '|' | '&' | '<' | '>' | '`' | '$' | '\\') => {
                return Err(WorkerError::AllowlistRejected {
                    command: command.to_string(),
                    reason: format!("запрещённый shell-символ: {ch:?}"),
                });
            }
            None => {
                token.push(ch);
                started = true;
            }
        }
    }
    if quote.is_some() {
        return Err(WorkerError::AllowlistRejected {
            command: command.to_string(),
            reason: "незакрытая кавычка".into(),
        });
    }
    if started {
        argv.push(token);
    }
    if argv.is_empty() {
        return Err(WorkerError::AllowlistRejected {
            command: command.to_string(),
            reason: "пустая команда".into(),
        });
    }
    Ok(argv)
}

fn trusted_executable(path: &str) -> bool {
    matches!(
        path,
        "/bin/bash" | "/usr/bin/bash" | "/bin/sh" | "/usr/bin/sh"
    )
}

/// Извлечь аргумент `-c` из токенов (`["bash","-c","rm -rf /"]` → `rm -rf /`).
///
/// Поддерживает `-c "cmd"` (уже разбито по пробелам — склеиваем хвост) и
/// `-c cmd`.
fn extract_dash_c(tokens: &[&str]) -> Option<String> {
    // Reject combined flags (-lc/-xc), script operands and options before -c.
    if tokens.len() < 3 || tokens[1] != "-c" || tokens.iter().filter(|t| **t == "-c").count() != 1 {
        return None;
    }
    let rest = &tokens[2..];
    if rest.is_empty() {
        return None;
    }
    // Склеиваем оставшееся: кавычки снимает shell, но в тестах строка приходит
    // уже разбитой, поэтому восстанавливаем по пробелу.
    let mut s = rest.join(" ");
    s = s.trim().to_string();
    // Снять обёртывающие кавычки: `"cargo test"` → `cargo test`.
    let quoted =
        (s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\''));
    if quoted && s.len() >= 2 {
        s = s[1..s.len() - 1].to_string();
    }
    if s.trim().is_empty() {
        return None;
    }
    Some(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn al() -> Allowlist {
        Allowlist::default()
    }

    // §8.1.1
    #[test]
    fn allowlist_accepts_cargo() {
        assert!(al().check("cargo test").is_ok());
        assert!(al().check("cargo build --release").is_ok());
        assert!(al().check("git status").is_ok());
        assert!(al().check("ls -la").is_ok());
    }

    // §8.1.2
    #[test]
    fn allowlist_rejects_rm() {
        let e = al().check("rm -rf /").unwrap_err();
        assert!(
            matches!(e, WorkerError::AllowlistRejected { .. }),
            "got {e:?}"
        );
        assert!(al().check("rm file.txt").is_err());
        // абсолютный путь тоже ловится
        assert!(al().check("/bin/rm -rf /").is_err());
    }

    // §8.1.3
    #[test]
    fn allowlist_rejects_sudo() {
        assert!(al().check("sudo ls").is_err());
        assert!(al().check("su - root").is_err());
    }

    // §8.1.4
    #[test]
    fn allowlist_rejects_network_cmds() {
        for c in [
            "curl http://x",
            "wget http://x",
            "nc -l 1234",
            "ssh host",
            "scp a b",
        ] {
            assert!(al().check(c).is_err(), "{c} должен быть отклонён");
        }
    }

    // Сверх ТЗ: shell-операторы (иначе `echo x; rm -rf /` обходит проверку).
    #[test]
    fn allowlist_rejects_shell_operators() {
        for c in [
            "ls > out.txt",
            "ls | grep x",
            "echo a; rm -rf /",
            "echo a && rm -rf /",
            "echo `whoami`",
            "echo $(whoami)",
        ] {
            assert!(al().check(c).is_err(), "{c} должен быть отклонён");
        }
    }

    // Сверх ТЗ: обход через `bash -c`.
    #[test]
    fn allowlist_rejects_bash_c_bypass() {
        assert!(
            al().check("bash -c \"rm -rf /\"").is_err(),
            "bash -c rm должен быть отклонён"
        );
        assert!(al().check("bash -c \"curl http://x\"").is_err());
        // легитимный bash -c с разрешённой командой проходит
        assert!(al().check("bash -c \"cargo test\"").is_ok());
    }

    // Сверх ТЗ: выход из /workspace.
    #[test]
    fn allowlist_rejects_path_traversal() {
        assert!(al().check("cat ../../etc/passwd").is_err());
        assert!(al().check("ls ../").is_err());
    }

    #[test]
    fn allowlist_from_env_str() {
        let a = Allowlist::from_env_str("cargo, git ,ls");
        // F11-доп: whitelist подкоманд сузил «любой cargo/git» до известных
        // подкоманд, поэтому проверяем на реальных.
        assert!(a.check("cargo test").is_ok());
        assert!(a.check("git status").is_ok());
        assert!(a.check("bash z").is_err(), "bash не в кастомном списке");
        // пусто → default; shell без явного -c запрещён.
        assert!(Allowlist::from_env_str("").check("bash x").is_err());
    }

    #[test]
    fn allowlist_empty_command_rejected() {
        assert!(al().check("").is_err());
        assert!(al().check("   ").is_err());
    }

    #[test]
    fn allowlist_rejects_shell_options_and_escaped_operators() {
        assert!(al().check("bash -lc 'rm -rf /'").is_err());
        assert!(al().check("echo foo\\; rm -rf /").is_err());
        assert!(al().check("echo ${HOME}").is_err());
    }

    // P0-1: shell-форма без однозначного `-c` — скрипт, интерактив, голый shell.
    #[test]
    fn allowlist_rejects_shell_script_operand() {
        for c in [
            "bash script.sh", // script operand вместо -c
            "sh /tmp/x.sh",
            "bash",    // голый shell
            "bash -i", // интерактивный shell
            "bash --login",
        ] {
            assert!(al().check(c).is_err(), "{c} должен быть отклонён");
        }
    }

    // P0-1: проверка доверенного executable path, а не только basename.
    #[test]
    fn allowlist_rejects_untrusted_executable_path() {
        assert!(
            al().check("/tmp/cargo build").is_err(),
            "basename cargo, но путь недоверенный"
        );
        assert!(al().check("./cargo build").is_err());
        assert!(al().check("/usr/local/bin/git status").is_err());
        assert!(al().check("../cargo build").is_err());
        // доверенный путь shell с -c проходит
        assert!(al().check("/bin/bash -c \"cargo test\"").is_ok());
    }

    // P0-1: комбинированные флаги и env-подстановки.
    #[test]
    fn allowlist_rejects_combined_flags_and_env_substitution() {
        assert!(al().check("bash -xc 'cargo test'").is_err());
        assert!(al().check("bash -ec 'cargo test'").is_err());
        assert!(al().check("cat ${HOME}/.ssh/id_rsa").is_err());
        assert!(al().check("cat $(whoami)").is_err());
    }

    // G2 (audit): политика и sandbox используют ОДИН argv-парсер.
    // Регрессия: до унификации allowlist шёл через split_whitespace и не
    // видел, что `echo "hello world"` — это один токен; теперь видит.
    #[test]
    fn single_argv_parser_shared_with_sandbox() {
        assert_eq!(
            parse_command_argv("echo 'hello world'").unwrap(),
            vec!["echo".to_string(), "hello world".to_string()]
        );
        // Кавычки больше не считаются «неоднозначным quoting» — это часть
        // единого argv-языка, а не признак инъекции.
        assert!(al().check("echo 'a b'").is_ok());
        // А shell-символы внутри/вне кавычек по-прежнему отсекаются.
        assert!(al().check("echo 'a; rm -rf /'").is_err());
        assert!(al().check("echo `id`").is_err());
    }

    // ---------- F11: конфигурационные опции git/cargo ----------

    #[test]
    fn f11_rejects_git_config_option() {
        // git -c alias.x=!rm -rf / x  -> RCE обход
        assert!(
            al().check("git -c alias.x=!rm -rf / x").is_err(),
            "git -c должен быть отклонён"
        );
        assert!(al().check("git --config core.pager=sh status").is_err());
    }

    #[test]
    fn f11_rejects_git_exec_path() {
        assert!(al().check("git --exec-path=/tmp status").is_err());
        assert!(al().check("git --upload-pack=/tmp/evil fetch").is_err());
    }

    #[test]
    fn f11_rejects_cargo_config_option() {
        // cargo --config build.rustc-wrapper=sh build -> RCE
        assert!(
            al().check("cargo --config build.rustc-wrapper=sh build")
                .is_err(),
            "cargo --config должен быть отклонён"
        );
        assert!(al().check("cargo -Z build-std build").is_err());
    }

    #[test]
    fn f11_allows_plain_git_and_cargo() {
        assert!(al().check("git status").is_ok());
        assert!(al().check("git commit -m 'x'").is_ok());
        assert!(al().check("cargo test").is_ok());
        assert!(al().check("cargo build --release").is_ok());
        // обычные флаги не задеваются
        assert!(al().check("git log --oneline -5").is_ok());
        assert!(al().check("cargo test --features shell-tools").is_ok());
    }

    // ---------- F11-доп (Волна 5.2): whitelist подкоманд ----------

    #[test]
    fn f11d_rejects_git_config_subcommand() {
        // Вектор из аудита: подкоманда `config` исполняет `alias.x=!...` и
        // переопределяет pager -> RCE. Опций здесь нет, поэтому старый
        // check_vcs_tool_options его пропускал.
        assert!(
            al().check("git config core.pager 'sh -c evil'").is_err(),
            "git config должен быть отклонён как подкоманда"
        );
        assert!(al().check("git config alias.x '!rm -rf /'").is_err());
        // Глобальная опция перед подкомандой не должна создавать лазейку.
        assert!(al().check("git -C /workspace config user.name x").is_err());
    }

    #[test]
    fn f11d_rejects_dangerous_git_subcommands() {
        for cmd in [
            "git daemon --reuseaddr --base-path=/ --export-all",
            "git credential fill",
            "git filter-branch --tree-filter 'sh -c evil' HEAD",
            "git submodule add /tmp/evil",
        ] {
            assert!(al().check(cmd).is_err(), "должно быть отклонено: {cmd}");
        }
    }

    #[test]
    fn f11d_rejects_non_whitelisted_cargo_subcommands() {
        // `cargo xtask` исполняет произвольный код из репозитория.
        assert!(al().check("cargo xtask evil").is_err());
        assert!(al().check("cargo install evil-crate").is_err());
    }

    #[test]
    fn f11d_allows_whitelisted_subcommands_with_global_opts() {
        // Глобальные опции и легитимные подкоманды не задеваются.
        assert!(al().check("git -C /workspace status").is_ok());
        assert!(al().check("git --no-pager log").is_ok());
        assert!(al().check("cargo test --release").is_ok());
        assert!(al().check("cargo clippy -- -D warnings").is_ok());
    }
}
