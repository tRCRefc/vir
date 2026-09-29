use vir::{
    buffer::TextBuffer,
    editor::{Editor, Register},
    visual::{Motion, VisualState},
};

#[test]
fn autoindent_does_not_rewrite_multiline_register_put_or_dot() {
    for before in [false, true] {
        for count in [1, 2] {
            let mut enabled =
                Editor::from_buffer(TextBuffer::from_text("  base\n\tother\n").unwrap());
            enabled
                .set_named_register('a', Register::Characters("中\n x\n\ty".into()), false)
                .unwrap();
            let mut disabled = enabled.clone();
            enabled.set_autoindent(true);
            let mut actual = VisualState::default();
            actual.column = 2;
            let mut reference = actual.clone();
            actual
                .put_selected(&mut enabled, before, count, Some('a'))
                .unwrap();
            reference
                .put_selected(&mut disabled, before, count, Some('a'))
                .unwrap();
            assert_eq!(enabled.buffer(), disabled.buffer());
            assert_eq!(
                (actual.row, actual.column),
                (reference.row, reference.column)
            );
            actual.undo(&mut enabled).unwrap();
            reference.undo(&mut disabled).unwrap();
            assert_eq!(enabled.buffer().to_text(), "  base\n\tother\n");
            actual.row = 1;
            actual.column = 1;
            reference.row = 1;
            reference.column = 1;
            actual.repeat_change(&mut enabled, None).unwrap();
            reference.repeat_change(&mut disabled, None).unwrap();
            assert_eq!(enabled.buffer(), disabled.buffer());
            assert_eq!(enabled.named_register('a'), disabled.named_register('a'));
            assert!(enabled.autoindent());
        }
    }
}

#[test]
fn yank_preserves_text_modified_cursor_and_existing_undo() {
    let mut editor = Editor::default();
    editor.insert_lines(0, ["a中🙂z", "next"]).unwrap();
    let mut state = VisualState::default();
    state.column = 1;
    let dot = editor.current();
    state
        .yank(&mut editor, Some(Motion::Right), Some(2))
        .unwrap();
    assert_eq!(
        editor.unnamed_register(),
        Some(&Register::Characters("中🙂".into()))
    );
    assert_eq!(editor.current(), dot);
    assert_eq!(state.column, 1);
    assert_eq!(editor.buffer().to_text(), "a中🙂z\nnext\n");
    editor.undo().unwrap();
    assert!(editor.buffer().is_empty());
    assert_eq!(
        editor.unnamed_register(),
        Some(&Register::Characters("中🙂".into()))
    );
}

#[test]
fn character_put_before_after_counts_and_undo() {
    let mut editor = Editor::from_buffer(TextBuffer::from_text("a中z").unwrap());
    let mut state = VisualState::default();
    state.column = 1;
    state.yank(&mut editor, Some(Motion::Right), None).unwrap();
    assert!(!editor.is_modified());
    state.put(&mut editor, false, 2).unwrap();
    assert_eq!(editor.buffer().to_text(), "a中中中z");
    assert_eq!(state.column, 3);
    editor.undo().unwrap();
    assert_eq!(editor.buffer().to_text(), "a中z");
    state.column = 1;
    state.put(&mut editor, true, 1).unwrap();
    assert_eq!(editor.buffer().to_text(), "a中中z");
}

#[test]
fn line_put_and_delete_all_retain_register_through_undo() {
    let mut editor = Editor::from_buffer(TextBuffer::from_text("one\ntwo\n").unwrap());
    let mut state = VisualState::default();
    state.yank(&mut editor, None, Some(2)).unwrap();
    state.put(&mut editor, true, 2).unwrap();
    assert_eq!(editor.buffer().to_text(), "one\ntwo\none\ntwo\none\ntwo\n");
    assert_eq!((state.row, state.column), (0, 0));
    editor.undo().unwrap();
    state.delete_lines(&mut editor, 2).unwrap();
    assert!(editor.buffer().is_empty());
    state.put(&mut editor, false, 1).unwrap();
    assert_eq!(editor.buffer().to_text(), "one\ntwo\n");
    editor.undo().unwrap();
    assert!(editor.buffer().is_empty());
    editor.undo().unwrap();
    assert_eq!(editor.buffer().to_text(), "one\ntwo\n");
}

#[test]
fn change_register_publishes_on_commit_and_cancel_preserves_old_value() {
    let mut editor = Editor::from_buffer(TextBuffer::from_text("abc\nnext\n").unwrap());
    let mut state = VisualState::default();
    state.yank(&mut editor, None, None).unwrap();
    let before = editor.clone();
    drop(
        state
            .begin_change(&mut editor, Some(Motion::Right), None)
            .unwrap(),
    );
    assert_eq!(editor, before);
    let mut edit = state.begin_change(&mut editor, None, Some(2)).unwrap();
    edit.insert("new").unwrap();
    edit.commit().unwrap();
    assert_eq!(
        editor.unnamed_register(),
        Some(&Register::Lines(vec!["abc".into(), "next".into()]))
    );
    state.put(&mut editor, false, 1).unwrap();
    assert_eq!(editor.buffer().to_text(), "new\nabc\nnext\n");
}

#[test]
fn character_deletion_and_c_preserve_removed_text() {
    let mut editor = Editor::from_buffer(TextBuffer::from_text("a中z").unwrap());
    let mut state = VisualState::default();
    state.column = 1;
    state.delete_character(&mut editor).unwrap();
    assert_eq!(
        editor.unnamed_register(),
        Some(&Register::Characters("中".into()))
    );
    state.put(&mut editor, true, 1).unwrap();
    assert_eq!(editor.buffer().to_text(), "a中z");
    state.column = 1;
    state
        .begin_insert(&mut editor, 'C')
        .unwrap()
        .commit()
        .unwrap();
    assert_eq!(
        editor.unnamed_register(),
        Some(&Register::Characters("中z".into()))
    );
    state.column = 0;
    state.put(&mut editor, false, 1).unwrap();
    assert_eq!(editor.buffer().to_text(), "a中z");
}

#[test]
fn multiline_character_register_and_failed_operations_are_atomic() {
    let mut editor = Editor::from_buffer(TextBuffer::from_text("abc\ndef\nlast").unwrap());
    let mut state = VisualState::default();
    state.column = 1;
    state.yank(&mut editor, Some(Motion::End), Some(2)).unwrap();
    assert_eq!(
        editor.unnamed_register(),
        Some(&Register::Characters("bc\ndef".into()))
    );
    let before = editor.clone();
    assert!(state.put(&mut editor, false, usize::MAX).is_err());
    assert!(state.yank(&mut editor, Some(Motion::Up), None).is_err());
    assert_eq!(editor, before);
    state.put(&mut editor, true, 1).unwrap();
    assert_eq!(editor.buffer().to_text(), "abc\ndefbc\ndef\nlast");
    editor.undo().unwrap();
    assert_eq!(editor.buffer(), before.buffer());
    let mut empty = Editor::default();
    assert!(VisualState::default().put(&mut empty, true, 1).is_err());
    assert_eq!(empty, Editor::default());
}

#[test]
fn register_survives_file_switch_and_can_be_pasted() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("target");
    std::fs::write(&path, "other\n").unwrap();
    let mut editor = Editor::from_buffer(TextBuffer::from_text("copy\n").unwrap());
    let mut state = VisualState::default();
    state.yank(&mut editor, None, None).unwrap();
    editor.load_with_force(&path, true).unwrap();
    state.put(&mut editor, false, 1).unwrap();
    assert_eq!(editor.buffer().to_text(), "other\ncopy\n");
}

#[test]
fn line_deletes_shift_numbered_registers() {
    let mut editor = Editor::from_buffer(TextBuffer::from_text("one\ntwo\nthree\n").unwrap());
    let mut state = VisualState::default();
    state.delete_lines(&mut editor, 1).unwrap();
    assert_eq!(
        editor.numbered_register(1),
        Some(&Register::Lines(vec!["one".into()]))
    );
    state.delete_lines(&mut editor, 1).unwrap();
    assert_eq!(
        editor.numbered_register(1),
        Some(&Register::Lines(vec!["two".into()]))
    );
    assert_eq!(
        editor.numbered_register(2),
        Some(&Register::Lines(vec!["one".into()]))
    );
}

#[test]
fn numbered_register_can_be_pasted_without_changing_unnamed() {
    let mut editor = Editor::from_buffer(TextBuffer::from_text("target\nsource\n").unwrap());
    let mut state = VisualState::default();
    state.delete_lines(&mut editor, 1).unwrap();
    editor.set_current(0).unwrap();
    state.row = 0;
    state.put_numbered(&mut editor, false, 1, 1).unwrap();
    assert_eq!(editor.buffer().to_text(), "source\ntarget\n");
}

#[test]
fn named_registers_select_yank_append_delete_and_put() {
    let mut editor = Editor::from_buffer(TextBuffer::from_text("one\ntwo\n").unwrap());
    let mut state = VisualState::default();
    state
        .yank_register(&mut editor, None, Some(1), Some('a'))
        .unwrap();
    assert_eq!(
        editor.named_register('a'),
        Some(&Register::Lines(vec!["one".into()]))
    );
    state.row = 1;
    state
        .yank_register(&mut editor, None, Some(1), Some('A'))
        .unwrap();
    assert_eq!(
        editor.named_register('a'),
        Some(&Register::Lines(vec!["one".into(), "two".into()]))
    );
    state
        .put_selected(&mut editor, false, 1, Some('a'))
        .unwrap();
    assert_eq!(editor.buffer().to_text(), "one\ntwo\none\ntwo\n");
}

#[test]
fn dot_replay_keeps_named_delete_selection() {
    let mut editor = Editor::from_buffer(TextBuffer::from_text("one\ntwo\nthree\n").unwrap());
    let mut state = VisualState::default();
    state
        .delete_lines_register(&mut editor, 1, Some('a'))
        .unwrap();
    state.row = 0;
    state.repeat_change(&mut editor, None).unwrap();
    assert_eq!(
        editor.named_register('a'),
        Some(&Register::Lines(vec!["two".into()]))
    );
    assert_eq!(editor.buffer().to_text(), "three\n");
}

#[test]
fn named_registers_survive_undo_and_file_switch() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("other");
    std::fs::write(&path, "disk\n").unwrap();
    let mut editor = Editor::from_buffer(TextBuffer::from_text("one\ntwo\n").unwrap());
    let mut state = VisualState::default();
    state
        .yank_register(&mut editor, None, Some(1), Some('a'))
        .unwrap();
    state.delete_lines(&mut editor, 1).unwrap();
    editor.undo().unwrap();
    assert_eq!(
        editor.named_register('a'),
        Some(&Register::Lines(vec!["one".into()]))
    );
    editor.load_with_force(&path, true).unwrap();
    assert_eq!(
        editor.named_register('a'),
        Some(&Register::Lines(vec!["one".into()]))
    );
    assert_eq!(
        editor.numbered_register(1),
        Some(&Register::Lines(vec!["one".into()]))
    );
}
