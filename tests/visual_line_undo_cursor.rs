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
