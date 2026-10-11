//! Аудит tool calls в журнал событий (ТЗ v0.4 §7).
//!
//! Пишем **только hash и размер** вывода, не полный текст.
//!
//! Важно: прямой `append_event` воркеру недоступен by design (см.
//! `step2/roles.sql` §6 — «журнал нельзя подделать снаружи»). Поэтому sink
//! вызывает SECURITY DEFINER-функцию `record_tool_call`, которая проверяет
//! живую аренду и пишет событие от имени `swarm_owner`
//! (`step2/roles_patch_tool_call_audit.sql`).

use crate::agent_loop::{ToolAudit, ToolAuditSink};
use crate::error::{Result, WorkerError};
use postgres::{Client, NoTls};
use uuid::Uuid;

/// Пишет событие `tool_call` в `events` через `record_tool_call`.
pub struct PgAuditSink {
    client: Client,
    task_id: Uuid,
    worker_id: String,
    epoch: i64,
    seq: u64,
}

impl PgAuditSink {
    /// Отдельное соединение к той же БД, что и воркер.
    pub fn connect(dsn: &str, task_id: Uuid, worker_id: &str, epoch: i64) -> Result<Self> {
        let client = Client::connect(dsn, NoTls)
            .map_err(|e| WorkerError::Database(swarm_core::DispatcherError::from_pg(e)))?;
        Ok(Self {
            client,
            task_id,
            worker_id: worker_id.to_string(),
            epoch,
            seq: 0,
        })
    }
}

impl ToolAuditSink for PgAuditSink {
    fn record(&mut self, a: &ToolAudit) -> Result<()> {
        self.seq += 1;
        let event_id = format!("tool_call_{}", self.seq);
        // §7: команда + exit_code + длительность + hash вывода + размер.
        let payload = serde_json::json!({
            "tool": a.tool,
            "command": a.command,
            "exit_code": a.exit_code,
            "duration_ms": a.duration_ms as i64,
            "output_hash": a.output_sha256,
            "output_bytes": a.output_bytes,
            "truncated": a.truncated,
            "rejected": a.rejected,
            "reason": a.reason,
        });
        self.client
            .execute(
                "select record_tool_call($1, $2, $3, $4, $5)",
                &[
                    &self.task_id,
                    &self.worker_id,
                    &self.epoch,
                    &event_id,
                    &payload,
                ],
            )
            .map_err(|e| WorkerError::Database(swarm_core::DispatcherError::from_pg(e)))?;
        Ok(())
    }
}

/// Посчитать события `tool_call` по задаче (для тестов/диагностики).
pub fn count_tool_calls(dsn: &str, task_id: Uuid) -> Result<i64> {
    let mut c = Client::connect(dsn, NoTls)
        .map_err(|e| WorkerError::Database(swarm_core::DispatcherError::from_pg(e)))?;
    let row = c
        .query_one(
            "select count(*) from events where task_id = $1 and kind = 'tool_call'",
            &[&task_id],
        )
        .map_err(|e| WorkerError::Database(swarm_core::DispatcherError::from_pg(e)))?;
    Ok(row.get(0))
}
