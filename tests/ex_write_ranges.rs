use std::fs;
use vir::{
    buffer::TextBuffer,
    editor::Editor,
    ex::{address::LineRange, command::execute, parser::parse},
};

fn run(editor: &mut Editor, command: &str) -> Result<bool, String> {
    execute(parse(command).unwrap(), editor).map(|result| result.quit)
}

#[test]
fn complete_ranges_save_without_moving_dot_or_losing_undo() {
    for command in ["%w", "1,$w", "1,2w"] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("file");
        fs::write(&path, "old\n").unwrap();
        let mut editor = Editor::default();
        editor.load(&path).unwrap();
        editor.insert_lines(1, ["new"]).unwrap();
        editor.set_current(0).unwrap();
        assert!(!run(&mut editor, command).unwrap());
        assert_eq!(editor.current(), Some(0));
        assert!(!editor.is_modified());
        assert_eq!(fs::read_to_string(&path).unwrap(), "old\nnew\n");
        editor.undo().unwrap();
        assert_eq!(editor.buffer().to_text(), "old\n");
    }
}

#[test]
fn partial_exports_preserve_state_and_line_endings() {
    for (range, expected) in [
        ("1", "one\n"),
        ("2", "二\n"),
        ("3", "last"),
        ("2,3", "二\nlast"),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("current");
        let target = dir.path().join("export");
        fs::write(&path, "one\n二\nlast").unwrap();
        let mut editor = Editor::default();
        editor.load(&path).unwrap();
        let before = editor.clone();
        assert!(!run(&mut editor, &format!("{range}w {}", target.display())).unwrap());
        assert_eq!(editor, before);
        assert_eq!(fs::read_to_string(target).unwrap(), expected);
        assert_eq!(fs::read_to_string(path).unwrap(), "one\n二\nlast");
    }
}

#[test]
fn partial_export_from_modified_unnamed_buffer_adopts_path_but_stays_dirty() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("export");
    let mut editor = Editor::default();
    editor.insert_lines(0, ["one", "two"]).unwrap();
    run(&mut editor, &format!("1w {}", target.display())).unwrap();
    assert_eq!(editor.path(), Some(target.as_path()));
    assert!(editor.is_modified());
    assert!(run(&mut editor, "q").is_err());
    assert_eq!(fs::read_to_string(&target).unwrap(), "one\n");
    run(&mut editor, "w").unwrap();
    assert_eq!(fs::read_to_string(target).unwrap(), "one\ntwo\n");
    editor.undo().unwrap();
    assert!(editor.buffer().is_empty());
}

#[test]
fn partial_wq_exports_but_does_not_quit_with_unsaved_changes() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("export");
    let mut editor = Editor::default();
    editor.insert_lines(0, ["one", "two"]).unwrap();
    assert_eq!(
        run(&mut editor, &format!("1wq {}", target.display())).unwrap_err(),
        "unsaved changes"
    );
    assert_eq!(fs::read_to_string(&target).unwrap(), "one\n");
    assert!(editor.is_modified());
    assert!(run(&mut editor, "%wq").unwrap());
    assert_eq!(fs::read_to_string(target).unwrap(), "one\ntwo\n");
}

#[test]
fn invalid_ranges_and_failed_exports_preserve_all_state() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("existing");
    fs::write(&target, "protected\n").unwrap();
    let mut editor = Editor::default();
    editor.insert_lines(0, ["one", "two"]).unwrap();
    let before = editor.clone();
    for range in ["0", "3", "2,1", "$+1", "1"] {
        assert!(run(&mut editor, &format!("{range}w {}", target.display())).is_err());
        assert_eq!(editor, before);
        assert_eq!(fs::read_to_string(&target).unwrap(), "protected\n");
    }
    assert!(
        editor
            .write_range_to(
                Some(&target),
                Some(LineRange {
                    start: 1,
                    end: usize::MAX
                })
            )
            .is_err()
    );
    assert_eq!(editor, before);
}

#[test]
fn empty_buffer_allows_only_unaddressed_write_and_single_line_is_full() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("new");
    let mut editor = Editor::default();
    for range in ["%", "1", "0", "$", "."] {
        assert!(run(&mut editor, &format!("{range}w {}", target.display())).is_err());
        assert_eq!(editor, Editor::default());
        assert!(!target.exists());
    }
    run(&mut editor, &format!("w {}", target.display())).unwrap();
    assert_eq!(fs::read(&target).unwrap(), b"");
    let mut editor = Editor::from_buffer(TextBuffer::from_text("single").unwrap());
    let other = dir.path().join("single");
    assert!(run(&mut editor, &format!("1wq {}", other.display())).unwrap());
    assert_eq!(fs::read(other).unwrap(), b"single");
}
