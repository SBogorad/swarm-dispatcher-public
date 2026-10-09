//! CLI: seed / status / review / reopen / unblock.

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
    },
    /// Переоткрыть завершённую задачу (каскадный откат).
    Reopen { task_id: Uuid, actor: String },
    /// Разблокировать задачу.
    Unblock { task_id: Uuid, actor: String },
}

fn main() {
    let dsn = std::env::var("DATABASE_URL").expect("DATABASE_URL required");
    let cli = Cli::parse();
    let mut d = SwarmDispatcher::connect(&dsn).expect("connect to Postgres");

    match cli.command {
        Cmd::Seed => seed(&mut d),
        Cmd::Status => status(&mut d),
        Cmd::Review { task_id, artifact_ref, verdict } => {
            let v = match verdict.as_str() {
                "approve" => Verdict::Approved,
                "changes" => Verdict::ChangesRequested,
                _ => Verdict::Rejected,
            };
            let checks = serde_json::json!([
                {"type": "cmd", "status": "pass", "exit_code": 0}
            ]);
            match d.submit_review(task_id, &artifact_ref, "cli-reviewer", v, &checks) {
                Ok(state) => println!("new state: {state}"),
                Err(e) => eprintln!("error: {e}"),
            }
        }
        Cmd::Reopen { task_id, actor } => {
            match d.reopen(task_id, &actor) {
                Ok(n) => println!("blocked {n} descendants"),
                Err(e) => eprintln!("error: {e}"),
            }
        }
        Cmd::Unblock { task_id, actor } => {
            match d.unblock(task_id, &actor) {
                Ok(state) => println!("new state: {state}"),
                Err(e) => eprintln!("error: {e}"),
            }
        }
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

fn seed(d: &mut SwarmDispatcher) {
    let root = Uuid::new_v4();
    d.add_task(root, &contract("root"), None, 10_000, 10)
        .expect("add root");

    for i in 1..=2 {
        let child = Uuid::new_v4();
        d.add_task(child, &contract(&format!("child-{i}")), Some(root), 10_000, 5)
            .expect("add child");
        d.add_dep(child, root).expect("add dep");
        println!("created child {} depends on root {}", child, root);
    }
    println!("seed done, root = {root}");
}

fn status(d: &mut SwarmDispatcher) {
    // Чтение через task_status(): прямой SELECT к tasks сервисным ролям закрыт
    // после roles.sql.
    let tasks = match d.list_task_status(200) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("error: {e}");
            return;
        }
    };

    if tasks.is_empty() {
        println!("(no tasks)");
        return;
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
}

