use vir::{
    buffer::TextBuffer,
    editor::{Editor, InputAction},
    visual::{Motion, VisualState},
};

#[test]
fn newline_replace_undo_targets_first_restored_line_in_both_directions() {
    for tail in ["", "\n"] {
        for (column, count, split_column) in [(0, 2, 0), (2, 2, 1), (3, 1, 2), (4, 2, 2)] {
            let original = format!("first\n  字abc\nlast{tail}");
            let mut editor = Editor::from_buffer(TextBuffer::from_text(&original).unwrap());
            let mut state = VisualState::default();
            state.row = 1;
            state.column = column;
            editor.set_current(1).unwrap();
            state.yank(&mut editor, None, None).unwrap();
            let register = editor.unnamed_register().cloned();
            state.replace_counted(&mut editor, '\n', count).unwrap();
            assert_eq!((state.row, state.column), (2, 0));
            let split = editor.buffer().clone();
            for toggle in 0..4 {
                state.row = editor.buffer().line_count() - 1;
                editor.set_current(state.row).unwrap();
                state.undo(&mut editor).unwrap();
                assert_eq!(
                    (state.row, state.column),
                    (1, if toggle % 2 == 0 { 2 } else { split_column })
                );
                assert_eq!(editor.current(), Some(1));
                assert_eq!(editor.unnamed_register(), register.as_ref());
                assert_eq!(editor.buffer().has_trailing_newline(), !tail.is_empty());
                if toggle % 2 == 0 {
                    assert_eq!(editor.buffer().to_text(), original);
                } else {
                    assert_eq!(editor.buffer(), &split);
                }
            }
        }
    }
}

#[test]
fn newline_replace_dot_uses_new_range_and_failed_replace_preserves_it() {
    let mut editor = Editor::from_buffer(TextBuffer::from_text("  abc\n  def\nlast").unwrap());
    let mut state = VisualState::default();
    state.column = 3;
    state.replace_counted(&mut editor, '\n', 2).unwrap();
    state.row = 2;
    state.column = 2;
    editor.set_current(2).unwrap();
    state.repeat_change(&mut editor, Some(1)).unwrap();
    assert!(
        state
            .replace_counted(&mut editor, '\n', usize::MAX)
            .is_err()
    );
    state.undo(&mut editor).unwrap();
    assert_eq!((state.row, state.column), (2, 2));
    assert_eq!(editor.buffer().to_text(), "  a\n\n  def\nlast");
    state.undo(&mut editor).unwrap();
    assert_eq!((state.row, state.column), (2, 1));
    state.undo(&mut editor).unwrap();
    state.replace_character(&mut editor, 'X').unwrap();
    state.undo(&mut editor).unwrap();
    assert_eq!((state.row, state.column), (2, 2));
}

#[test]
fn whole_line_replaced_by_newline_handles_empty_lines_and_saved_undo() {
    for original in ["字", "字\n"] {
        let mut editor = Editor::from_buffer(TextBuffer::from_text(original).unwrap());
        let mut state = VisualState::default();
        state.replace_character(&mut editor, '\n').unwrap();
        assert_eq!(editor.buffer().line_count(), 2);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("text");
        state
            .execute_ex(&format!("w {}", path.display()), &mut editor)
            .unwrap();
        state.execute_ex("u", &mut editor).unwrap();
        assert_eq!(editor.buffer().to_text(), original);
        state.undo(&mut editor).unwrap();
        assert_eq!((state.row, state.column), (0, 0));
        state.undo(&mut editor).unwrap();
        assert_eq!((state.row, state.column), (0, 0));
        assert_eq!(editor.buffer().to_text(), original);
        assert_eq!(
            std::fs::read_to_string(path).unwrap(),
            if original.ends_with('\n') {
                "\n\n"
            } else {
                "\n"
            }
        );
    }
}

#[test]
fn character_put_into_zero_lines_keeps_explicit_fallback_and_restores_zero_lines() {
    for before in [false, true] {
        for count in [1, 2] {
            let mut editor = Editor::from_buffer(TextBuffer::from_text("  abc\n  def").unwrap());
            let mut state = VisualState::default();
            state.column = 3;
            state.yank(&mut editor, Some(Motion::End), Some(2)).unwrap();
            let register = editor.unnamed_register().cloned();
            state.execute_ex("%d", &mut editor).unwrap();
            assert!(editor.buffer().is_empty());
            state.put(&mut editor, before, count).unwrap();
            let inserted = editor.buffer().clone();
            state.undo(&mut editor).unwrap();
            assert!(editor.buffer().is_empty());
            assert_eq!((state.row, state.column), (0, 0));
            state.undo(&mut editor).unwrap();
            assert_eq!(editor.buffer(), &inserted);
            assert_eq!((state.row, state.column), (count, 0));
            assert_eq!(editor.unnamed_register(), register.as_ref());
        }
    }
}

#[test]
fn input_crossing_original_line_boundary_keeps_undo_fallback() {
    for backspace in [false, true] {
        let original = "  first\n  second\nlast";
        let mut editor = Editor::from_buffer(TextBuffer::from_text(original).unwrap());
        let mut state = VisualState::default();
        state.row = 1;
        state.column = 0;
        editor.set_current(1).unwrap();
        let mut input = state
            .begin_counted_insert(&mut editor, if backspace { 'i' } else { 'A' }, 1)
            .unwrap();
        input
            .apply_input(&if backspace {
                InputAction::Backspace
            } else {
                InputAction::Delete
            })
            .unwrap();
        input.paste("X\nY").unwrap();
        input.commit(&mut state).unwrap();
        let edited = editor.buffer().clone();
        state.undo(&mut editor).unwrap();
        assert_eq!(editor.buffer().to_text(), original);
        assert_eq!((state.row, state.column), (1, 0));
        state.undo(&mut editor).unwrap();
        assert_eq!(editor.buffer(), &edited);
        assert_eq!(state.column, 0);
    }
}
