use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use vir::{
    buffer::TextBuffer,
    recovery::{self, Recovery},
};

fn run(args: &[&Path], directory: &Path, input: &[u8]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_vir"))
        .arg("--ex")
        .args(args)
        .env("VIR_RECOVERY_DIR", directory)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn dirty_eof_retains_complete_input_and_discards_partial_line() {
    for named in [false, true] {
        for input in [
            b"a\ncomplete\n".as_slice(),
            b"a\ncomplete\npartial",
            b"a\ncomplete\n.\n",
        ] {
            let dir = tempfile::tempdir().unwrap();
            let original = dir.path().join("original");
            fs::write(&original, "old\n").unwrap();
            let args = if named {
                vec![original.as_path()]
            } else {
                vec![]
            };
            let result = run(&args, dir.path(), input);
            assert!(result.status.success(), "{:?}", result.stderr);
            assert!(result.stdout.is_empty());
            let snapshots = recovery::list(dir.path()).unwrap();
            assert_eq!(snapshots.len(), 1);
            assert!(
                String::from_utf8(result.stderr)
                    .unwrap()
                    .contains("Recovery snapshot retained at")
            );
            let expected = if named {
                "old\ncomplete\n"
            } else {
                "complete\n"
            };
            assert_eq!(
                recovery::read(&snapshots[0]).unwrap().buffer.to_text(),
                expected
            );
            assert_eq!(fs::read(&original).unwrap(), b"old\n");
            // Use the normal recovery entry and save to prove the retained file is usable.
            let export = dir.path().join("export");
            let restored = run(
                &[Path::new("--recover"), &snapshots[0]],
                dir.path(),
                format!("w {}\nq\n", export.display()).as_bytes(),
            );
            assert!(restored.status.success());
            assert_eq!(fs::read_to_string(export).unwrap(), expected);
            assert_eq!(recovery::list(dir.path()).unwrap(), snapshots);
        }
    }
}

#[test]
fn eof_after_deletion_failed_quit_or_recovery_keeps_unsaved_state() {
    for (script, expected, output) in [
        ("d\n", "", ""),
        ("c\n", "", ""),
        ("c\nnew\n.\nq\n", "new\n", "? unsaved changes\n"),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let original = dir.path().join("original");
        fs::write(&original, "old\n").unwrap();
        let result = run(&[&original], dir.path(), script.as_bytes());
        assert!(result.status.success());
        assert_eq!(result.stdout, output.as_bytes());
        let snapshots = recovery::list(dir.path()).unwrap();
        assert_eq!(snapshots.len(), 1);
        assert_eq!(
            recovery::read(&snapshots[0]).unwrap().buffer.to_text(),
            expected
        );
        let source_bytes = fs::read(&snapshots[0]).unwrap();
        let restored = run(&[Path::new("--recover"), &snapshots[0]], dir.path(), b"");
        assert!(restored.status.success());
        assert_eq!(fs::read(&snapshots[0]).unwrap(), source_bytes);
        let retained = recovery::list(dir.path()).unwrap();
        assert_eq!(retained.len(), 2);
        for path in retained {
            assert_eq!(recovery::read(&path).unwrap().buffer.to_text(), expected);
        }
        assert_eq!(fs::read(original).unwrap(), b"old\n");
    }
}

#[test]
fn explicit_quit_save_and_reload_clean_only_the_current_session() {
    for script in [
        "q\n",
        "a\ndiscard\n.\nq!\n",
        "a\nsaved\n.\nw\n",
        "a\ndiscard\n.\ne!\n",
        "a\nsaved\n.\nwq\n",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let original = dir.path().join("original");
        fs::write(&original, "old\n").unwrap();
        let mut other = Recovery::in_directory(dir.path().to_owned());
        other
            .checkpoint(
                None,
                &TextBuffer::from_text("other session\n").unwrap(),
                true,
            )
            .unwrap();
        let result = run(&[&original], dir.path(), script.as_bytes());
        assert!(result.status.success(), "{:?}", result.stderr);
        assert!(result.stdout.is_empty());
        assert!(result.stderr.is_empty());
        assert_eq!(
            recovery::list(dir.path()).unwrap(),
            vec![other.path().unwrap()]
        );
        assert_eq!(
            recovery::read(other.path().unwrap())
                .unwrap()
                .buffer
                .to_text(),
            "other session\n"
        );
        let expected = if script.contains("saved") {
            "old\nsaved\n"
        } else {
            "old\n"
        };
        assert_eq!(fs::read_to_string(original).unwrap(), expected);
    }
}

#[test]
fn failed_checkpoint_at_eof_does_not_claim_a_snapshot_was_retained() {
    let dir = tempfile::tempdir().unwrap();
    let result = run(&[], &dir.path().join("missing"), b"a\nnew\n");
    assert!(result.status.success());
    assert!(
        String::from_utf8(result.stdout)
            .unwrap()
            .contains("recovery unavailable")
    );
    assert!(result.stderr.is_empty());
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
}
