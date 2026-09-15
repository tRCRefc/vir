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
