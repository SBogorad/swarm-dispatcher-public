//! CLI: `swarm-planner "задача"` (или из stdin).

use clap::Parser;
use swarm_core::SwarmDispatcher;
use swarm_planner::{LlmClient, LlmConfig, Planner, PlannerConfig};

#[derive(Parser)]
#[command(name = "swarm-planner", version, about = "LLM-планировщик графа задач")]
struct Cli {
    /// Текст задачи. Если не задан — читается из stdin.
    task: Option<String>,
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();
    let task_text = match cli.task {
        Some(t) => t,
        None => {
            use std::io::Read;
            let mut s = String::new();
            std::io::stdin()
                .read_to_string(&mut s)
                .expect("read stdin");
            s.trim().to_string()
        }
    };
    if task_text.is_empty() {
        eprintln!("error: пустой текст задачи");
        std::process::exit(2);
    }

    let dsn = std::env::var("DATABASE_URL").expect("DATABASE_URL required");
    let cfg = PlannerConfig::from_env();
    let llm_cfg = match LlmConfig::from_env() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(2);
        }
    };
    let llm = LlmClient::new(llm_cfg).expect("LLM client");

    let context = std::env::var("PLANNER_CONTEXT").ok();
    let expected = std::env::var("PLANNER_EXPECTED").ok();

    let planner = Planner::new(cfg, llm).with_context(context, expected);

    let mut dispatcher = match SwarmDispatcher::connect(&dsn) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("error: connect: {e}");
            std::process::exit(1);
        }
    };

    match planner.plan(&mut dispatcher, &task_text) {
        Ok(out) if out.already_existed => {
            println!(
                "graph already exists (root_id={}, tasks={})",
                out.root_id, out.existing_tasks
            );
        }
        Ok(out) => {
            println!("root_id: {}", out.root_id);
            println!("tasks created: {}", out.tasks_created);
            println!("ready to execute: {}", out.ready_to_execute);
        }
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
}
