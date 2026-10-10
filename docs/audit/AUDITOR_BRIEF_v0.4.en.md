# Audit brief: `swarm-dispatcher` v0.4

**To:** an independent auditor (an LLM with no repository access).
**Input:** XML volumes with the full source code, infrastructure, and documentation.
**Task:** find correctness, security, and concurrency defects — with evidence from the code.

---

## 1. What this project is

`swarm-dispatcher` is a distributed task scheduler on PostgreSQL. The queue, the
state machine, and the access rules live **in the database** (`SECURITY DEFINER`
SQL functions), not in the application. The Rust processes are thin clients of
that queue.

Components (crates):

| Crate | Role |
|-------|------|
| `swarm-core` | Data model, dispatcher, transactions, types |
| `swarm-planner` | Task decomposition via an LLM (Planner) |
| `swarm-worker` | Subtask execution, artifacts, retry |
| `swarm-watchdog` | Returns expired leases to the queue |
| `swarm-cli` | Operator interface, review |
| `swarm-llm` | LLM provider client |

**Project constraints (respect them in your findings):**
- **Synchronous** code only, no async/tokio.
- PostgreSQL only. The DB schema is the source of truth.
- The public `swarm-core` API must not change (audit, not refactoring).

---

## 2. How to read the package

Volumes (all inside the ZIP next to this brief):

| Volume | File | Contents |
|--------|------|----------|
| `CODE` | `PROJECT_CODE_BUNDLE_v0.4.xml` | Rust: 6 crates, src + tests + fixtures, Cargo |
| `INFRA` | `PROJECT_INFRA_BUNDLE_v0.4.xml` | SQL (schema/roles/patches), init scripts, Dockerfile, compose, `.env.example` |
| `DOCS_CORE` | `PROJECT_DOCS_CORE_BUNDLE_v0.4.xml` | Specs, architecture, known limitations, reports |
| `DOCS` | `PROJECT_DOCS_BUNDLE_v0.4.xml` | The above + methodology, decision history, correspondence |

**Recommended order:**
1. `INFRA` → `schema/schema_v0.sql`, `step2/roles.sql`, `step2b/roles_2b.sql` — first understand
   the state machine and the privileges.
2. `CODE` → `swarm-core` (dispatcher), then `swarm-worker`, `swarm-watchdog`, `swarm-planner`.
3. `DOCS_CORE` → `ARCHITECTURE.md`, `KNOWN_LIMITATIONS.md` — what the authors **claim**.
4. The rest as needed.

**Integrity:** `CHECKSUMS_v0.4.txt` (sha256 per volume). Every file inside a volume carries
`sha256` and `md5` in `<manifest>`; the full text is in `<content>`.

**Format:** `<file path="…" sha256="…"><content><![CDATA[ …text… ]]></content></file>`.

---

## 3. What to check

### 3.1. State machine and DB invariants
Files: `schema/schema_v0.sql`, `step2/roles.sql`, `step2b/roles_2b.sql`, `crates/swarm-core/src/dispatcher.rs`.

- Are **orphan states** possible: a task `claimed` with no live lease, `done` without an artifact,
  `failed` without a reason?
- Are there transitions the function **allows** but the state machine should not?
- Is `claim` atomic? Check `FOR UPDATE SKIP LOCKED` and the selection predicate — is there a window
  between selecting the row and updating it?
- Can one task be handed to **two workers**?
- Are `lease_expires_at` and the system clock consistent (time-skew vulnerability)?

### 3.2. Concurrency: watchdog vs worker
Files: `crates/swarm-watchdog/src/main.rs`, `crates/swarm-worker/src/worker.rs`.

- Can the watchdog **steal** a task from a live worker (false timeout)?
- What happens if the worker finishes **after** the watchdog requeued the task?
  A duplicate artifact write? A lost result?
- Is there protection against a "zombie" worker (process alive but not progressing)?

### 3.3. Role privileges (least privilege)
Files: `schema/roles.sql`, `step2/roles.sql`, `step2/roles_patch_planner_status.sql`, `step2b/roles_2b.sql`.

- Does each role have **only what it needs**? Is there a direct `INSERT`/`UPDATE` into tables that
  bypasses the functions?
- `SECURITY DEFINER` functions: is `search_path` pinned? Otherwise — privilege escalation via object
  shadowing.
- Can `swarm_worker_w1` call the planner functions (`swarm_planner`)?
- The grant patch ships as a separate file — **check the order** `schema → roles → roles_2b →
  roles_patch_planner_status` and that, without the patch, nothing breaks except what is expected.

### 3.4. Planner: trust in the LLM
Files: `crates/swarm-planner/src/parse.rs`, `lib.rs`, `prompt.rs`.

- What happens on **garbage/incomplete** JSON from the model? Is there a retry? Can it be an
  infinite loop?
- **Prompt injection**: the task text goes into the prompt inside `<untrusted>…</untrusted>` and is
  stripped from the JSON contract. Is that enough? Find a path where the description influences the
  plan structure.
- Cyclic dependencies in the plan: are they checked?
- Is there a cap on the number of subtasks (protection against a "10,000-step plan")?

### 3.5. Worker: artifacts and retry
Files: `crates/swarm-worker/src/artifacts.rs`, `worker.rs`, `retry.rs`, `prompt.rs`.

- Is the artifact `sha256` computed **before** the DB write? Can the DB and the disk diverge (artifact
  written, hash not — and vice versa)?
- Retry: how many attempts? What happens on a failure **mid-way** — is a partial artifact left behind?
- Is the artifact of a previous attempt overwritten?

### 3.6. Tests: honesty of coverage
Files: `crates/*/tests/*.rs`, `tests/test_v0.py`, `step2/test_roles.py`, `step2b/test_roles_2b.py`.

- Do the integration tests really hit PostgreSQL, or do they substitute mocks?
- Are there **concurrency** tests (two workers, watchdog + worker), or is everything sequential?
- The reports claim mutation testing M1–M7. Check against the tests: is it reproducible? Which
  mutants **survive** (the tests do not catch them)?
- Find claims in the reports that the code **does not support**.

### 3.7. Documentation vs code
Files: `README.md`, `docs/KNOWN_LIMITATIONS.md`, `docs/ARCHITECTURE.md`, `REPORT_*.md`.

- Verify the **honesty** of the claims. Particular attention: `KNOWN_LIMITATIONS.md` — are all
  limitations really listed, or is something withheld?
- `docs/TZ_v0.4_shell_tools_sandbox.md` — the spec for shell tools and the Docker sandbox.
  **Its implementation is not in this bundle** (expected; next version). Check only that the spec is
  self-consistent.

---

## 4. What not to do

- Do not rewrite the code wholesale, do not propose general refactoring.
- Do not comment on style, formatting, or naming.
- Do not invent problems without a **quote from the code**.
- Do not equate "tests pass" with "code is correct". Inspect the tests themselves.

---

## 5. Answer format

One finding — one block:

```
### [SEVERITY] Short title
- File: path:line
- Essence: what is wrong and why it is a defect
- Evidence: code quote (5–15 lines)
- Reproduction / verification: how to confirm it
- Proposal: the minimal fix
```

`SEVERITY`: `CRITICAL` (data loss/corruption, privilege escalation) · `HIGH` (race, invariant bypass) ·
`MEDIUM` (error handling, resources) · `LOW` (minor) · `INFO` (observation).

At the end — a summary: how many findings per severity, and the **three main risks** of the project.

If a section has no findings, say so explicitly: "checked, clean".

---

## 6. Package verification

```bash
sha256sum -c CHECKSUMS_v0.4.txt          # volume integrity
unzip -l swarm-dispatcher_audit_v0.4.zip # contents
```

The volumes are reproducible from the repository:
`python3 scripts/build_project_bundle.py && python3 scripts/build_audit_index.py`.
