//! CLI: seed / status / review / reopen / unblock.

use std::process::ExitCode;

use clap::{Parser, Subcommand};
use swarm_core::{Budget, Needs, SwarmDispatcher, TaskContract, Verdict};
use uuid::Uuid;

#[derive(Parser)]
#[command(name = "swarm-cli", version, about = "Swarm Dispatcher CLI")]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Создать демо-граф: корень + 2 зависимые задачи.
    Seed,
    /// Показать все задачи.
    Status,
    /// Оставить ревью.
    Review {
        task_id: Uuid,
        artifact_ref: String,
        #[arg(value_parser = ["approve", "changes", "reject"])]
        verdict: String,
        /// JSON-массив реально наблюдённых проверок.
        ///
        /// Обязателен для `approve`: CLI не выдумывает проходящий набор
        /// checks (иначе строгий review-gate в `submit_review` обходится
        /// фиктивным успехом). Для `changes`/`reject` не нужен.
        #[arg(long)]
        checks: Option<String>,
    },
    /// Переоткрыть завершённую задачу (каскадный откат).
    Reopen { task_id: Uuid, actor: String },
    /// Разблокировать задачу.
    Unblock { task_id: Uuid, actor: String },
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    let dsn = match std::env::var("DATABASE_URL") {
        Ok(v) => v,
        Err(_) => {
            eprintln!("error: DATABASE_URL required");
            return ExitCode::FAILURE;
        }
    };
    let mut d = match SwarmDispatcher::connect(&dsn) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("error: connect to Postgres: {e}");
            return ExitCode::FAILURE;
        }
    };

    // Ненулевой код возврата на любой ошибке: вызывающие скрипты не должны
    // принимать неуспех за успех (аудит: exit-code семантика CLI).
    let outcome: Result<(), String> = match cli.command {
        Cmd::Seed => seed(&mut d),
        Cmd::Status => status(&mut d),
        Cmd::Review {
            task_id,
            artifact_ref,
            verdict,
            checks,
        } => review(&mut d, task_id, &artifact_ref, &verdict, checks.as_deref()),
        Cmd::Reopen { task_id, actor } => match d.reopen(task_id, &actor) {
            Ok(n) => {
                println!("blocked {n} descendants");
                Ok(())
            }
            Err(e) => Err(e.to_string()),
        },
        Cmd::Unblock { task_id, actor } => match d.unblock(task_id, &actor) {
            Ok(state) => {
                println!("new state: {state}");
                Ok(())
            }
            Err(e) => Err(e.to_string()),
        },
    };

    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Оставить ревью.
///
/// Набор проверок (`--checks`) приходит от оператора и не выдумывается: для
/// `approve` он обязателен, иначе CLI отказывает (fail-closed). Это закрывает
/// обход строгого review-gate в `submit_review`, который раньше «проходил» на
/// жёстко зашитом `[{"type":"cmd","status":"pass"}]` независимо от контракта.
fn review(
    d: &mut SwarmDispatcher,
    task_id: Uuid,
    artifact_ref: &str,
    verdict: &str,
    checks_json: Option<&str>,
) -> Result<(), String> {
    let v = match verdict {
        "approve" => Verdict::Approved,
        "changes" => Verdict::ChangesRequested,
        _ => Verdict::Rejected,
    };

    let checks = match checks_json {
        Some(raw) => {
            let parsed: serde_json::Value = serde_json::from_str(raw)
                .map_err(|e| format!("--checks is not valid JSON: {e}"))?;
            if !parsed.is_array() {
                return Err("--checks must be a JSON array".to_string());
            }
            parsed
        }
        None if verdict == "approve" => {
            return Err(
                "approve requires --checks <json>: the CLI will not fabricate a \
                 passing check set; supply the checks actually observed"
                    .to_string(),
            );
        }
        None => serde_json::json!([]),
    };

    match d.submit_review(task_id, artifact_ref, "cli-reviewer", v, &checks) {
        Ok(state) => {
            println!("new state: {state}");
            Ok(())
        }
        Err(e) => Err(e.to_string()),
    }
}

fn contract(kind: &str) -> TaskContract {
    TaskContract {
        kind: kind.to_string(),
        description: None,
        checks: vec![serde_json::json!({"type": "cmd", "run": "true"})],
        needs: Needs::default(),
        budget: Budget::default(),
    }
}

fn seed(d: &mut SwarmDispatcher) -> Result<(), String> {
    let root = Uuid::new_v4();
    d.add_task(root, &contract("root"), None, 10_000, 10)
        .map_err(|e| format!("add root: {e}"))?;

    for i in 1..=2 {
        let child = Uuid::new_v4();
        d.add_task(
            child,
            &contract(&format!("child-{i}")),
            Some(root),
            10_000,
            5,
        )
        .map_err(|e| format!("add child: {e}"))?;
        d.add_dep(child, root)
            .map_err(|e| format!("add dep: {e}"))?;
        println!("created child {} depends on root {}", child, root);
    }
    println!("seed done, root = {root}");
    Ok(())
}

fn status(d: &mut SwarmDispatcher) -> Result<(), String> {
    // Чтение через task_status(): прямой SELECT к tasks сервисным ролям закрыт
    // после roles.sql.
    let tasks = d.list_task_status(200).map_err(|e| e.to_string())?;

    if tasks.is_empty() {
        println!("(no tasks)");
        return Ok(());
    }

    println!(
        "{:<38} {:<10} {:<8} {:<10} {:>4} {:>4}",
        "id", "state", "epoch", "owner", "att", "rw"
    );
    for t in tasks {
        println!(
            "{:<38} {:<10} {:<8} {:<10} {:>4} {:>4}",
            t.id,
            t.state,
            t.lease_epoch,
            t.owner.as_deref().unwrap_or("-"),
            t.attempts,
            t.rework,
        );
        if let Some(reason) = t.block_reason {
            println!("    blocked: {reason}");
        }
    }
    Ok(())
}
