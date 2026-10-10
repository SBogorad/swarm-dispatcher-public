//! Docker-sandbox для shell tools (ТЗ v0.4 §2–§3, §5).
//!
//! Один контейнер на задачу: `docker run -d ... sleep infinity`, затем
//! `docker exec` на каждую команду, `docker rm -f` после завершения.
//!
//! Изоляция (ТЗ §3): `--cap-drop ALL`, `--read-only`, `no-new-privileges`,
//! `--network none`, tmpfs для `/tmp` и `/workspace`, лимиты памяти и CPU.
//!
//! Только синхронный код (`std::process`), без async — как требует ТЗ §12.

use crate::allowlist::parse_command_argv;
use crate::error::{Result, WorkerError};
use sha2::{Digest, Sha256};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Конфигурация sandbox (ТЗ §3, §5).
#[derive(Debug, Clone)]
pub struct SandboxConfig {
    /// Образ песочницы (ТЗ §3).
    pub image: String,
    /// Префикс имени контейнера: `<prefix>-<task_short>`.
    pub name_prefix: String,
    /// Лимит памяти (`--memory`).
    pub memory: String,
    /// Лимит CPU (`--cpus`).
    pub cpus: String,
    /// Таймаут одной команды, сек (ТЗ §5).
    pub cmd_timeout_sec: u64,
    /// Лимит вывода stdout+stderr, байт (ТЗ §5, default 10 MB).
    pub max_output_bytes: usize,
    /// Сеть контейнера. В v0.4 — `none` (ТЗ §3).
    pub network: String,
    /// Бинарь docker.
    pub docker_bin: String,
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            image: "agent-sandbox:1.85".into(),
            name_prefix: "swarm-sandbox".into(),
            memory: "2g".into(),
            cpus: "2".into(),
            cmd_timeout_sec: 60,
            max_output_bytes: 10 * 1024 * 1024,
            network: "none".into(),
            docker_bin: "docker".into(),
        }
    }
}

/// Результат выполнения одной команды в sandbox.
#[derive(Debug, Clone)]
pub struct ExecOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub duration_ms: u128,
    pub truncated: bool,
    pub output_bytes: usize,
    pub output_sha256: String,
}

impl ExecOutput {
    /// Команда завершилась успешно.
    pub fn success(&self) -> bool {
        self.exit_code == 0
    }
}

/// Живой sandbox-контейнер. При `Drop` контейнер **не** удаляется автоматически:
/// остановка явная (`stop`), а брошенные контейнеры убирает `cleanup_stale`
/// (ТЗ §8.2 п.9 — переживание краха воркера).
pub struct Sandbox {
    name: String,
    cfg: SandboxConfig,
}

impl Sandbox {
    /// Собрать argv для `docker run` (публично — для unit-тестов изоляции).
    pub fn build_run_args(cfg: &SandboxConfig, name: &str) -> Vec<String> {
        vec![
            "run".into(),
            "-d".into(),
            "--rm".into(),
            "--name".into(),
            name.into(),
            "--security-opt".into(),
            "no-new-privileges:true".into(),
            "--cap-drop".into(),
            "ALL".into(),
            "--read-only".into(),
            "--tmpfs".into(),
            "/tmp:rw,noexec,nosuid,size=512m".into(),
            "--tmpfs".into(),
            // uid/gid/mode обязательны: без них tmpfs монтируется root:root 0755,
            // и пользователь 1001 не может писать в /workspace.
            // exec обязателен: cargo test запускает собранный бинарник из target/
            // (Docker по умолчанию добавляет noexec к --tmpfs).
            "/workspace:rw,exec,nosuid,size=1g,uid=1001,gid=1001,mode=0700".into(),
            "--network".into(),
            cfg.network.clone(),
            "--memory".into(),
            cfg.memory.clone(),
            "--cpus".into(),
            cfg.cpus.clone(),
            "-u".into(),
            "1001:1001".into(),
            "-e".into(),
            "HOME=/workspace".into(),
            // Корень образа read-only: кэш cargo должен жить в tmpfs.
            "-e".into(),
            "CARGO_HOME=/workspace/.cargo".into(),
            "-e".into(),
            "CARGO_TARGET_DIR=/workspace/target".into(),
            "-w".into(),
            "/workspace".into(),
            cfg.image.clone(),
            "sleep".into(),
            "infinity".into(),
        ]
    }

    /// Запустить sandbox для задачи. `task_short` — первые 12 символов id.
    pub fn start(cfg: &SandboxConfig, task_short: &str) -> Result<Sandbox> {
        let name = format!("{}-{}", cfg.name_prefix, task_short);
        // На случай остатка от прошлого запуска.
        let _ = run_cmd(
            &cfg.docker_bin,
            &["rm", "-f", &name],
            Duration::from_secs(30),
        );

        let args = Self::build_run_args(cfg, &name);
        let out = run_cmd_owned(&cfg.docker_bin, &args, Duration::from_secs(120))?;
        if !out.success() {
            return Err(WorkerError::Sandbox(format!(
                "docker run failed ({}): {}",
                out.exit_code,
                out.stderr.trim()
            )));
        }
        Ok(Sandbox {
            name,
            cfg: cfg.clone(),
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// Выполнить команду в sandbox. Таймаут и лимит вывода — ТЗ §5.
    ///
    /// `command` должен быть уже проверен allowlist'ом (см. `allowlist.rs`).
    pub fn exec(&self, command: &str) -> Result<ExecOutput> {
        let argv = parse_command_argv(command)?;
        let sec = self.cfg.cmd_timeout_sec.to_string();
        // Execute argv directly. The allowlist is policy, not a shell parser.
        let mut args = vec![
            "exec".to_string(),
            self.name.clone(),
            "timeout".to_string(),
            "-s".to_string(),
            "TERM".to_string(),
            sec.clone(),
        ];
        args.extend(argv);
        let started = Instant::now();
        // Внешний wall-clock: страховка на случай, если подвиснет сам docker.
        let outer = Duration::from_secs(self.cfg.cmd_timeout_sec + 15);
        let out = run_cmd_owned(&self.cfg.docker_bin, &args, outer)?;
        let duration_ms = started.elapsed().as_millis();

        if out.timed_out {
            return Err(WorkerError::CommandTimeout {
                command: command.to_string(),
                timeout_sec: self.cfg.cmd_timeout_sec,
            });
        }
        // 124 — таймаут внутри контейнера.
        if out.exit_code == 124 {
            return Err(WorkerError::CommandTimeout {
                command: command.to_string(),
                timeout_sec: self.cfg.cmd_timeout_sec,
            });
        }

        let (stdout, trunc_out) = truncate_output(&out.stdout, self.cfg.max_output_bytes);
        let (stderr, trunc_err) = truncate_output(&out.stderr, self.cfg.max_output_bytes);
        let output_bytes = out.stdout.len() + out.stderr.len();
        let output_sha256 = sha256_hex(format!("{}{}", out.stdout, out.stderr).as_bytes());

        Ok(ExecOutput {
            stdout,
            stderr,
            exit_code: out.exit_code,
            duration_ms,
            truncated: trunc_out || trunc_err,
            output_bytes,
            output_sha256,
        })
    }

    /// Записать файл в /workspace sandbox (ТЗ §1, инструмент `write_file`).
    ///
    /// `path` уже проверен allowlist'ом вызывающей стороной.
    pub fn write_file(&self, path: &str, content: &str) -> Result<ExecOutput> {
        let args = vec![
            "exec".to_string(),
            "-i".to_string(),
            self.name.clone(),
            "bash".to_string(),
            "-c".to_string(),
            "cat > \"$1\"".to_string(),
            "_".to_string(),
            path.to_string(),
        ];
        let started = Instant::now();
        let outer = Duration::from_secs(self.cfg.cmd_timeout_sec + 15);
        let out = run_cmd_stdin(&self.cfg.docker_bin, &args, outer, content)?;
        let duration_ms = started.elapsed().as_millis();
        if out.timed_out {
            return Err(WorkerError::CommandTimeout {
                command: format!("write_file {path}"),
                timeout_sec: self.cfg.cmd_timeout_sec,
            });
        }
        Ok(ExecOutput {
            stdout: out.stdout.clone(),
            stderr: out.stderr.clone(),
            exit_code: out.exit_code,
            duration_ms,
            truncated: false,
            output_bytes: content.len(),
            output_sha256: sha256_hex(content.as_bytes()),
        })
    }

    /// Прочитать файл отдельным argv без shell-интерпретации пути.
    pub fn read_file(&self, path: &str) -> Result<ExecOutput> {
        let args = vec![
            "exec".to_string(),
            self.name.clone(),
            "cat".to_string(),
            "--".to_string(),
            path.to_string(),
        ];
        let started = Instant::now();
        let outer = Duration::from_secs(self.cfg.cmd_timeout_sec + 15);
        let out = run_cmd_owned(&self.cfg.docker_bin, &args, outer)?;
        if out.timed_out {
            return Err(WorkerError::CommandTimeout {
                command: format!("read_file {path}"),
                timeout_sec: self.cfg.cmd_timeout_sec,
            });
        }
        let (stdout, trunc_out) = truncate_output(&out.stdout, self.cfg.max_output_bytes);
        let (stderr, trunc_err) = truncate_output(&out.stderr, self.cfg.max_output_bytes);
        Ok(ExecOutput {
            stdout,
            stderr,
            exit_code: out.exit_code,
            duration_ms: started.elapsed().as_millis(),
            truncated: trunc_out || trunc_err,
            output_bytes: out.stdout.len() + out.stderr.len(),
            output_sha256: sha256_hex(format!("{}{}", out.stdout, out.stderr).as_bytes()),
        })
    }

    /// Остановить и удалить контейнер.
    pub fn stop(&mut self) -> Result<()> {
        let out = run_cmd(
            &self.cfg.docker_bin,
            &["rm", "-f", &self.name],
            Duration::from_secs(30),
        )?;
        if !out.success() && !out.stderr.contains("No such container") {
            return Err(WorkerError::Sandbox(format!(
                "docker rm failed: {}",
                out.stderr.trim()
            )));
        }
        Ok(())
    }
}

/// Убрать брошенные sandbox-контейнеры (ТЗ §8.2 п.9).
///
/// `keep` — имена контейнеров активных задач; остальные с префиксом удаляются.
pub fn cleanup_stale(cfg: &SandboxConfig, keep: &[String]) -> Result<usize> {
    let out = run_cmd(
        &cfg.docker_bin,
        &[
            "ps",
            "-a",
            "--filter",
            &format!("name={}", cfg.name_prefix),
            "--format",
            "{{.Names}}",
        ],
        Duration::from_secs(30),
    )?;
    if !out.success() {
        return Err(WorkerError::Sandbox(format!(
            "docker ps failed: {}",
            out.stderr.trim()
        )));
    }
    let mut removed = 0usize;
    for name in out.stdout.lines().map(str::trim).filter(|l| !l.is_empty()) {
        if keep.iter().any(|k| k == name) {
            continue;
        }
        let rm = run_cmd(
            &cfg.docker_bin,
            &["rm", "-f", name],
            Duration::from_secs(30),
        )?;
        if rm.success() {
            removed += 1;
        }
    }
    Ok(removed)
}

/// Обрезать вывод до лимита. Возвращает (текст, был_ли_обрезан).
pub fn truncate_output(s: &str, limit: usize) -> (String, bool) {
    if s.len() <= limit {
        return (s.to_string(), false);
    }
    // Не режем посередине UTF-8.
    let mut end = limit;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    (format!("{}\n[truncated at {limit} bytes]", &s[..end]), true)
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    format!("sha256:{}", hex(&h.finalize()))
}

fn hex(b: &[u8]) -> String {
    let mut s = String::with_capacity(b.len() * 2);
    for x in b {
        s.push_str(&format!("{x:02x}"));
    }
    s
}

struct RawOut {
    stdout: String,
    stderr: String,
    exit_code: i32,
    timed_out: bool,
}

impl RawOut {
    fn success(&self) -> bool {
        self.exit_code == 0 && !self.timed_out
    }
}

/// Запустить процесс с wall-clock таймаутом (kill при превышении).
fn run_cmd(bin: &str, args: &[&str], timeout: Duration) -> Result<RawOut> {
    let owned: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    run_cmd_owned(bin, &owned, timeout)
}

fn run_cmd_owned(bin: &str, args: &[String], timeout: Duration) -> Result<RawOut> {
    run_cmd_owned_inner(bin, args, timeout, None)
}

/// Запустить процесс, записав `stdin_data` в stdin (для `write_file`).
fn run_cmd_stdin(
    bin: &str,
    args: &[String],
    timeout: Duration,
    stdin_data: &str,
) -> Result<RawOut> {
    run_cmd_owned_inner(bin, args, timeout, Some(stdin_data.to_string()))
}

fn run_cmd_owned_inner(
    bin: &str,
    args: &[String],
    timeout: Duration,
    stdin_data: Option<String>,
) -> Result<RawOut> {
    use std::io::{Read, Write};

    let mut child = Command::new(bin)
        .args(args)
        .stdin(if stdin_data.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| WorkerError::Sandbox(format!("spawn {bin}: {e}")))?;

    if let Some(data) = stdin_data {
        if let Some(mut si) = child.stdin.take() {
            // Do not synchronously fill a pipe: a child that stops reading
            // stdin must still be reachable by the watchdog below.
            std::thread::spawn(move || {
                let _ = si.write_all(data.as_bytes());
            });
        }
    }

    // Drain both pipes concurrently. A child that writes more than the OS
    // pipe buffer (~64 KiB) blocks on write; polling `try_wait` without
    // reading would then let the wall-clock watchdog kill a healthy command
    // and lose its output. Independent reader threads keep the pipes moving
    // while the loop below only tracks liveness and the deadline.
    let stdout_reader = child.stdout.take().map(|mut p| {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = p.read_to_end(&mut buf);
            buf
        })
    });
    let stderr_reader = child.stderr.take().map(|mut p| {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = p.read_to_end(&mut buf);
            buf
        })
    });

    let deadline = Instant::now() + timeout;
    let mut timed_out = false;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    timed_out = true;
                    break;
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(e) => return Err(WorkerError::Sandbox(format!("wait {bin}: {e}"))),
        }
    }

    let status = child
        .wait()
        .map_err(|e| WorkerError::Sandbox(format!("output {bin}: {e}")))?;
    let stdout = stdout_reader
        .map(|h| h.join().unwrap_or_default())
        .unwrap_or_default();
    let stderr = stderr_reader
        .map(|h| h.join().unwrap_or_default())
        .unwrap_or_default();
    Ok(RawOut {
        stdout: String::from_utf8_lossy(&stdout).into_owned(),
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
        exit_code: status.code().unwrap_or(-1),
        timed_out,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // M3/M4/M5 (ТЗ §9): флаги изоляции присутствуют в argv.
    #[test]
    fn run_args_have_security_flags() {
        let cfg = SandboxConfig::default();
        let args = Sandbox::build_run_args(&cfg, "swarm-sandbox-test");
        let joined = args.join(" ");
        assert!(
            joined.contains("--cap-drop ALL"),
            "нет --cap-drop ALL: {joined}"
        );
        assert!(joined.contains("--read-only"), "нет --read-only");
        assert!(joined.contains("--network none"), "нет --network none");
        assert!(
            joined.contains("no-new-privileges:true"),
            "нет no-new-privileges"
        );
        assert!(joined.contains("--memory 2g"), "нет лимита памяти");
        assert!(joined.contains("--cpus 2"), "нет лимита cpu");
        assert!(
            joined.contains("-u 1001:1001"),
            "нет непривилегированного uid"
        );
        assert!(
            joined.contains("--tmpfs /workspace"),
            "нет tmpfs /workspace"
        );
        assert!(
            joined.contains("mode=0700"),
            "tmpfs /workspace без uid/mode: {joined}"
        );
    }

    #[test]
    fn run_args_use_configured_image_and_name() {
        let mut cfg = SandboxConfig::default();
        cfg.image = "custom:1".into();
        let args = Sandbox::build_run_args(&cfg, "my-name");
        assert!(args.contains(&"custom:1".to_string()));
        assert!(args.contains(&"my-name".to_string()));
        assert_eq!(args.last().unwrap(), "infinity");
    }

    // §8.1.6: лимит вывода обрезает.
    #[test]
    fn output_limit_truncates() {
        let big = "x".repeat(20 * 1024 * 1024);
        let (s, tr) = truncate_output(&big, 1024);
        assert!(tr, "должно быть помечено truncated");
        assert!(s.len() < 2000);
        assert!(s.contains("truncated at 1024 bytes"));

        let small = "hello";
        let (s2, tr2) = truncate_output(small, 1024);
        assert!(!tr2);
        assert_eq!(s2, "hello");
    }

    #[test]
    fn truncate_respects_utf8_boundary() {
        // «Ж» = 2 байта; лимит 5 попадёт в середину символа.
        let s = "ЖЖЖ";
        let (out, tr) = truncate_output(s, 5);
        assert!(tr);
        // не должно быть паники и невалидного UTF-8
        assert!(out.starts_with('Ж'));
    }

    #[test]
    fn sha256_is_prefixed() {
        assert!(sha256_hex(b"abc").starts_with("sha256:"));
        assert_eq!(
            sha256_hex(b"abc"),
            "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn command_argv_does_not_interpret_shell() {
        assert!(parse_command_argv("echo ok; touch /tmp/pwned").is_err());
        assert_eq!(
            parse_command_argv("echo 'hello world'").unwrap(),
            ["echo", "hello world"]
        );
    }

    #[test]
    fn command_argv_rejects_expansion_and_unclosed_quotes() {
        assert!(parse_command_argv("echo $(id)").is_err());
        assert!(parse_command_argv("echo 'unterminated").is_err());
    }

    // Wave 2: output larger than the OS pipe buffer must not deadlock.
    // The child blocks on write once a pipe fills (~64 KiB); polling
    // `try_wait` without draining the pipes would let the wall-clock watchdog
    // kill a perfectly healthy command and lose its output.
    #[test]
    fn captures_output_larger_than_pipe_buffer() {
        // 20000 lines * 17 bytes = ~332 KiB on stdout, far above 64 KiB.
        let out = run_cmd(
            "bash",
            &["-c", "yes 0123456789abcdef | head -n 20000"],
            Duration::from_secs(15),
        )
        .expect("run_cmd");
        assert!(
            !out.timed_out,
            "large output must not be killed as a timeout (deadlock)"
        );
        assert_eq!(out.exit_code, 0);
        assert_eq!(
            out.stdout.len(),
            17 * 20_000,
            "full stdout must be captured, not truncated by a stall"
        );
    }
}
