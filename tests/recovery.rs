use std::fs;
use std::io::{self, Cursor};
use vir::{
    buffer::TextBuffer,
    editor::Editor,
    recovery::{self, Recovery},
};

fn text(value: &str) -> TextBuffer {
    TextBuffer::from_text(value).unwrap()
}

#[test]
fn snapshots_round_trip_private_exact_text_and_drop_retains_them() {
    let dir = tempfile::tempdir().unwrap();
    for content in ["", "\n", "中\t文", "two\nlines\n"] {
        let mut journal = Recovery::in_directory(dir.path().to_owned());
        journal
            .checkpoint(Some(std::path::Path::new("source")), &text(content), true)
            .unwrap();
        let path = journal.path().unwrap().to_owned();
        let snapshot = recovery::read(&path).unwrap();
        assert_eq!(snapshot.buffer.to_text(), content);
        assert!(snapshot.source.ends_with("source"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        drop(journal);
        assert!(path.exists());
        let restored = Editor::from_recovery(snapshot.buffer);
        assert!(restored.is_modified());
        assert!(restored.path().is_none());
    }
    assert_eq!(recovery::list(dir.path()).unwrap().len(), 4);
}

#[test]
fn checkpoints_replace_atomically_and_clean_only_the_owned_session() {
    let dir = tempfile::tempdir().unwrap();
    let mut first = Recovery::in_directory(dir.path().to_owned());
    let mut other = Recovery::in_directory(dir.path().to_owned());
    first.checkpoint(None, &text("first"), true).unwrap();
    other.checkpoint(None, &text("other"), true).unwrap();
    let path = first.path().unwrap().to_owned();
    first.checkpoint(None, &text("new\n"), true).unwrap();
    assert_eq!(first.path(), Some(path.as_path()));
    assert_eq!(recovery::read(&path).unwrap().buffer, text("new\n"));
    // A no-change checkpoint does not publish a new inode or timestamp.
    let modified = fs::metadata(&path).unwrap().modified().unwrap();
    first.checkpoint(None, &text("new\n"), true).unwrap();
    assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), modified);
    first.checkpoint(None, &text("new\n"), false).unwrap();
    assert!(!path.exists());
    assert_eq!(
        recovery::list(dir.path()).unwrap(),
        vec![other.path().unwrap().to_owned()]
    );
    first.finish().unwrap();
    other.finish().unwrap();
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
}

#[test]
fn corrupt_truncated_unknown_and_oversized_snapshots_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let mut journal = Recovery::in_directory(dir.path().to_owned());
    journal.checkpoint(None, &text("recover me"), true).unwrap();
    let original = fs::read(journal.path().unwrap()).unwrap();
    let path = dir.path().join("bad");
    for len in 0..original.len() {
        fs::write(&path, &original[..len]).unwrap();
        assert!(recovery::read(&path).is_err(), "length {len}");
    }
    for at in [0, 7, 8, 16, 24, original.len() - 1] {
        let mut corrupt = original.clone();
        corrupt[at] ^= 0xff;
        fs::write(&path, corrupt).unwrap();
        assert!(recovery::read(&path).is_err(), "offset {at}");
    }
    let file = fs::File::create(&path).unwrap();
    file.set_len(128 * 1024 * 1024 + 1).unwrap();
    assert!(recovery::read(&path).is_err());
    assert!(recovery::read(dir.path()).is_err());
    assert_eq!(
        recovery::read(journal.path().unwrap()).unwrap().buffer,
        text("recover me")
    );
}

#[test]
fn failed_checkpoint_retains_previous_snapshot_and_retries() {
    let dir = tempfile::tempdir().unwrap();
    let live = dir.path().join("live");
    let moved = dir.path().join("moved");
    fs::create_dir(&live).unwrap();
    let mut journal = Recovery::in_directory(live.clone());
    journal.checkpoint(None, &text("previous"), true).unwrap();
    let name = journal.path().unwrap().file_name().unwrap().to_owned();
    fs::rename(&live, &moved).unwrap();
    assert!(journal.checkpoint(None, &text("latest"), true).is_err());
    assert_eq!(
        recovery::read(&moved.join(&name)).unwrap().buffer,
        text("previous")
    );
    fs::rename(&moved, &live).unwrap();
    journal.checkpoint(None, &text("latest"), true).unwrap();
    assert_eq!(
        recovery::read(journal.path().unwrap()).unwrap().buffer,
        text("latest")
    );
}

#[cfg(unix)]
#[test]
fn snapshot_symlinks_are_not_followed() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("target");
    fs::write(&target, "untouched").unwrap();
    let mut journal = Recovery::in_directory(dir.path().to_owned());
    journal.checkpoint(None, &text("old"), true).unwrap();
    let path = journal.path().unwrap().to_owned();
    fs::remove_file(&path).unwrap();
    std::os::unix::fs::symlink(&target, &path).unwrap();
    assert!(journal.checkpoint(None, &text("new"), true).is_err());
    assert!(recovery::read(&path).is_err());
    assert_eq!(fs::read_to_string(target).unwrap(), "untouched");
}

#[test]
fn ex_io_failure_retains_completed_input_lines_without_committing_draft() {
    struct Broken {
        data: Cursor<Vec<u8>>,
    }
    impl io::Read for Broken {
        fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
            self.data.read(bytes)
        }
    }
    impl io::BufRead for Broken {
        fn fill_buf(&mut self) -> io::Result<&[u8]> {
            if self.data.position() == self.data.get_ref().len() as u64 {
                Err(io::Error::other("injected input failure"))
            } else {
                self.data.fill_buf()
            }
        }
        fn consume(&mut self, count: usize) {
            self.data.consume(count);
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let mut journal = Recovery::in_directory(dir.path().to_owned());
    let mut editor = Editor::from_buffer(text("old\n"));
    let before = editor.clone();
    let mut input = Broken {
        data: Cursor::new("c\n中\nsecond\n".as_bytes().to_vec()),
    };
    assert!(
        vir::ex::repl::run_with_recovery(
            &mut input,
            &mut Vec::new(),
            &mut editor,
            false,
            &mut journal
        )
        .is_err()
    );
    assert_eq!(editor, before);
    assert_eq!(
        recovery::read(journal.path().unwrap()).unwrap().buffer,
        text("中\nsecond\n")
    );
}

#[test]
fn recovery_write_failure_is_visible_without_blocking_edit_or_export() {
    let dir = tempfile::tempdir().unwrap();
    let mut journal = Recovery::in_directory(dir.path().join("missing"));
    let mut editor = Editor::default();
    let export = dir.path().join("export");
    let script = format!("a\nkept\n.\nw {}\nq\n", export.display());
    let mut output = Vec::new();
    vir::ex::repl::run_with_recovery(
        &mut Cursor::new(script),
        &mut output,
        &mut editor,
        false,
        &mut journal,
    )
    .unwrap();
    assert!(
        String::from_utf8(output)
            .unwrap()
            .contains("recovery unavailable")
    );
    assert_eq!(fs::read_to_string(export).unwrap(), "kept\n");
    assert!(!editor.is_modified());
}

#[test]
fn full_save_and_forced_reload_clean_snapshots_before_session_exit() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("original");
    fs::write(&path, "original\n").unwrap();
    for ending in ["w", "e!"] {
        let mut editor = Editor::default();
        editor.load(&path).unwrap();
        let mut journal = Recovery::in_directory(dir.path().to_owned());
        editor.insert_lines(1, ["unsaved"]).unwrap();
        journal.observe(&editor).unwrap();
        assert!(journal.path().is_some());
        let script = format!("{ending}\n");
        vir::ex::repl::run_with_recovery(
            &mut Cursor::new(script),
            &mut Vec::new(),
            &mut editor,
            false,
            &mut journal,
        )
        .unwrap();
        assert!(journal.path().is_none());
        assert!(recovery::list(dir.path()).unwrap().is_empty());
    }
}

#[test]
fn corrupt_cli_recovery_fails_before_editing_or_writing() {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(".vir-recovery-bad");
    fs::write(&path, b"VIRREC01broken").unwrap();
    let export = dir.path().join("export");
    let mut process = Command::new(env!("CARGO_BIN_EXE_vir"))
        .args(["--ex", "--recover"])
        .arg(&path)
        .env("VIR_RECOVERY_DIR", dir.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let _ = writeln!(process.stdin.take().unwrap(), "w {}\nq", export.display());
    assert!(!process.wait_with_output().unwrap().status.success());
    assert!(!export.exists());
    assert_eq!(fs::read(&path).unwrap(), b"VIRREC01broken");
    let listing = Command::new(env!("CARGO_BIN_EXE_vir"))
        .arg("--recover-list")
        .arg(dir.path())
        .output()
        .unwrap();
    assert!(listing.status.success());
    assert!(
        String::from_utf8(listing.stdout)
            .unwrap()
            .contains("invalid:")
    );
}

#[test]
fn missing_cached_snapshot_is_recreated_without_another_edit() {
    let dir = tempfile::tempdir().unwrap();
    let mut journal = Recovery::in_directory(dir.path().to_owned());
    let editor = Editor::from_recovery(text("still unsaved"));
    journal.observe(&editor).unwrap();
    fs::remove_file(journal.path().unwrap()).unwrap();
    journal.observe(&editor).unwrap();
    assert_eq!(
        recovery::read(journal.path().unwrap()).unwrap().buffer,
        *editor.buffer()
    );
    fs::remove_file(journal.path().unwrap()).unwrap();
    fs::create_dir(journal.path().unwrap()).unwrap();
    assert!(journal.observe(&editor).is_err());
}
