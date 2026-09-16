use std::fs;
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use vir::editor::Editor;
use vir::ex::command::execute;
use vir::ex::parser::parse;

struct TempDir(PathBuf);
impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        loop {
            let path = std::env::temp_dir().join(format!(
                "vir-loading-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&path) {
                Ok(()) => return Self(path),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) => panic!("cannot create test directory: {e}"),
            }
        }
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn loads_supported_text_exactly_and_initializes_ex_dot() {
    let dir = TempDir::new();
    let path = dir.0.join("text");
    let mut editor = Editor::default();
    for (text, lines) in [
        ("", 0),
        ("\n", 1),
        ("hello", 1),
        ("hello\n", 1),
        ("一\n二", 2),
        ("one\ntwo\nthree\n", 3),
    ] {
        fs::write(&path, text).unwrap();
        editor.load(&path).unwrap();
        assert_eq!(editor.buffer().to_text(), text);
        assert_eq!(editor.buffer().line_count(), lines);
        // ex_io.c rop2 -> append leaves dot at dol in command mode.
        assert_eq!(editor.current(), lines.checked_sub(1));
        assert_eq!(editor.path(), Some(path.as_path()));
        assert!(!editor.is_modified());
    }
    assert_eq!(
        execute(parse("p").unwrap(), &mut editor).unwrap().output,
        "three\n"
    );
    execute(parse("1p").unwrap(), &mut editor).unwrap();
    assert_eq!(editor.current(), Some(0));
    assert_eq!(editor.path(), Some(path.as_path()));
    assert!(!editor.is_modified());
}

#[test]
fn missing_file_becomes_clean_empty_buffer_without_creating_file() {
    let dir = TempDir::new();
    let existing = dir.0.join("existing");
    fs::write(&existing, "old\n").unwrap();
    let mut editor = Editor::default();
    editor.load(&existing).unwrap();
    let missing = dir.0.join("new file");
    editor.load(&missing).unwrap();
    assert!(editor.buffer().is_empty());
    assert_eq!(editor.current(), None);
    assert_eq!(editor.path(), Some(missing.as_path()));
    assert!(!editor.is_modified());
    assert!(!missing.exists());
    assert_eq!(fs::read_to_string(existing).unwrap(), "old\n");
}

#[test]
fn failed_loads_preserve_all_editor_state() {
    let dir = TempDir::new();
    let path = dir.0.join("original");
    fs::write(&path, "keep\nthis\n").unwrap();
    let mut editor = Editor::default();
    editor.load(&path).unwrap();
    editor.set_current(0).unwrap();
    let before = editor.clone();
    let bad = dir.0.join("bad");
    for bytes in [b"\xff".as_slice(), b"CRLF\r\n", b"embedded\rcarriage"] {
        fs::write(&bad, bytes).unwrap();
        assert_eq!(
            editor.load(&bad).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        assert_eq!(editor, before);
    }
    // Reading a directory provides a real I/O failure even when tests run as root.
    assert!(editor.load(&dir.0).is_err());
    assert_eq!(editor, before);
    assert_eq!(fs::read_to_string(path).unwrap(), "keep\nthis\n");
}

#[test]
fn invalid_cursor_moves_preserve_state_including_empty_buffer() {
    let mut editor = Editor::default();
    let before = editor.clone();
    assert!(editor.set_current(0).is_err());
    assert_eq!(editor, before);
    let dir = TempDir::new();
    let path = dir.0.join("one");
    fs::write(&path, "one").unwrap();
    editor.load(&path).unwrap();
    let before = editor.clone();
    for index in [1, usize::MAX] {
        assert!(editor.set_current(index).is_err());
        assert_eq!(editor, before);
    }
}

#[test]
fn save_preserves_reversible_undo_and_undo_marks_modified() {
    let dir = TempDir::new();
    let path = dir.0.join("saved");
    fs::write(&path, "one\n").unwrap();
    let mut editor = Editor::default();
    editor.load(&path).unwrap();
    editor.insert_lines(1, ["two"]).unwrap();
    editor.save().unwrap();
    assert!(!editor.is_modified());
    editor.undo().unwrap();
    assert_eq!(editor.buffer().to_text(), "one\n");
    assert!(editor.is_modified());
    assert!(execute(parse("q").unwrap(), &mut editor).is_err());
    editor.undo().unwrap();
    assert_eq!(editor.buffer().to_text(), "one\ntwo\n");
    assert!(editor.is_modified());
    assert_eq!(fs::read_to_string(path).unwrap(), "one\ntwo\n");
}

#[test]
fn addressed_writes_do_not_touch_disk_or_editor_history() {
    let dir = TempDir::new();
    let path = dir.0.join("saved");
    fs::write(&path, "original\n").unwrap();
    let mut editor = Editor::default();
    editor.load(&path).unwrap();
    editor.insert_lines(1, ["unsaved"]).unwrap();
    let before = editor.clone();
    for command in ["1w", "%w", "1,2w", "999w", "2,1w", "0w"] {
        assert_eq!(
            execute(parse(command).unwrap(), &mut editor).unwrap_err(),
            "write ranges are not supported"
        );
        assert_eq!(editor, before);
        assert_eq!(fs::read_to_string(&path).unwrap(), "original\n");
    }
    execute(parse("w").unwrap(), &mut editor).unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), "original\nunsaved\n");
    assert!(!editor.is_modified());
}

#[test]
fn unaddressed_write_still_saves_an_empty_file() {
    let dir = TempDir::new();
    let path = dir.0.join("empty");
    let mut editor = Editor::default();
    editor.load(&path).unwrap();
    execute(parse("w").unwrap(), &mut editor).unwrap();
    assert_eq!(fs::read(&path).unwrap(), b"");
}

#[test]
fn failed_save_keeps_undo_and_successful_load_resets_it() {
    let dir = TempDir::new();
    let parent = dir.0.join("missing-parent");
    let target = parent.join("new");
    let mut editor = Editor::default();
    editor.load(&target).unwrap();
    editor.insert_lines(0, ["one", "two"]).unwrap();
    let before = editor.clone();
    assert!(editor.save().is_err());
    assert_eq!(editor, before);
    assert!(!target.exists());
    editor.undo().unwrap();
    assert!(editor.buffer().is_empty());
    editor.undo().unwrap();
    fs::create_dir(&parent).unwrap();
    editor.save().unwrap();
    editor.load(&target).unwrap();
    assert!(editor.undo().is_err());
    assert!(!editor.is_modified());
    assert_eq!(editor.buffer().to_text(), "one\ntwo\n");
}

#[cfg(unix)]
#[test]
fn safe_save_preserves_permissions_and_rejects_read_only_files() {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("script");
    fs::write(&path, "original\n").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o750)).unwrap();
    let original = fs::metadata(&path).unwrap();
    let mut editor = Editor::default();
    editor.load(&path).unwrap();
    editor.insert_lines(1, ["new"]).unwrap();
    editor.save().unwrap();
    let saved = fs::metadata(&path).unwrap();
    assert_eq!(saved.mode() & 0o7777, 0o750);
    assert_eq!((saved.uid(), saved.gid()), (original.uid(), original.gid()));
    assert_eq!(fs::read_to_string(&path).unwrap(), "original\nnew\n");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o440)).unwrap();
    editor.insert_lines(2, ["unsaved"]).unwrap();
    let before = editor.clone();
    assert_eq!(
        editor.save().unwrap_err().kind(),
        io::ErrorKind::PermissionDenied
    );
    assert_eq!(editor, before);
    assert_eq!(fs::read_to_string(&path).unwrap(), "original\nnew\n");
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    editor.undo().unwrap();
    assert_eq!(editor.buffer().to_text(), "original\nnew\n");
}

#[cfg(unix)]
#[test]
fn safe_save_rejects_links_without_changing_their_targets() {
    use std::os::unix::fs::symlink;
    for hard in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target");
        let link = dir.path().join("link");
        fs::write(&target, "original\n").unwrap();
        if hard {
            fs::hard_link(&target, &link).unwrap();
        } else {
            symlink("target", &link).unwrap();
        }
        let mut editor = Editor::default();
        editor.load(&link).unwrap();
        editor.insert_lines(1, ["unsaved"]).unwrap();
        let before = editor.clone();
        assert!(editor.save().is_err());
        assert_eq!(editor, before);
        assert_eq!(fs::read_to_string(&link).unwrap(), "original\n");
        assert_eq!(fs::read_to_string(&target).unwrap(), "original\n");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);
        if !hard {
            assert_eq!(fs::read_link(&link).unwrap(), PathBuf::from("target"));
        }
    }
}

#[cfg(unix)]
#[test]
fn safe_save_does_not_replace_a_dangling_symlink() {
    use std::os::unix::fs::symlink;
    let dir = tempfile::tempdir().unwrap();
    let link = dir.path().join("link");
    symlink("missing", &link).unwrap();
    let mut editor = Editor::default();
    editor.load(&link).unwrap();
    editor.insert_lines(0, ["unsaved"]).unwrap();
    let before = editor.clone();
    assert!(editor.save().is_err());
    assert_eq!(editor, before);
    assert_eq!(fs::read_link(&link).unwrap(), PathBuf::from("missing"));
    assert!(!dir.path().join("missing").exists());
}

#[test]
fn safe_save_rejects_a_directory_that_replaced_the_target() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("target");
    let mut editor = Editor::default();
    editor.load(&target).unwrap();
    editor.insert_lines(0, ["unsaved"]).unwrap();
    fs::create_dir(&target).unwrap();
    let before = editor.clone();
    assert!(editor.save().is_err());
    assert_eq!(editor, before);
    assert!(target.is_dir());
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
}
