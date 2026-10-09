use crate::errors::DispatcherError;
use crate::types::{Lease, TaskContract, Verdict};
use postgres::{Client, NoTls};
use uuid::Uuid;

pub struct SwarmDispatcher {
    client: Client,
}

impl SwarmDispatcher {
    pub fn connect(dsn: &str) -> Result<Self, DispatcherError> {
        let client = Client::connect(dsn, NoTls).map_err(DispatcherError::from_pg)?;
        Ok(Self { client })
    }

    pub fn add_task(&mut self, task_id: Uuid, contract: &TaskContract, root_id: Option<Uuid>,
                    budget_cap: i64, priority: i32) -> Result<(), DispatcherError> {
        contract.validate()?;
        let body = serde_json::to_value(contract)?;
        self.client
            .execute("SELECT add_task($1, $2, $3::jsonb, $4, $5)",
                     &[&task_id, &root_id, &body, &budget_cap, &priority])
            .map_err(DispatcherError::from_pg)?;
        Ok(())
    }

    pub fn add_dep(&mut self, task_id: Uuid, dep_id: Uuid) -> Result<(), DispatcherError> {
        self.client.execute("SELECT add_dep($1, $2)", &[&task_id, &dep_id])
            .map_err(DispatcherError::from_pg)?;
        Ok(())
    }

    pub fn promote_ready(&mut self) -> Result<i32, DispatcherError> {
        let row = self.client.query_one("SELECT promote_ready()", &[]).map_err(DispatcherError::from_pg)?;
        Ok(row.get(0))
    }

    pub fn register_worker(&mut self, worker_id: &str, capabilities: &serde_json::Value,
                           tags: &[String]) -> Result<(), DispatcherError> {
        self.client
            .execute("SELECT register_worker($1, $2::jsonb, $3)", &[&worker_id, capabilities, &tags])
            .map_err(DispatcherError::from_pg)?;
        Ok(())
    }

    pub fn claim(&mut self, worker_id: &str, lease_sec: i32) -> Result<Option<Lease>, DispatcherError> {
        let rows = self.client
            .query("SELECT out_task_id, out_contract, out_epoch, out_budget, out_spent FROM claim_task($1, $2)",
                   &[&worker_id, &lease_sec])
            .map_err(DispatcherError::from_pg)?;
        if rows.is_empty() { return Ok(None); }
        let r = &rows[0];
        let contract: serde_json::Value = r.get(1);
        Ok(Some(Lease { task_id: r.get(0), contract, epoch: r.get(2), budget_cap: r.get(3),
                        spent_tokens: r.get(4), worker_id: worker_id.to_owned() }))
    }

    pub fn heartbeat(&mut self, lease: &Lease, extend_sec: i32) -> Result<bool, DispatcherError> {
        let row = self.client
            .query_one("SELECT heartbeat($1, $2, $3, $4)",
                       &[&lease.task_id, &lease.worker_id, &lease.epoch, &extend_sec])
            .map_err(DispatcherError::from_pg)?;
        Ok(row.get(0))
    }

    pub fn save_artifact(&mut self, lease: &Lease, artifact_ref: &str, storage_path: &str,
                         content_type: Option<&str>, size_bytes: Option<i64>) -> Result<(), DispatcherError> {
        self.client
            .execute("SELECT save_artifact($1, $2, $3, $4, $5, $6, $7)",
                     &[&lease.task_id, &lease.worker_id, &lease.epoch, &artifact_ref, &storage_path,
                       &content_type, &size_bytes])
            .map_err(DispatcherError::from_pg)?;
        Ok(())
    }

    pub fn complete(&mut self, lease: &Lease, result_ref: &str, tokens: i64) -> Result<String, DispatcherError> {
        let row = self.client
            .query_one("SELECT complete_task($1, $2, $3, $4, $5)",
                       &[&lease.task_id, &lease.worker_id, &lease.epoch, &result_ref, &tokens])
            .map_err(DispatcherError::from_pg)?;
        Ok(row.get(0))
    }

    pub fn submit_review(&mut self, task_id: Uuid, artifact_ref: &str, reviewer: &str, verdict: Verdict,
                         checks: &serde_json::Value) -> Result<String, DispatcherError> {
        let row = self.client
            .query_one("SELECT submit_review($1, $2, $3, $4, $5::jsonb)",
                       &[&task_id, &artifact_ref, &reviewer, &verdict.as_str(), checks])
            .map_err(DispatcherError::from_pg)?;
        Ok(row.get(0))
    }

    pub fn reclaim_expired(&mut self) -> Result<i32, DispatcherError> {
        let row = self.client.query_one("SELECT reclaim_expired()", &[]).map_err(DispatcherError::from_pg)?;
        Ok(row.get(0))
    }

    pub fn reopen(&mut self, task_id: Uuid, actor: &str) -> Result<i32, DispatcherError> {
        let row = self.client.query_one("SELECT reopen_task($1, $2)", &[&task_id, &actor])
            .map_err(DispatcherError::from_pg)?;
        Ok(row.get(0))
    }

    pub fn unblock(&mut self, task_id: Uuid, actor: &str) -> Result<String, DispatcherError> {
        let row = self.client.query_one("SELECT unblock_task($1, $2)", &[&task_id, &actor])
            .map_err(DispatcherError::from_pg)?;
        Ok(row.get(0))
    }

    pub fn verify_chain(&mut self, task_id: Uuid) -> Result<bool, DispatcherError> {
        let row = self.client.query_one("SELECT verify_chain($1)", &[&task_id]).map_err(DispatcherError::from_pg)?;
        Ok(row.get(0))
    }
}

// ---------- транзакции ----------
// Два лайфтайма обязательны: `&'a mut Transaction<'a>` делает заём бесконечным
// (инвариантность), и `tx.commit()` после замыкания не компилируется (E0597/E0505).
pub struct DispatcherTx<'a, 'b> {
    tx: &'a mut postgres::Transaction<'b>,
}

impl SwarmDispatcher {
    /// Всё или ничего: Err из замыкания (или паника) = rollback, Ok = commit.
    pub fn with_transaction<F, T>(&mut self, f: F) -> Result<T, DispatcherError>
    where
        F: FnOnce(&mut DispatcherTx<'_, '_>) -> Result<T, DispatcherError>,
    {
        let mut tx = self.client.transaction().map_err(DispatcherError::from_pg)?;
        let out = {
            let mut w = DispatcherTx { tx: &mut tx };
            f(&mut w)?           // при Err tx дропается без commit -> rollback
        };
        tx.commit().map_err(DispatcherError::from_pg)?;
        Ok(out)
    }
}

impl DispatcherTx<'_, '_> {
    pub fn add_task(&mut self, task_id: Uuid, contract: &TaskContract, root_id: Option<Uuid>,
                    budget_cap: i64, priority: i32) -> Result<(), DispatcherError> {
        contract.validate()?;
        let body = serde_json::to_value(contract)?;
        self.tx
            .execute("SELECT add_task($1, $2, $3::jsonb, $4, $5)",
                     &[&task_id, &root_id, &body, &budget_cap, &priority])
            .map_err(DispatcherError::from_pg)?;
        Ok(())
    }

    pub fn add_dep(&mut self, task_id: Uuid, dep_id: Uuid) -> Result<(), DispatcherError> {
        self.tx.execute("SELECT add_dep($1, $2)", &[&task_id, &dep_id])
            .map_err(DispatcherError::from_pg)?;
        Ok(())
    }

    pub fn promote_ready(&mut self) -> Result<i32, DispatcherError> {
        let row = self.tx.query_one("SELECT promote_ready()", &[]).map_err(DispatcherError::from_pg)?;
        Ok(row.get(0))
    }
}
