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
    state.restore_line(&mut editor).unwrap();
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
    state.restore_line(&mut editor).unwrap();
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
    state.restore_line(&mut editor).unwrap();
    state.column = 0;
    state.replace_character(&mut editor, 'Z').unwrap();
    state.execute_ex("s/Z/Q/", &mut editor).unwrap();
    state.restore_line(&mut editor).unwrap();
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
    state.restore_line(&mut editor).unwrap();
    assert_eq!(editor.buffer().to_text(), "Xbc");
}

#[test]
fn empty_buffer_rejects_u_but_unchanged_existing_lines_publish_undo() {
    for text in ["", "\n", "abc"] {
        let mut editor = editor(text);
        let mut state = VisualState::default();
        let before = editor.clone();
        state.checkpoint_line(&editor);
        if text.is_empty() {
            assert!(state.restore_line(&mut editor).is_err());
            assert_eq!(editor, before);
        } else {
            state.restore_line(&mut editor).unwrap();
            assert_eq!(editor.buffer(), before.buffer());
            assert!(editor.is_modified());
            state.undo(&mut editor).unwrap();
            assert_eq!(editor.buffer(), before.buffer());
        }
    }
}

#[test]
fn unchanged_u_replaces_undo_and_rearms_after_equal_undo_or_edit() {
    for lf in ["", "\n"] {
        let directory = tempfile::tempdir().unwrap();
        let mut editor = editor(&format!("abc{lf}"));
        let mut state = VisualState::default();
        state.replace_character(&mut editor, 'X').unwrap();
        state.checkpoint_line(&editor);
        state.yank(&mut editor, Some(Motion::End), None).unwrap();
        let register = editor.unnamed_register().cloned();
        editor
            .write_to(Some(&directory.path().join("text")))
            .unwrap();
        state.column = 2;
        state.restore_line(&mut editor).unwrap();
        assert_eq!(editor.buffer().to_text(), format!("Xbc{lf}"));
        assert!(editor.is_modified());
        assert_eq!(state.column, 0);
        let consumed = editor.clone();
        state.move_cursor(Motion::Right, editor.buffer(), 8);
        assert!(state.restore_line(&mut editor).is_err());
        assert_eq!(editor, consumed);
        state.undo(&mut editor).unwrap();
        assert_eq!(editor.buffer().to_text(), format!("Xbc{lf}"));
        state.restore_line(&mut editor).unwrap();
        assert!(state.restore_line(&mut editor).is_err());
        state.column = 0;
        state.replace_character(&mut editor, 'X').unwrap();
        state.restore_line(&mut editor).unwrap();
        // Neither U nor u replaces the dot recipe or register.
        state.column = 1;
        state.repeat_change(&mut editor, None).unwrap();
        assert_eq!(editor.buffer().to_text(), format!("XXc{lf}"));
        assert_eq!(editor.unnamed_register(), register.as_ref());
    }
}

#[test]
fn ex_and_search_entry_release_the_consecutive_u_guard() {
    let directory = tempfile::tempdir().unwrap();
    let mut editor = editor("abc");
    let mut state = VisualState::default();
    editor
        .write_to(Some(&directory.path().join("text")))
        .unwrap();
    state.checkpoint_line(&editor);
    state.restore_line(&mut editor).unwrap();
    // Saving through the shared Editor doesn't invent a visual command.
    editor.save().unwrap();
    assert!(state.restore_line(&mut editor).is_err());
    state.execute_ex("w", &mut editor).unwrap();
    state.restore_line(&mut editor).unwrap();
    assert!(state.restore_line(&mut editor).is_err());
    assert!(state.execute_ex("unknown", &mut editor).is_err());
    state.restore_line(&mut editor).unwrap();
    assert!(state.search(&mut editor, "absent", true).is_err());
    state.restore_line(&mut editor).unwrap();
    state.search(&mut editor, "b", true).unwrap();
    state.restore_line(&mut editor).unwrap();
    for reverse in [false, true] {
        state.repeat_search(&mut editor, reverse).unwrap();
        state.restore_line(&mut editor).unwrap();
        assert!(state.restore_line(&mut editor).is_err());
    }
    assert_eq!(editor.buffer().to_text(), "abc");
}
