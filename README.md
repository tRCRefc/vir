# vir

A small vi reimplementation in Rust, built as an exercise in understanding a classic editor. It provides modal editing, composable operators and motions, registers, macros, undo, dot repeat, and an ex frontend. It does not aim for complete historical vi/ex compatibility.

This is a human–AI collaboration: the maintainer directs the project and makes scope decisions, while AI has written much of the implementation, tests, and documentation. See [AGENTS.md](AGENTS.md) for development guidelines.

## Build and run

Requires Rust 1.88+ and Cargo. Visual mode requires terminal stdin and stdout; terminal behavior has been tested on Linux using Unix PTYs.

```sh
cargo build --release --locked

target/release/vir --vi notes.txt    # Visual editor
target/release/vir --ex notes.txt    # Ex commands, including piped input
target/release/vir --help            # Command reference
```

To install locally, run `cargo install --path . --locked`, then use `vir`.

## Basic editing

- `i` enters Insert; Esc returns to Normal. Move with `h/j/k/l` or arrow keys.
- `x` deletes a character, `dd` deletes a line, `u` toggles the last edit, and `.` repeats an edit.
- `/pattern` searches; `n` finds the next match.
- `:w` saves, `:wq` saves and quits, `:q` checks before quitting, and `:q!` discards changes. Press Enter to execute.
- Use `:w PATH` to save an unnamed buffer. Standalone ex commands omit the leading colon.

See `vir --help` or the [runtime reference](src/help.txt) for commands and behavior details.

## Files and recovery

- Text must be UTF-8 with LF line endings; CRLF files are rejected. Positions use Unicode scalars. Whole-line insertion and ex change/substitute can add a final LF.
- Searches use Rust regex syntax. Configuration scripts, shell/filter integration, the system clipboard, and multiple windows are outside the current scope.
- Saving checks for external changes and rejects unsupported targets such as symbolic links. On conflict, use `:w NEW_PATH` to export to a new file, or `:e!` to discard edits and reload.
- CLI sessions maintain recovery snapshots beside the initial file or in the working directory. `VIR_RECOVERY_DIR` can select an existing directory.

```sh
vir --recover-list DIRECTORY
vir --ex --recover SNAPSHOT
# Enter: w NEW_PATH, then q
```

Ex EOF does not save the original file; it retains the last successful snapshot of unsaved text. `q!` removes this session's snapshot. Ex command errors print `? ...` and do not necessarily produce a nonzero exit status. Recovery source snapshots remain intact; remove them after checking the recovered text. Recovery is not a complete power-loss guarantee.

## Development checks

```sh
cargo test --locked
cargo fmt --check
cargo clippy --all-targets --all-features --locked -- -D warnings
```

Terminal checks are in `tests/*_pty.py`; run them with Python 3 on Unix after building the debug binary.

## License

Licensed under the [MIT License](LICENSE).
