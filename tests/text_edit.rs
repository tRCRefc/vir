use vir::{
    buffer::{Position, TextBuffer},
    editor::Editor,
};
fn p(row: usize, column: usize) -> Position {
    Position { row, column }
}

#[test]
fn scalar_ranges_split_join_and_preserve_final_newline() {
    for suffix in ["", "\n"] {
        let mut buffer = TextBuffer::from_text(&format!("a中🙂z\nlast{suffix}")).unwrap();
        let end = buffer.replace_text(p(0, 1), p(0, 3), "文\n字").unwrap();
        assert_eq!(end, p(1, 1));
        assert_eq!(buffer.to_text(), format!("a文\n字z\nlast{suffix}"));
        buffer.replace_text(p(0, 2), p(1, 1), "").unwrap();
        assert_eq!(buffer.to_text(), format!("a文z\nlast{suffix}"));
    }
}

#[test]
fn invalid_ranges_and_input_are_atomic() {
    let mut buffer = TextBuffer::from_text("中x\n").unwrap();
    let before = buffer.clone();
    for (start, end, text) in [
        (p(0, 3), p(0, 3), "x"),
        (p(1, 0), p(1, 0), ""),
        (p(0, 2), p(0, 1), ""),
        (p(0, 0), p(0, 1), "a\r\nb"),
    ] {
        assert!(buffer.replace_text(start, end, text).is_err());
        assert_eq!(buffer, before);
    }
    let mut editor = Editor::from_buffer(before);
    let saved = editor.clone();
    assert!(editor.begin_text_edit(p(8, 0)).is_err());
    assert_eq!(editor, saved);
    let mut edit = editor.begin_text_edit(p(0, 1)).unwrap();
    assert!(edit.insert("\r").is_err());
    assert_eq!(edit.cursor(), p(0, 1));
    edit.commit().unwrap();
    assert_eq!(editor, saved);
}

#[test]
fn session_is_one_reversible_edit_and_save_preserves_undo() {
    let mut editor = Editor::from_buffer(TextBuffer::from_text("one\ntwo").unwrap());
    editor.set_current(0).unwrap();
    let mut edit = editor.begin_text_edit(p(0, 3)).unwrap();
    edit.insert("中").unwrap();
    edit.insert("\n🙂").unwrap();
    edit.backspace().unwrap();
    edit.backspace().unwrap();
    edit.delete().unwrap(); // join the next line
    edit.insert("!").unwrap();
    assert_eq!(edit.commit().unwrap(), p(0, 5));
    assert_eq!(editor.buffer().to_text(), "one中!two");
    assert_eq!(editor.current(), Some(0));
    assert!(editor.is_modified());
    let failed = editor.clone();
    assert!(editor.save().is_err());
    assert_eq!(editor, failed);
    let directory = tempfile::tempdir().unwrap();
    editor
        .write_to(Some(&directory.path().join("saved")))
        .unwrap();
    assert!(!editor.is_modified());
    editor.undo().unwrap();
    assert_eq!(editor.buffer().to_text(), "one\ntwo");
    assert!(editor.is_modified());
    editor.undo().unwrap();
    assert_eq!(editor.buffer().to_text(), "one中!two");
}

#[test]
fn cancelled_and_no_change_sessions_preserve_existing_history() {
    let mut editor = Editor::default();
    editor.insert_lines(0, ["old"]).unwrap();
    let before = editor.clone();
    {
        let mut edit = editor.begin_text_edit(p(0, 0)).unwrap();
        edit.insert("discard\n").unwrap();
    }
    assert_eq!(editor, before);
    let mut edit = editor.begin_text_edit(p(0, 0)).unwrap();
    edit.backspace().unwrap();
    edit.insert("中").unwrap();
    edit.backspace().unwrap();
    edit.commit().unwrap();
    assert_eq!(editor, before);
    editor.undo().unwrap();
    assert!(editor.buffer().is_empty());
}

#[test]
fn empty_buffer_and_end_boundaries() {
    let mut editor = Editor::default();
    let mut edit = editor.begin_text_edit(p(0, 0)).unwrap();
    edit.backspace().unwrap();
    edit.delete().unwrap();
    edit.insert("").unwrap();
    edit.commit().unwrap();
    assert_eq!(editor, Editor::default());
    let mut edit = editor.begin_text_edit(p(0, 0)).unwrap();
    edit.insert("中\n").unwrap();
    edit.delete().unwrap();
    assert_eq!(edit.cursor(), p(1, 0));
    assert_eq!(edit.buffer().to_text(), "中\n\n");
    edit.commit().unwrap();
    editor.undo().unwrap();
    assert!(editor.buffer().is_empty());
    editor.undo().unwrap();
    let mut edit = editor.begin_text_edit(p(0, 0)).unwrap();
    edit.delete().unwrap();
    edit.delete().unwrap();
    assert_eq!(edit.buffer().to_text(), "\n");
    edit.commit().unwrap();
    assert_eq!(editor.current(), Some(0));
}
