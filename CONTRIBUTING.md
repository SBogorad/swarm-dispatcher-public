# Contributing

Thanks for considering a contribution. This document explains how work moves
from an idea to `main`, and what will be asked of your pull request.

## Ground rules

1. **One logical change per pull request.** A refactor and a bug fix in the same
   PR cannot be reviewed or reverted independently.
2. **`main` is protected in practice, not just by convention.** Work happens on
   a branch; the STRICT gate must be green before merge.
3. **Every claim in a commit message must be reproducible from the diff.** If you
   write "fixes X", a reviewer should be able to verify X from the change alone.
   Unverifiable claims are treated as noise and sent back.
4. **No secrets, ever.** The pre-commit hook scans the diff against real values
   from your local `.env*`. Do not bypass it with `--no-verify`.

## Setup

```bash
git clone https://github.com/xodapi/swarm-dispatcher-public.git
cd swarm-dispatcher-public

# Pre-commit and pre-push hooks (secret scan)
git config core.hooksPath .githooks

cargo build --release
```

Requirements: Rust 1.94.1+, PostgreSQL 16+, Docker (for the full gate).

## The gate

Everything is verified by one script, which is the exact mirror of CI:

```bash
scripts/acceptance_harness.sh --base main --branch HEAD \
  --full-workspace-test --allowlist-file scripts/acceptance_allowlist.txt
```

A green run means: SQL invariants hold on a disposable database, RBAC is intact,
the review gate cannot be bypassed, sandbox boundaries hold, mutation testing
found no survivors, formatting is clean, and the documentation has no dead links.

**A step that cannot run is `BLOCKED`, not `PASS`.** If your environment lacks
Docker or a database, the gate will tell you so rather than quietly succeeding.

### If the gate rejects your paths

The gate compares `git diff --name-only base..HEAD` against
`scripts/acceptance_allowlist.txt` and fails on any path outside it. This is
deliberate: it makes scope creep visible. If your change legitimately touches a
new path, add it to the allowlist **in the same PR**, with a comment explaining
why.

## Pull request checklist

- [ ] One logical change; the title says what changed, not "fixes"
- [ ] `cargo fmt --all -- --check` passes
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` passes
- [ ] `cargo test --workspace --lib --locked` passes
- [ ] The STRICT gate passes locally
- [ ] New behaviour has a test that fails without your change
- [ ] Documentation updated if behaviour or limits changed
- [ ] No new entries needed in `docs/KNOWN_LIMITATIONS.md` — or they are added

## Writing tests that mean something

This project has a specific standard, born from real defects:

- **A test that cannot fail is a defect.** A negative control that greps for a
  symbol in the wrong mangled form passed for months while a real leak existed.
  Assert on a positive control first, then the negative one.
- **Test the failure mode, not the happy path.** If you add a limit, prove that
  exceeding it is rejected.
- **Prefer a behavioural arbiter over a unit test** when the invariant lives in
  SQL. The Python arbiters in `tests/` run against a real database for exactly
  this reason.

## Documentation

- `README.md` — entry point, English
- `README.ru.md` — entry point, Russian
- `MANIFEST.md` — map of every path; update it when you add one
- `docs/KNOWN_LIMITATIONS.md` — the honest edge list
- `CHANGELOG.md` — user-visible changes, under `[Unreleased]`

Docs are checked for dead links by `tests/test_docs_link_drift.py`. If you
reference a file, it must exist. If you must reference something that does not
exist (a planned file, a deleted artefact), add it to the `ALLOW` map in that
test with a reason — and remove the entry once it is no longer needed.

## Commit messages

Present tense, imperative, and specific:

```
fix(planner): normalize multi-root plans before validation

A flash-tier model sometimes emits two roots for a single task; validate_graph
rejected the plan outright, so ~40% of runs failed. normalize_multi_root welds
extra roots to the first one before validation.
```

Prefixes in use: `feat`, `fix`, `chore`, `docs`, `test`, `refactor`, `perf`.

## Review

Reviews check three things in order: **does it do what it claims**, **does it
break anything**, **is it the simplest thing that works**. Expect questions about
the third.

## License

By contributing you agree that your contribution is licensed under the MIT
License, see [LICENSE](LICENSE).
