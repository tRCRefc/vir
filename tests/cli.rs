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
fn ex_rejects_extra_arguments_and_load_errors() {
    for args in [
        vec!["--ex", "one", "two"],
        vec!["--ex", env!("CARGO_MANIFEST_DIR")],
    ] {
        let output = run(&args, b"");
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(!output.stderr.is_empty());
    }
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
    assert!(output.stderr.is_empty());
    assert_eq!(std::fs::read_to_string(path).unwrap(), "original\n");
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
