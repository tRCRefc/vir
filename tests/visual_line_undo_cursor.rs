use vir::{
    buffer::TextBuffer,
    editor::Editor,
    visual::{Motion, VisualState},
};
fn editor(text: &str) -> Editor {
    Editor::from_buffer(TextBuffer::from_text(text).unwrap())
}

#[test]
fn delete_rows_restore_first_deleted_then_preceding_line_despite_browsing() {
    let mut editor = editor("  first\n\tsecond\n  third\nlast");
    let mut state = VisualState::default();
    state.row = 1;
    editor.set_current(1).unwrap();
    state.delete_lines(&mut editor, 2).unwrap();
    state.row = 0;
    editor.set_current(0).unwrap();
    let register = editor.unnamed_register().cloned();
    state.undo(&mut editor).unwrap();
    assert_eq!((state.row, state.column), (1, 1));
    assert_eq!(editor.unnamed_register(), register.as_ref());
    state.row = 3;
    editor.set_current(3).unwrap();
    state.undo(&mut editor).unwrap();
    assert_eq!(editor.buffer().to_text(), "  first\nlast");
    assert_eq!((state.row, state.column), (0, 2));
    state.undo(&mut editor).unwrap();
    assert_eq!((state.row, state.column), (1, 1));
}

#[test]
fn backward_line_motion_uses_range_start_not_operator_origin() {
    let mut editor = editor("  first\n\tsecond\n  third\nlast");
    let mut state = VisualState::default();
    state.row = 2;
    editor.set_current(2).unwrap();
    state
        .delete_motion(&mut editor, Motion::Up, Some(1))
        .unwrap();
    state.undo(&mut editor).unwrap();
    assert_eq!((state.row, state.column), (1, 1));
}

#[test]
fn line_put_before_and_after_have_directional_targets() {
    for before in [false, true] {
        let mut editor = editor("  first\n\tsecond\nlast");
        let mut state = VisualState::default();
        state.yank(&mut editor, None, None).unwrap();
        state.row = 1;
        editor.set_current(1).unwrap();
        state.put(&mut editor, before, 2).unwrap();
        state.row = 4;
        editor.set_current(4).unwrap();
        state.undo(&mut editor).unwrap();
        assert_eq!(
            (state.row, state.column),
            if before { (0, 2) } else { (1, 1) }
        );
        state.undo(&mut editor).unwrap();
        assert_eq!(
            (state.row, state.column),
            if before { (1, 2) } else { (2, 2) }
        );
        assert_eq!(editor.buffer().line_count(), 5);
    }
}

#[test]
fn empty_file_and_boundary_targets_are_clamped_without_losing_direction() {
    let mut editor = editor("\ttext\n");
    let mut state = VisualState::default();
    state.delete_lines(&mut editor, 99).unwrap();
    state.undo(&mut editor).unwrap();
    assert_eq!((state.row, state.column), (0, 1));
    state.undo(&mut editor).unwrap();
    assert!(editor.buffer().is_empty());
    assert_eq!((state.row, state.column), (0, 0));
    state.put(&mut editor, true, 1).unwrap();
    state.undo(&mut editor).unwrap();
    assert!(editor.buffer().is_empty());
    state.undo(&mut editor).unwrap();
    assert_eq!((state.row, state.column), (0, 1));
}

#[test]
fn colon_undo_swaps_direction_without_adopting_visual_placement() {
    let mut editor = editor("first\n  second\nthird");
    let mut state = VisualState::default();
    state.row = 1;
    editor.set_current(1).unwrap();
    state.delete_lines(&mut editor, 1).unwrap();
    state.execute_ex("u", &mut editor).unwrap();
    assert_eq!((state.row, state.column), (1, 0));
    state.undo(&mut editor).unwrap();
    assert_eq!((state.row, state.column), (0, 0));
    state.undo(&mut editor).unwrap();
    assert_eq!((state.row, state.column), (1, 2));
}

#[test]
fn dot_publishes_fresh_line_targets_and_failed_commands_preserve_them() {
    let mut editor = editor("  a\n  b\n  c\n  d");
    let mut state = VisualState::default();
    state.delete_lines(&mut editor, 1).unwrap();
    state.row = 1;
    editor.set_current(1).unwrap();
    state.repeat_change(&mut editor, None).unwrap();
    assert!(state.execute_ex("999d", &mut editor).is_err());
    state.undo(&mut editor).unwrap();
    assert_eq!((state.row, state.column), (1, 2));
    assert_eq!(editor.buffer().to_text(), "  b\n  c\n  d");
    state.undo(&mut editor).unwrap();
    assert_eq!((state.row, state.column), (0, 2));
}

#[test]
fn open_line_input_uses_inserted_block_in_both_directions() {
    for mode in ['o', 'O'] {
        for typed in ["", "  X", "  X\n\tY"] {
            let mut editor = editor("  first\n\tsecond\nlast");
            let mut state = VisualState::default();
            state.row = 1;
            editor.set_current(1).unwrap();
            let mut input = state.begin_counted_insert(&mut editor, mode, 3).unwrap();
            input.paste(typed).unwrap();
            input.commit(&mut state).unwrap();
            let edited = editor.buffer().clone();
            state.row = 0;
            editor.set_current(0).unwrap();
            state.undo(&mut editor).unwrap();
            assert_eq!(
                (state.row, state.column),
                if mode == 'o' { (1, 1) } else { (0, 2) }
            );
            state.undo(&mut editor).unwrap();
            assert_eq!(editor.buffer(), &edited);
            assert_eq!(
                (state.row, state.column),
                (
                    if mode == 'o' { 2 } else { 1 },
                    if typed.is_empty() { 0 } else { 2 }
                )
            );
        }
    }
}

#[test]
fn open_empty_and_unterminated_file_preserves_undo_text_and_dot_location() {
    for text in ["", "  original", "  original\n"] {
        for mode in ['o', 'O'] {
            let mut editor = editor(text);
            let mut state = VisualState::default();
            let mut input = state.begin_counted_insert(&mut editor, mode, 1).unwrap();
            input.paste(" X\nY").unwrap();
            input.commit(&mut state).unwrap();
            state.undo(&mut editor).unwrap();
            assert_eq!(editor.buffer().to_text(), text);
            assert_eq!(state.column, if text.is_empty() { 0 } else { 2 });
            state.repeat_change(&mut editor, None).unwrap();
            state.undo(&mut editor).unwrap();
            assert_eq!(editor.buffer().to_text(), text);
            state.undo(&mut editor).unwrap();
            assert_eq!(state.column, 1);
        }
    }
}

#[test]
fn open_that_changes_original_text_keeps_conservative_fallback() {
    use vir::editor::InputAction;
    let mut editor = editor("  first\n  second\nlast");
    let mut state = VisualState::default();
    state.row = 1;
    editor.set_current(1).unwrap();
    let mut input = state.begin_counted_insert(&mut editor, 'O', 1).unwrap();
    input.apply_input(&InputAction::Delete).unwrap(); // merge original second into open line
    input.apply_input(&InputAction::Delete).unwrap(); // remove an original space
    input.paste("X\nY").unwrap();
    input.commit(&mut state).unwrap();
    state.undo(&mut editor).unwrap();
    assert_eq!(editor.buffer().to_text(), "  first\n  second\nlast");
    assert_eq!((state.row, state.column), (1, 0));
}
