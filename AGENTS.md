# Human–AI development

vir is developed collaboratively by its human maintainer and AI coding agents. The maintainer initiates work, sets direction, and makes scope decisions. AI has written much of the code, tests, and documentation, and has participated extensively in debugging and review. This repository should describe that contribution honestly.

AI-generated code and AI reviews require the same scrutiny as other contributions. Passing tests or repeated reviews are evidence within their coverage, not proof of correctness or independent human review.

## Project scope

- Build a small, modern vi reimplementation in Rust. Preserve modal editing, operators and motions, registers, macros, autoindent, undo, dot repeat, local marks, and basic ex operations.
- Complete historical vi/ex compatibility is not a release requirement. Rust regex, UTF-8/LF text, Unicode scalar positions, terminal paste, conflict detection, and recovery snapshots are deliberate adaptations.
- Keep configuration scripting, shell/filter integration, plugins, LSP, multiple windows, and system clipboard integration outside the current scope unless the maintainer explicitly changes it.
- Use original vi source and reproducible behavior when investigating compatibility. Distinguish defects from accepted simplifications; do not treat current code or tests as the definition of correct behavior.
- Prefer focused changes with a concrete reason. Do not restart broad refactoring or expand features just to keep development moving.

## Before making changes

1. Read the README, relevant runtime help, and the code being changed.
2. Check the branch, HEAD, recent commits, working tree, and staged changes. Preserve unrelated work and user data, including any untracked text or recovery snapshots.
3. If local `docs/` exists, read `docs/agent/CODEX.md` and the current handoff before continuing. These optional local records do not ship with a clone; their absence must not prevent ordinary repository work.
4. State the intended change and relevant uncertainty. Follow the maintainer's latest instructions; do not turn historical notes into new tasks.

## Implementation and review

- Advance one reviewable behavior or mechanism at a time. Explain what changed, why, how it was checked, and what remains unverified.
- Preserve text, undo, registers, cursor state, and recovery data according to each operation's contract. Pay particular attention to failure paths and resource limits.
- When documentation and implementation disagree, evaluate which behavior better fits the project before deciding which to change.
- Keep implementation, tests, and runtime help consistent. Do not claim tests were run when they were only planned or inherited from an earlier session.
- Use English for new public documentation and the local design and operations manual.

## Verification

Run checks appropriate to the change. Typical code checks are:

```sh
cargo test --offline --locked
cargo test --release --offline --locked
cargo fmt --check
cargo clippy --all-targets --all-features --offline --locked -- -D warnings
cargo build --offline --locked
```

Offline commands require cached dependencies. For terminal changes, run the relevant Python 3 PTY suites under `tests/` on Unix after building. Run `tests/put_budget_pty.py` for the controlled memory checks; do not run all ignored tests without their required terminal or memory-limit wrappers.

For documentation-only changes, check accuracy, links, formatting, and Git inclusion instead of rerunning unrelated code tests.

## Documentation and commits

- Track this `AGENTS.md`, the concise `README.md`, and `src/help.txt`. The runtime reference remains available through `vir --help`.
- Keep detailed design, the operations manual, research, progress, and session handoffs in ignored local `docs/`. Do not force-add those files or make published documentation depend on them.
- When local handoff records exist, update them at reviewable milestones and before ending a session. Record the code baseline, completed and remaining work, validation results, and protected changes.
- Commit completed changes in reviewable stages. Inspect the staged diff before committing; exclude user files, recovery snapshots, build artifacts, and local documentation.
- Do not rewrite history or delete recovery data as part of routine cleanup.
