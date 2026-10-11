use std::io::Write;
use std::process::{Command, Stdio};

fn run(args: &[&str], input: &[u8]) -> std::process::Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_vir"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn ex_loads_file_and_executes_commands_without_prompts() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml");
    let output = run(&["--ex", path], b"1p\nq\n");
    assert!(output.status.success());
    assert_eq!(output.stdout, b"[package]\n");
    assert!(output.stderr.is_empty());
}

#[test]
fn ex_without_file_accepts_quit_and_clean_eof() {
    for input in [b"q\n".as_slice(), b""] {
        let output = run(&["--ex"], input);
        assert!(output.status.success());
        assert!(output.stdout.is_empty());
        assert!(output.stderr.is_empty());
    }
}

#[test]
fn ex_rejects_initial_load_errors() {
    let output = run(&["--ex", env!("CARGO_MANIFEST_DIR")], b"");
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(!output.stderr.is_empty());
}

#[test]
fn ex_edits_saves_and_reopens_a_new_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("new file");
    let args = ["--ex", path.to_str().unwrap()];
    let output = run(
        &args,
        b"a\none\ntwo\n.\n1i\nhead\n.\n2c\nONE\n.\n3d\nw\nq\n",
    );
    assert!(output.status.success());
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "head\nONE\n");
    let reopened = run(&args, b"%p\nq\n");
    assert!(reopened.status.success());
    assert_eq!(reopened.stdout, b"head\nONE\n");
    assert!(reopened.stderr.is_empty());
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
}

#[test]
fn ex_failed_save_preserves_edits_and_recovers_to_commands() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("missing-parent").join("new");
    let output = run(
        &["--ex", path.to_str().unwrap()],
        b"a\nkept\n.\nw\nq\n%p\nu\nu\n%p\nq!\n",
    );
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.starts_with("? "));
    assert!(stdout.ends_with("? unsaved changes\nkept\nkept\n"));
    assert!(!path.exists());
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
}

#[test]
fn ex_quit_protects_unsaved_changes_and_force_quit_leaves_disk_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("original");
    std::fs::write(&path, "original\n").unwrap();
    let output = run(
        &["--ex", path.to_str().unwrap()],
        b"c\nunsaved\n.\nq\n%p\nq!\n",
    );
    assert!(output.status.success());
    assert_eq!(output.stdout, b"? unsaved changes\nunsaved\n");
    assert_eq!(std::fs::read_to_string(path).unwrap(), "original\n");
}

#[test]
fn ex_save_then_undo_requires_another_save_before_quitting() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("original");
    std::fs::write(&path, "original\n").unwrap();
    let output = run(
        &["--ex", path.to_str().unwrap()],
        b"a\nnew\n.\nw\nu\nq\n%p\nw\nq\n",
    );
    assert!(output.status.success());
    assert_eq!(output.stdout, b"? unsaved changes\noriginal\n");
    assert_eq!(std::fs::read_to_string(path).unwrap(), "original\n");
}

#[test]
fn ex_script_eof_does_not_implicitly_save() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("original");
    std::fs::write(&path, "original\n").unwrap();
    let output = run(&["--ex", path.to_str().unwrap()], b"a\nunsaved\npartial");
    assert!(output.status.success());
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("Recovery snapshot retained at")
    );
    assert_eq!(std::fs::read_to_string(path).unwrap(), "original\n");
    let snapshots = vir::recovery::list(dir.path()).unwrap();
    assert_eq!(snapshots.len(), 1);
    assert_eq!(
        vir::recovery::read(&snapshots[0]).unwrap().buffer.to_text(),
        "original\nunsaved\n"
    );
}

#[test]
fn ex_file_workflow_and_failed_wq_continue_the_session() {
    let dir = tempfile::tempdir().unwrap();
    let first = dir.path().join("first file");
    let second = dir.path().join("second");
    std::fs::write(&second, "second\n").unwrap();
    let script = format!(
        "a\nfirst\n.\nwq {}/missing/child\n%p\nw {}\ne {}\n%p\na\nextra\n.\ne\n%p\ne!\n%p\nwq\n",
        dir.path().display(),
        first.display(),
        second.display()
    );
    let output = run(&["--ex"], script.as_bytes());
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.starts_with("? "));
    assert!(stdout.ends_with("first\nsecond\n? unsaved changes\nsecond\nextra\nsecond\n"));
    assert_eq!(std::fs::read_to_string(first).unwrap(), "first\n");
    assert_eq!(std::fs::read_to_string(second).unwrap(), "second\n");
}

#[test]
fn ex_partial_write_quit_keeps_session_open_until_full_save() {
    let dir = tempfile::tempdir().unwrap();
    let current = dir.path().join("current");
    let export = dir.path().join("export");
    std::fs::write(&current, "one\ntwo\n").unwrap();
    let script = format!("a\nthree\n.\n1w\n1wq {}\n%p\n%wq\n", export.display());
    let output = run(&["--ex", current.to_str().unwrap()], script.as_bytes());
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    assert_eq!(
        output.stdout,
        b"? cannot write partial buffer to current file\n? unsaved changes\none\ntwo\nthree\n"
    );
    assert_eq!(std::fs::read_to_string(export).unwrap(), "one\n");
    assert_eq!(
        std::fs::read_to_string(current).unwrap(),
        "one\ntwo\nthree\n"
    );
}

#[test]
fn ex_cli_file_list_switches_and_marks_current_argument() {
    let dir = tempfile::tempdir().unwrap();
    let first = dir.path().join("first file");
    let second = dir.path().join("second");
    std::fs::write(&first, "first\n").unwrap();
    std::fs::write(&second, "second\n").unwrap();
    let output = run(
        &["--ex", first.to_str().unwrap(), second.to_str().unwrap()],
        b"args\nq\na\nunsaved\n.\nn\nn!\nargs\n%p\nn\nq\n",
    );
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        format!(
            "[{}] {}\n? 1 more file to edit\n? unsaved changes\n{} [{}]\nsecond\n? no more files\n",
            first.display(),
            second.display(),
            first.display(),
            second.display()
        )
    );
    assert_eq!(std::fs::read_to_string(first).unwrap(), "first\n");
}

#[test]
fn ex_search_addresses_print_edit_and_recover_from_no_match() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("file");
    std::fs::write(&path, "one\ntarget\nlast\n").unwrap();
    let output = run(
        &["--ex", path.to_str().unwrap()],
        b"/target/\n/missing/\n??\n/target/d\n%p\nu\n//\nq!\n",
    );
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    assert_eq!(
        output.stdout,
        b"target\n? pattern not found\ntarget\none\nlast\ntarget\n"
    );
    assert_eq!(
        std::fs::read_to_string(path).unwrap(),
        "one\ntarget\nlast\n"
    );
}

#[test]
fn ex_substitution_undo_save_and_error_recovery() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("file");
    std::fs::write(&path, "cat cat\ncat\n").unwrap();
    let output = run(
        &["--ex", path.to_str().unwrap()],
        b"%s/cat/dog/g\n%p\nu\n%p\nu\ns/missing/x/\n%wq\n",
    );
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    assert_eq!(
        output.stdout,
        b"dog dog\ndog\ncat cat\ncat\n? pattern not found\n"
    );
    assert_eq!(std::fs::read_to_string(path).unwrap(), "dog dog\ndog\n");
}

#[test]
fn help_explains_supported_workflow_without_starting_editor() {
    for flag in ["--help", "-h"] {
        let output = run(&[flag], b"");
        assert!(output.status.success());
        assert!(output.stderr.is_empty());
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(text.contains("vir --ex [file ...]"));
        assert!(text.contains("EOF exits without saving"));
        assert!(text.contains("not historical vi BRE"));
    }
}

#[test]
fn s4_combined_session_preserves_export_undo_and_file_switch_state() {
    let dir = tempfile::tempdir().unwrap();
    let first = dir.path().join("first");
    let second = dir.path().join("second");
    let part = dir.path().join("part");
    std::fs::write(&first, "cat cat\ncat\n").unwrap();
    std::fs::write(&second, "cat next\n").unwrap();
    let script = format!(
        "%s/cat/dog/g\n1w {}\nn\nu\n%p\nu\n%w\nn\n//\ns//fox/g\nwq {}/missing/file\nq\n%p\nwq\n",
        part.display(),
        dir.path().display()
    );
    let output = run(
        &["--ex", first.to_str().unwrap(), second.to_str().unwrap()],
        script.as_bytes(),
    );
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.starts_with("? unsaved changes\ncat cat\ncat\ncat next\n? "));
    assert!(text.ends_with("? unsaved changes\nfox next\n"));
    assert_eq!(std::fs::read_to_string(first).unwrap(), "dog dog\ndog\n");
    assert_eq!(std::fs::read_to_string(second).unwrap(), "fox next\n");
    assert_eq!(std::fs::read_to_string(part).unwrap(), "dog dog\n");
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 3);
}

#[test]
fn visual_rejects_pipes_and_extra_files_without_terminal_sequences() {
    for args in [vec!["--vi"], vec!["--vi", "one", "two"]] {
        let output = run(&args, b"");
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(!output.stderr.contains(&0x1b));
    }
}
