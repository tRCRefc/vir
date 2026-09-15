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
