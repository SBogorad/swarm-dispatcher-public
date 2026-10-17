# Pull request

## What changed

<!-- One sentence. What behaviour differs after this PR? -->

## Why

<!-- The problem. If it is a defect, how does it manifest? -->

## How it was verified

<!-- Paste the commands you ran and their outcome. Claims must be reproducible
     from the diff — see CONTRIBUTING.md. -->

```
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --lib --locked
scripts/acceptance_harness.sh --base main --branch HEAD \
  --full-workspace-test --allowlist-file scripts/acceptance_allowlist.txt
```

## Checklist

- [ ] One logical change — no unrelated refactors
- [ ] New behaviour has a test that fails without this change
- [ ] The STRICT gate passes locally
- [ ] `scripts/acceptance_allowlist.txt` updated if new paths are touched
- [ ] `docs/KNOWN_LIMITATIONS.md` updated if a limit changed
- [ ] `MANIFEST.md` updated if a path was added or removed
- [ ] `CHANGELOG.md` updated under `[Unreleased]` for user-visible changes
- [ ] No secrets in the diff (pre-commit hook is not bypassed)

## Breaking changes

<!-- Contract changes, migration requirements, or "none". -->
