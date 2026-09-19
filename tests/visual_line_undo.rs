use vir::{
    buffer::TextBuffer,
    editor::{Editor, InputAction},
    visual::{Motion, VisualState},
};
fn editor(text: &str) -> Editor {
    Editor::from_buffer(TextBuffer::from_text(text).unwrap())
}

#[test]
fn restores_multiple_same_line_changes_with_one_undo_and_keeps_dot_register() {
    for lf in ["", "\n"] {
        let mut editor = editor(&format!("a中🙂def{lf}"));
        let mut state = VisualState::default();
        state.checkpoint_line(&editor);
        state.yank(&mut editor, Some(Motion::End), None).unwrap();
        let register = editor.unnamed_register().cloned();
        state.column = 1;
        state.replace_counted(&mut editor, 'X', 2).unwrap();
        state.checkpoint_line(&editor);
        state.column = 4;
        state.replace_character(&mut editor, 'Y').unwrap();
        let edited = editor.buffer().clone();
        state.restore_line(&mut editor).unwrap();
        assert_eq!(editor.buffer().to_text(), format!("a中🙂def{lf}"));
        assert_eq!(state.column, 0);
        assert_eq!(editor.unnamed_register(), register.as_ref());
        let before = editor.clone();
        assert!(state.restore_line(&mut editor).is_err());
        assert_eq!(editor, before);
        editor.undo().unwrap();
        assert_eq!(editor.buffer(), &edited);
        state.restore_line(&mut editor).unwrap();
        state.repeat_change(&mut editor, None).unwrap();
        assert_eq!(editor.buffer().to_text(), format!("Y中🙂def{lf}"));
    }
}

#[test]
fn leaving_and_revisiting_a_line_drops_its_old_snapshot() {
    let mut editor = editor("abc\ndef");
    let mut state = VisualState::default();
    state.checkpoint_line(&editor);
    state.replace_character(&mut editor, 'X').unwrap();
    state.checkpoint_line(&editor);
    state.move_cursor(Motion::Down, editor.buffer(), 8);
    state.checkpoint_line(&editor);
    state.move_cursor(Motion::Up, editor.buffer(), 8);
    state.checkpoint_line(&editor);
    assert!(state.restore_line(&mut editor).is_err());
    assert_eq!(editor.buffer().to_text(), "Xbc\ndef");
    state.replace_character(&mut editor, 'Y').unwrap();
    state.restore_line(&mut editor).unwrap();
    assert_eq!(editor.buffer().to_text(), "Xbc\ndef");
}

#[test]
fn input_that_crosses_lines_invalidates_even_if_the_split_is_reversed() {
    let mut editor = editor("abc");
    let mut state = VisualState::default();
    state.checkpoint_line(&editor);
    state.replace_character(&mut editor, 'X').unwrap();
    state.checkpoint_line(&editor);
    let mut input = state.begin_counted_insert(&mut editor, 'i', 1).unwrap();
    input
        .apply_input(&InputAction::Insert("\n".into()))
        .unwrap();
    input.apply_input(&InputAction::Backspace).unwrap();
    input.commit(&mut state).unwrap();
    assert!(state.restore_line(&mut editor).is_err());
    assert_eq!(editor.buffer().to_text(), "Xbc");
}

#[test]
fn structural_edits_and_successful_ex_edits_reset_but_failed_ex_keeps_snapshot() {
    let mut editor = editor("abc\ndef");
    let mut state = VisualState::default();
    state.checkpoint_line(&editor);
    state.replace_character(&mut editor, 'X').unwrap();
    assert!(state.execute_ex("unknown", &mut editor).is_err());
    state.restore_line(&mut editor).unwrap();
    state.replace_character(&mut editor, 'Y').unwrap();
    state.join_lines(&mut editor, 2).unwrap();
    assert!(state.restore_line(&mut editor).is_err());
    state.column = 0;
    state.replace_character(&mut editor, 'Z').unwrap();
    state.execute_ex("s/Z/Q/", &mut editor).unwrap();
    assert!(state.restore_line(&mut editor).is_err());
    assert_eq!(editor.buffer().to_text(), "Qbc def\n");
}

#[test]
fn saves_preserve_snapshot_but_even_identical_reload_invalidates_it() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("text");
    std::fs::write(&path, "abc").unwrap();
    let mut editor = Editor::default();
    let mut state = VisualState::default();
    state
        .execute_ex(&format!("e {}", path.display()), &mut editor)
        .unwrap();
    state.checkpoint_line(&editor);
    state.replace_character(&mut editor, 'X').unwrap();
    state.execute_ex("w", &mut editor).unwrap();
    state.restore_line(&mut editor).unwrap();
    assert!(editor.is_modified());
    assert_eq!(editor.buffer().to_text(), "abc");
    state.execute_ex("e!", &mut editor).unwrap();
    assert!(state.restore_line(&mut editor).is_err());
    assert_eq!(editor.buffer().to_text(), "Xbc");
}

#[test]
fn empty_and_unchanged_lines_do_not_create_undo_entries() {
    for text in ["", "\n", "abc"] {
        let mut editor = editor(text);
        let mut state = VisualState::default();
        let before = editor.clone();
        state.checkpoint_line(&editor);
        assert!(state.restore_line(&mut editor).is_err());
        assert_eq!(editor, before);
    }
}
