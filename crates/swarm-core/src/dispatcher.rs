use crate::errors::DispatcherError;
use crate::types::{Lease, ParentArtifactRef, TaskContract, TaskRow, TaskStatusRow, Verdict};
use postgres::{Client, NoTls};
use uuid::Uuid;

pub struct SwarmDispatcher {
    client: Client,
}

impl SwarmDispatcher {
    /// Единственный `?`-совместимый путь к `postgres::Error` — явный `map_err`.
    /// Никогда не полагаться на `From<postgres::Error>` (см. комментарий в `errors.rs`).
    pub fn connect(dsn: &str) -> Result<Self, DispatcherError> {
        let client = Client::connect(dsn, NoTls).map_err(DispatcherError::from_pg)?;
        Ok(Self { client })
    }

    // ---------- чтение ----------

    /// Список задач **напрямую из таблицы** — доступно только суперпользователю
    /// (тесты). Сервисные роли читают через [`SwarmDispatcher::list_task_status`].
    pub fn list_tasks(
        &mut self,
        filter_state: Option<&str>,
        limit: i64,
    ) -> Result<Vec<TaskRow>, DispatcherError> {
        let rows = match filter_state {
            Some(state) => self
                .client
                .query(
                    "SELECT id, root_id, state, lease_owner, lease_epoch, \
                            attempts, rework, priority, block_reason \
                     FROM tasks WHERE state = $1 \
                     ORDER BY priority DESC, created_at ASC LIMIT $2",
                    &[&state, &limit],
                )
                .map_err(DispatcherError::from_pg)?,
            None => self
                .client
                .query(
                    "SELECT id, root_id, state, lease_owner, lease_epoch, \
                            attempts, rework, priority, block_reason \
                     FROM tasks \
                     ORDER BY priority DESC, created_at ASC LIMIT $1",
                    &[&limit],
                )
                .map_err(DispatcherError::from_pg)?,
        };

        Ok(rows
            .iter()
            .map(|r| TaskRow {
                id: r.get(0),
                root_id: r.get(1),
                state: r.get(2),
                owner: r.get(3),
                lease_epoch: r.get(4),
                attempts: r.get(5),
                rework: r.get(6),
                priority: r.get(7),
                block_reason: r.get(8),
            })
            .collect())
    }

    /// Список задач через `task_status(n)` — путь чтения для сервисных ролей.
    /// Прямой SELECT к `tasks` после `roles.sql` им закрыт.
    pub fn list_task_status(&mut self, limit: i32) -> Result<Vec<TaskStatusRow>, DispatcherError> {
        let rows = self
            .client
            .query(
                "SELECT id, root_id, state, lease_epoch, lease_owner, \
                        attempts, rework, block_reason \
                 FROM task_status($1)",
                &[&limit],
            )
            .map_err(DispatcherError::from_pg)?;

        Ok(rows
            .iter()
            .map(|r| TaskStatusRow {
                id: r.get(0),
                root_id: r.get(1),
                state: r.get(2),
                lease_epoch: r.get(3),
                owner: r.get(4),
                attempts: r.get(5),
                rework: r.get(6),
                block_reason: r.get(7),
            })
            .collect())
    }

    // ---------- граф ----------

    pub fn add_task(
        &mut self,
        task_id: Uuid,
        contract: &TaskContract,
        root_id: Option<Uuid>,
        budget_cap: i64,
        priority: i32,
    ) -> Result<(), DispatcherError> {
        contract.validate()?;
        let body = serde_json::to_value(contract)?;
        self.client
            .execute(
                "SELECT add_task($1, $2, $3::jsonb, $4, $5)",
                &[&task_id, &root_id, &body, &budget_cap, &priority],
            )
            .map_err(DispatcherError::from_pg)?;
        Ok(())
    }

    pub fn add_dep(&mut self, task_id: Uuid, dep_id: Uuid) -> Result<(), DispatcherError> {
        self.client
            .execute("SELECT add_dep($1, $2)", &[&task_id, &dep_id])
            .map_err(DispatcherError::from_pg)?;
        Ok(())
    }

    pub fn promote_ready(&mut self) -> Result<i32, DispatcherError> {
        let row = self
            .client
            .query_one("SELECT promote_ready()", &[])
            .map_err(DispatcherError::from_pg)?;
        Ok(row.get(0))
    }

    // ---------- воркеры ----------

    pub fn register_worker(
        &mut self,
        worker_id: &str,
        capabilities: &serde_json::Value,
        tags: &[String],
    ) -> Result<(), DispatcherError> {
        self.client
            .execute(
                "SELECT register_worker($1, $2::jsonb, $3)",
                &[&worker_id, capabilities, &tags],
            )
            .map_err(DispatcherError::from_pg)?;
        Ok(())
    }

    /// Выдаёт аренду. Контракт возвращается **сырым JSON** — разбор только через
    /// [`Lease::typed_contract`]. См. комментарий к `Lease` в `types.rs`.
    pub fn claim(
        &mut self,
        worker_id: &str,
        lease_sec: i32,
    ) -> Result<Option<Lease>, DispatcherError> {
        let rows = self
            .client
            .query(
                "SELECT out_task_id, out_contract, out_epoch, out_budget, out_spent \
                 FROM claim_task($1, $2)",
                &[&worker_id, &lease_sec],
            )
            .map_err(DispatcherError::from_pg)?;
        if rows.is_empty() {
            return Ok(None);
        }
        let r = &rows[0];
        let task_id: Uuid = r.get(0);
        let epoch: i64 = r.get(2);
        // FIND-CODE-02: контур обратной связи. Отдельный RPC, а не лишние
        // колонки `claim_task`: смена возвращаемого типа ломает применение
        // миграций (`create or replace` не меняет row type) и требует DROP +
        // перевыдачи гранта. Здесь же — обычный вызов привилегированной
        // функции, доступный только держателю живой аренды.
        //
        // Best-effort (аудит Gemini Argon, вопрос 13): `claim_task` уже
        // перевёл задачу в `leased` и вернул аренду. Если вспомогательный RPC
        // диагностики недоступен (например, на старом томе нет миграции 012),
        // ошибка здесь роняла бы `claim()` — аренда терялась бы, а задача
        // висела в `leased` до истечения `lease_sec`. Диагностика — не повод
        // срывать уже выданную работу, поэтому деградируем в `(0, None)`.
        let (rework, last_review) = match self.rework_feedback(task_id, worker_id, epoch) {
            Ok(v) => v,
            Err(e) => {
                eprintln!(
                    "warn: rework_feedback unavailable for task {task_id}: {e}; \
                         continuing without review diagnosis"
                );
                (0, None)
            }
        };
        Ok(Some(Lease {
            task_id,
            contract: r.get(1), // сырой JSON: никакого from_value здесь
            epoch,
            budget_cap: r.get(3),
            spent_tokens: r.get(4),
            worker_id: worker_id.to_owned(),
            rework,
            last_review,
        }))
    }

    /// Итерация 3 (DAG handoff): артефакты родительских задач.
    ///
    /// Дочерняя задача в графе видит выход своих зависимостей, не имея
    /// прямого `SELECT` к `tasks`/`artifacts`: RPC `task_parent_artifacts`
    /// — SECURITY DEFINER и проверяет identity воркера и ЖИВУЮ аренду
    /// именно этой задачи.
    pub fn task_parent_artifacts(
        &mut self,
        task_id: Uuid,
        worker_id: &str,
        epoch: i64,
    ) -> Result<Vec<ParentArtifactRef>, DispatcherError> {
        let rows = self
            .client
            .query(
                "SELECT parent_task_id, parent_kind, artifact_ref, artifact_sha256 \
                 FROM task_parent_artifacts($1, $2, $3)",
                &[&task_id, &worker_id, &epoch],
            )
            .map_err(DispatcherError::from_pg)?;
        Ok(rows
            .iter()
            .map(|r| ParentArtifactRef {
                parent_task_id: r.get(0),
                parent_kind: r.get(1),
                artifact_ref: r.get(2),
                artifact_sha256: r.get(3),
            })
            .collect())
    }

    /// FIND-CODE-02: диагностика последнего ревью для возвращённой задачи.
    /// Возвращает `(rework, last_review)`; при отсутствии ревью — `(n, None)`.
    /// `task_rework_feedback` — SECURITY DEFINER с проверкой аренды и identity,
    /// поэтому чужой воркер не прочтёт диагностику чужой задачи.
    fn rework_feedback(
        &mut self,
        task_id: Uuid,
        worker_id: &str,
        epoch: i64,
    ) -> Result<(i32, Option<serde_json::Value>), DispatcherError> {
        let rows = self
            .client
            .query(
                "SELECT out_rework, out_last_review \
                 FROM task_rework_feedback($1, $2, $3)",
                &[&task_id, &worker_id, &epoch],
            )
            .map_err(DispatcherError::from_pg)?;
        if rows.is_empty() {
            return Ok((0, None));
        }
        let r = &rows[0];
        Ok((r.get(0), r.get(1)))
    }

    /// F8: свежее состояние бюджета задачи (локальный + по дереву).
    ///
    /// `Lease.spent_tokens` — снимок на момент claim; перед вызовом LLM воркер
    /// обязан видеть расход, сделанный после выдачи (в т.ч. по тому же дереву).
    /// Возвращает `(spent, budget_cap, root_spent, root_budget_cap)`;
    /// `root_budget_cap` = `None`, если лимита по дереву нет.
    pub fn task_budget_state(
        &mut self,
        task_id: Uuid,
    ) -> Result<(i64, i64, i64, Option<i64>), DispatcherError> {
        let row = self
            .client
            .query_one(
                "SELECT out_spent, out_budget, out_root_spent, out_root_budget \
                 FROM task_budget_state($1)",
                &[&task_id],
            )
            .map_err(DispatcherError::from_pg)?;
        Ok((row.get(0), row.get(1), row.get(2), row.get(3)))
    }

    /// Не бросает исключение при потере аренды — возвращает `Ok(false)`.
    /// Так воркер может реагировать на потерю без `catch`.
    pub fn heartbeat(&mut self, lease: &Lease, extend_sec: i32) -> Result<bool, DispatcherError> {
        let row = self
            .client
            .query_one(
                "SELECT heartbeat($1, $2, $3, $4)",
                &[&lease.task_id, &lease.worker_id, &lease.epoch, &extend_sec],
            )
            .map_err(DispatcherError::from_pg)?;
        Ok(row.get(0))
    }

    pub fn save_artifact(
        &mut self,
        lease: &Lease,
        artifact_ref: &str,
        storage_path: &str,
        content_type: Option<&str>,
        size_bytes: Option<i64>,
    ) -> Result<(), DispatcherError> {
        self.client
            .execute(
                "SELECT save_artifact($1, $2, $3, $4, $5, $6, $7)",
                &[
                    &lease.task_id,
                    &lease.worker_id,
                    &lease.epoch,
                    &artifact_ref,
                    &storage_path,
                    &content_type,
                    &size_bytes,
                ],
            )
            .map_err(DispatcherError::from_pg)?;
        Ok(())
    }

    /// Возвращает новое состояние задачи: `"review"` или `"blocked"`.
    pub fn complete(
        &mut self,
        lease: &Lease,
        result_ref: &str,
        tokens: i64,
    ) -> Result<String, DispatcherError> {
        let row = self
            .client
            .query_one(
                "SELECT complete_task($1, $2, $3, $4, $5)",
                &[
                    &lease.task_id,
                    &lease.worker_id,
                    &lease.epoch,
                    &result_ref,
                    &tokens,
                ],
            )
            .map_err(DispatcherError::from_pg)?;
        Ok(row.get(0))
    }

    /// F4: честный отказ воркера. Переводит задачу в `blocked` с `reason`,
    /// без подделки `spent_tokens` (раньше это делалось через `complete`
    /// с overspend). Возвращает `"blocked"`.
    pub fn fail(
        &mut self,
        lease: &Lease,
        result_ref: Option<&str>,
        reason: &str,
    ) -> Result<String, DispatcherError> {
        let row = self
            .client
            .query_one(
                "SELECT fail_task($1, $2, $3, $4, $5)",
                &[
                    &lease.task_id,
                    &lease.worker_id,
                    &lease.epoch,
                    &result_ref,
                    &reason,
                ],
            )
            .map_err(DispatcherError::from_pg)?;
        Ok(row.get(0))
    }

    /// Issue #3: отказ воркера с явным целевым состоянием.
    ///
    /// Отличается от [`SwarmDispatcher::fail`] тем, что состояние не жёстко
    /// `blocked`, а выбирается вызывающим: `budget_exhausted` (исчерпан бюджет)
    /// или `failed` (сбой авторизации — нужна ротация ключа). Причина всё
    /// равно пишется в `block_reason`, а событие — в журнал.
    pub fn fail_with_state(
        &mut self,
        lease: &Lease,
        result_ref: Option<&str>,
        reason: &str,
        state: &str,
    ) -> Result<String, DispatcherError> {
        let row = self
            .client
            .query_one(
                "SELECT fail_task_state($1, $2, $3, $4, $5, $6)",
                &[
                    &lease.task_id,
                    &lease.worker_id,
                    &lease.epoch,
                    &result_ref,
                    &reason,
                    &state,
                ],
            )
            .map_err(DispatcherError::from_pg)?;
        Ok(row.get(0))
    }

    // ---------- ревью ----------

    /// Возвращает новое состояние задачи: `"done"`, `"ready"` или `"blocked"`.
    pub fn submit_review(
        &mut self,
        task_id: Uuid,
        artifact_ref: &str,
        reviewer: &str,
        verdict: Verdict,
        checks: &serde_json::Value,
    ) -> Result<String, DispatcherError> {
        let row = self
            .client
            .query_one(
                "SELECT submit_review($1, $2, $3, $4, $5::jsonb)",
                &[
                    &task_id,
                    &artifact_ref,
                    &reviewer,
                    &verdict.as_str(),
                    checks,
                ],
            )
            .map_err(DispatcherError::from_pg)?;
        Ok(row.get(0))
    }

    // ---------- сторож ----------

    pub fn reclaim_expired(&mut self) -> Result<i32, DispatcherError> {
        let row = self
            .client
            .query_one("SELECT reclaim_expired()", &[])
            .map_err(DispatcherError::from_pg)?;
        Ok(row.get(0))
    }

    // ---------- Issue #2: Cost ledger ----------

    /// Записать метрики LLM-вызова для точного учёта расходов.
    #[allow(clippy::too_many_arguments)]
    pub fn record_llm_call(
        &mut self,
        task_id: Uuid,
        worker_id: &str,
        lease_epoch: i64,
        model: &str,
        provider: &str,
        prompt_tokens: i64,
        completion_tokens: i64,
        cached_tokens: i64,
        prompt_cost_usd: f64,
        completion_cost_usd: f64,
        started_at: &str,
        completed_at: &str,
        duration_ms: i64,
        success: bool,
        error_code: Option<&str>,
        error_message: Option<&str>,
        http_status: Option<i32>,
    ) -> Result<(), DispatcherError> {
        let total_cost = prompt_cost_usd + completion_cost_usd;
        self.client
            .execute(
                "INSERT INTO llm_calls \
                 (task_id, worker_id, lease_epoch, model, provider, \
                  prompt_tokens, completion_tokens, cached_tokens, \
                  prompt_cost_usd, completion_cost_usd, total_cost_usd, \
                  started_at, completed_at, duration_ms, success, error_code, error_message, \
                  http_status) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9::float8::numeric, $10::float8::numeric, $11::float8::numeric, $12::text::timestamptz, $13::text::timestamptz, $14, $15, $16, $17, $18)",
                &[
                    &task_id,
                    &worker_id,
                    &lease_epoch,
                    &model,
                    &provider,
                    &prompt_tokens,
                    &completion_tokens,
                    &cached_tokens,
                    &prompt_cost_usd,
                    &completion_cost_usd,
                    &total_cost,
                    &started_at,
                    &completed_at,
                    &duration_ms,
                    &success,
                    &error_code,
                    &error_message,
                    &http_status,
                ],
            )
            .map_err(DispatcherError::from_pg)?;
        Ok(())
    }

    /// Issue #26: кандидаты на ревью через `review_candidates(n)`.
    ///
    /// Путь чтения для роли `swarm_reviewer`: прямой SELECT на `tasks`/`artifacts`
    /// ей закрыт (least privilege), витрина — SECURITY DEFINER.
    pub fn review_candidates(
        &mut self,
        limit: i32,
    ) -> Result<Vec<crate::types::ReviewCandidate>, DispatcherError> {
        let rows = self
            .client
            .query(
                "SELECT id, result_ref, contract, storage_path, created_by \
                 FROM review_candidates($1)",
                &[&limit],
            )
            .map_err(DispatcherError::from_pg)?;
        Ok(rows
            .iter()
            .map(|r| crate::types::ReviewCandidate {
                id: r.get(0),
                result_ref: r.get(1),
                contract: r.get(2),
                storage_path: r.get(3),
                created_by: r.get(4),
            })
            .collect())
    }

    /// Issue #3: суммарная стоимость всех LLM-вызовов (USD) из cost ledger.
    ///
    /// Используется hard budget guard перед каждым вызовом модели. Считается
    /// в микродолларах (целые), чтобы не терять мелкие расходы на f64.
    pub fn total_cost_usd(&mut self) -> Result<f64, DispatcherError> {
        let row = self
            .client
            .query_one(
                "SELECT COALESCE(SUM(total_cost_usd), 0)::float8 FROM llm_calls",
                &[],
            )
            .map_err(DispatcherError::from_pg)?;
        Ok(row.get(0))
    }

    // ---------- операторские ----------

    pub fn reopen(&mut self, task_id: Uuid, actor: &str) -> Result<i32, DispatcherError> {
        let row = self
            .client
            .query_one("SELECT reopen_task($1, $2)", &[&task_id, &actor])
            .map_err(DispatcherError::from_pg)?;
        Ok(row.get(0))
    }

    pub fn unblock(&mut self, task_id: Uuid, actor: &str) -> Result<String, DispatcherError> {
        let row = self
            .client
            .query_one("SELECT unblock_task($1, $2)", &[&task_id, &actor])
            .map_err(DispatcherError::from_pg)?;
        Ok(row.get(0))
    }

    pub fn verify_chain(&mut self, task_id: Uuid) -> Result<bool, DispatcherError> {
        let row = self
            .client
            .query_one("SELECT verify_chain($1)", &[&task_id])
            .map_err(DispatcherError::from_pg)?;
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
    /// Всё или ничего: `Err` из замыкания (или паника) = rollback, `Ok` = commit.
    pub fn with_transaction<F, T>(&mut self, f: F) -> Result<T, DispatcherError>
    where
        F: FnOnce(&mut DispatcherTx<'_, '_>) -> Result<T, DispatcherError>,
    {
        let mut tx = self
            .client
            .transaction()
            .map_err(DispatcherError::from_pg)?;
        let out = {
            let mut w = DispatcherTx { tx: &mut tx };
            f(&mut w)? // при Err tx дропается без commit -> rollback
        };
        tx.commit().map_err(DispatcherError::from_pg)?;
        Ok(out)
    }
}

impl DispatcherTx<'_, '_> {
    pub fn add_task(
        &mut self,
        task_id: Uuid,
        contract: &TaskContract,
        root_id: Option<Uuid>,
        budget_cap: i64,
        priority: i32,
    ) -> Result<(), DispatcherError> {
        contract.validate()?;
        let body = serde_json::to_value(contract)?;
        self.tx
            .execute(
                "SELECT add_task($1, $2, $3::jsonb, $4, $5)",
                &[&task_id, &root_id, &body, &budget_cap, &priority],
            )
            .map_err(DispatcherError::from_pg)?;
        Ok(())
    }

    pub fn add_dep(&mut self, task_id: Uuid, dep_id: Uuid) -> Result<(), DispatcherError> {
        self.tx
            .execute("SELECT add_dep($1, $2)", &[&task_id, &dep_id])
            .map_err(DispatcherError::from_pg)?;
        Ok(())
    }

    pub fn promote_ready(&mut self) -> Result<i32, DispatcherError> {
        let row = self
            .tx
            .query_one("SELECT promote_ready()", &[])
            .map_err(DispatcherError::from_pg)?;
        Ok(row.get(0))
    }
}
