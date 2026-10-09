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
const FORBIDDEN_OPERATORS: &[&str] = &["|", ">", "<", ";", "&", "`", "$(", "${", "\n", "\r"];

/// Оболочки, для которых делается рекурсивная проверка `-c`.
const SHELLS: &[&str] = &["bash", "sh", "dash", "zsh"];

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

        // 2. Обход каталога.
        if cmd.split_whitespace().any(|t| t.contains("..")) {
            return Err(WorkerError::AllowlistRejected {
                command: command.to_string(),
                reason: "путь с '..' запрещён".into(),
            });
        }

        let tokens: Vec<&str> = cmd.split_whitespace().collect();
        let first = tokens[0].to_lowercase();

        // 3. Явный deny (в т.ч. абсолютный путь: /bin/rm → rm).
        let base = first.rsplit('/').next().unwrap_or(&first);
        if first.contains('/') && !trusted_executable(&first) {
            return Err(WorkerError::AllowlistRejected {
                command: command.to_string(),
                reason: "не доверенный путь executable".into(),
            });
        }
        if EXPLICIT_DENY.contains(&base) {
            return Err(WorkerError::AllowlistRejected {
                command: command.to_string(),
                reason: format!("команда '{base}' в явном deny-списке"),
            });
        }

        // 4. Allowlist по первому слову.
        if !self.allows(base) {
            return Err(WorkerError::AllowlistRejected {
                command: command.to_string(),
                reason: format!("'{base}' нет в allowlist"),
            });
        }

        // 5. Рекурсия для `bash -c "<внутренняя>"`.
        if SHELLS.contains(&base) {
            let Some(inner) = extract_dash_c(&tokens) else {
                return Err(WorkerError::AllowlistRejected {
                    command: command.to_string(),
                    reason: "shell разрешён только с однозначным -c".into(),
                });
            };
            return self.check_depth(&inner, depth + 1);
        }

        if tokens.iter().any(|t| t.contains('"') || t.contains('\'')) {
            return Err(WorkerError::AllowlistRejected {
                command: command.to_string(),
                reason: "неоднозначное quoting запрещено".into(),
            });
        }

        Ok(())
    }
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
        assert!(a.check("cargo x").is_ok());
        assert!(a.check("git y").is_ok());
        assert!(a.check("bash z").is_err(), "bash не в кастомном списке");
        // пусто → default; shell без явного -c запрещён.
        assert!(Allowlist::from_env_str("").check("bash x").is_err());
    }

    #[test]
    fn allowlist_empty_command_rejected() {
        assert!(al().check("").is_err());
        assert!(al().check("   ").is_err());
    }
}
