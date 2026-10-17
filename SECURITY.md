# Security Policy

## Reporting a vulnerability

**Do not open a public issue.**

Use GitHub's private vulnerability reporting:

<https://github.com/xodapi/swarm-dispatcher-public/security/advisories/new>

If that is unavailable, contact the maintainer directly and include
`SECURITY` in the subject.

Please include:

- Affected component (crate, SQL function, script, workflow)
- Reproduction steps or a proof-of-concept
- Impact assessment: what an attacker gains
- Whether the issue is already public anywhere

You can expect an acknowledgement within 72 hours and an assessment within
seven days. Please allow time for a fix before public disclosure.

## Scope

This is a reference implementation, and its security posture is documented
rather than assumed. The following are **known and accepted** limitations, not
vulnerabilities — see [docs/KNOWN_LIMITATIONS.md](docs/KNOWN_LIMITATIONS.md):

| Accepted limitation | Why it is not a vulnerability report |
|---|---|
| Worker identity is a Postgres role, not mTLS | Documented design. A host with valid credentials is trusted by definition. |
| Event log is tamper-*evident*, not tamper-*proof* | The hash chain detects rewriting; no trigger blocks `UPDATE`/`DELETE` yet. Documented. |
| No rate limiting, no OPA, no Ed25519 approvals | Deferred by design to later versions. |
| Single Postgres is a bottleneck | Availability limitation, not a security boundary. |
| No OpenTelemetry, no alerts | Observability gap, documented. |

Genuine vulnerabilities we do want to hear about:

- **Sandbox escape** — a shell tool or agent loop path that executes outside the
  container, or bypasses the command allowlist (including via `git config`
  aliases, `cargo xtask`, or a written cargo config that redirects `rustc`).
- **Privilege escalation across roles** — a worker acting on another worker's
  task, a reviewer approving its own artefact, a role gaining grants it should
  not have.
- **Review gate bypass** — making `submit_review` accept checks that were never
  observed.
- **Secret leakage** — credentials reaching the repository, logs, an audit
  archive, or CI output.
- **SQL injection or unsafe dynamic SQL** in schema functions.
- **Cost ledger bypass** — spending past `MAX_TOTAL_COST_USD`.

## What the project already does

- Deny-by-default shell allowlist with subcommand whitelists
- Sandbox: `cap_drop: ALL`, `read_only`, `network: none`, resource limits
- Least-privilege Postgres roles, `SECURITY DEFINER` where required
- Secret scanning in a pre-commit hook and in CI, self-tested
- Dependabot alerts enabled; advisories triaged and pinned in `Cargo.lock`

## Disclosure policy

We follow coordinated disclosure: fix, release, then publish an advisory with
credit to the reporter unless anonymity is requested.
