#![cfg(unix)]

use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::Write;
use std::os::unix::ffi::OsStringExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use vir::{buffer::TextBuffer, recovery::Recovery};

fn native_path(directory: &Path, name: &[u8]) -> PathBuf {
    directory.join(OsString::from_vec(name.to_vec()))
}

fn run(args: &[&OsStr], directory: &Path, input: &[u8]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_vir"))
        .args(args)
        .env("VIR_RECOVERY_DIR", directory)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let _ = child.stdin.take().unwrap().write_all(input);
    child.wait_with_output().unwrap()
}

#[test]
fn native_file_arguments_open_switch_and_save_without_lossy_aliases() {
    let dir = tempfile::tempdir().unwrap();
    let first = native_path(dir.path(), b"first-\xff");
    let second = native_path(dir.path(), b"second-\xfe");
    let alias = PathBuf::from(first.to_string_lossy().into_owned());
    fs::write(&first, "first\n").unwrap();
    fs::write(&second, "second\n").unwrap();
    fs::write(&alias, "do not touch\n").unwrap();
    let result = run(
        &[OsStr::new("--ex"), first.as_os_str(), second.as_os_str()],
        dir.path(),
        b"%p\ns/first/FIRST/\nw\nn\n%p\nq\n",
    );
    assert!(result.status.success(), "{:?}", result.stderr);
    assert!(result.stderr.is_empty());
    assert_eq!(result.stdout, b"first\nsecond\n");
    assert_eq!(fs::read(&first).unwrap(), b"FIRST\n");
    assert_eq!(fs::read(&second).unwrap(), b"second\n");
    assert_eq!(fs::read(&alias).unwrap(), b"do not touch\n");
}

#[test]
fn native_snapshot_and_directory_arguments_list_and_recover() {
    let dir = tempfile::tempdir().unwrap();
    let directory = native_path(dir.path(), b"recovery-\xff");
    fs::create_dir(&directory).unwrap();
    let mut journal = Recovery::in_directory(directory.clone());
    journal
        .checkpoint(None, &TextBuffer::from_text("rescued\n").unwrap(), true)
        .unwrap();
    let snapshot = native_path(&directory, b".vir-recovery-native-\xfe");
    fs::rename(journal.path().unwrap(), &snapshot).unwrap();
    let source = fs::read(&snapshot).unwrap();
    let listed = run(
        &[OsStr::new("--recover-list"), directory.as_os_str()],
        &directory,
        b"",
    );
    assert!(listed.status.success(), "{:?}", listed.stderr);
    assert!(
        String::from_utf8(listed.stdout)
            .unwrap()
            .contains("bytes=8")
    );
    let result = run(
        &[
            OsStr::new("--ex"),
            OsStr::new("--recover"),
            snapshot.as_os_str(),
        ],
        &directory,
        b"%p\nq!\n",
    );
    assert!(result.status.success(), "{:?}", result.stderr);
    assert_eq!(result.stdout, b"rescued\n");
    assert_eq!(fs::read(&snapshot).unwrap(), source);
    assert_eq!(fs::read_dir(&directory).unwrap().count(), 1);
}

#[test]
fn native_legacy_paths_and_invalid_modes_do_not_panic() {
    let dir = tempfile::tempdir().unwrap();
    let file = native_path(dir.path(), b"text-\xff");
    fs::write(&file, "legacy\n").unwrap();
    let result = run(&[OsStr::new("unused"), file.as_os_str()], dir.path(), b"");
    assert!(result.status.success(), "{:?}", result.stderr);
    assert_eq!(result.stdout, b"legacy\n");
    let invalid = OsString::from_vec(b"--\xff".to_vec());
    let result = run(&[&invalid], dir.path(), b"");
    assert_eq!(result.status.code(), Some(1));
    assert!(String::from_utf8(result.stderr).unwrap().contains("Usage:"));
}

#[test]
fn native_visual_path_reaches_terminal_validation() {
    let dir = tempfile::tempdir().unwrap();
    let file = native_path(dir.path(), b"visual-\xff");
    fs::write(&file, "original\n").unwrap();
    for args in [
        vec![OsStr::new("--vi"), file.as_os_str()],
        vec![file.as_os_str()],
    ] {
        let result = run(&args, dir.path(), b"");
        assert_eq!(result.status.code(), Some(1));
        assert!(result.stdout.is_empty());
        assert!(
            String::from_utf8(result.stderr)
                .unwrap()
                .contains("requires a terminal")
        );
        assert_eq!(fs::read(&file).unwrap(), b"original\n");
    }
}
