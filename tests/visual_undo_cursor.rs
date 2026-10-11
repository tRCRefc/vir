use vir::{
    buffer::TextBuffer,
    editor::{Editor, InputAction},
    visual::{Motion, VisualState},
};
fn editor() -> Editor {
    Editor::from_buffer(TextBuffer::from_text("a中🙂def\nsecond").unwrap())
}

#[test]
fn undo_single_line_range_uses_scalar_anchor_not_browsing_cursor() {
    let mut editor = editor();
    let mut state = VisualState::default();
    state.column = 2;
    state.replace_counted(&mut editor, 'X', 2).unwrap();
    state.row = 1;
    state.column = 5;
    state.undo(&mut editor).unwrap();
    assert_eq!(editor.buffer().line(0), Some("a中🙂def"));
    assert_eq!((state.row, state.column), (0, 2));
    state.undo(&mut editor).unwrap();
    assert_eq!(editor.buffer().line(0), Some("a中XXef"));
    assert_eq!((state.row, state.column), (0, 2));
    state.move_cursor(Motion::Down, editor.buffer(), 8);
    assert_eq!(state.column, 2);
}

#[test]
fn input_and_backward_deletion_restore_the_edit_start() {
    for mode in ['i', 'a', 'A', 'R'] {
        let mut editor = editor();
        let mut state = VisualState::default();
        state.column = 2;
        let mut input = state.begin_counted_insert(&mut editor, mode, 2).unwrap();
        input.paste("XY").unwrap();
        input.commit(&mut state).unwrap();
        state.undo(&mut editor).unwrap();
        assert_eq!(
            state.column,
            match mode {
                'a' => 3,
                'A' => 5,
                _ => 2,
            }
        );
    }
    let mut editor = editor();
    let mut state = VisualState::default();
    state.column = 4;
    state
        .delete_motion(&mut editor, Motion::Left, Some(2))
        .unwrap();
    state.undo(&mut editor).unwrap();
    assert_eq!(state.column, 2);
}

#[test]
fn identical_r_publishes_anchor_and_failed_edits_preserve_it() {
    let mut editor = editor();
    let mut state = VisualState::default();
    state.column = 2;
    state.replace_character(&mut editor, 'X').unwrap();
    state.column = 0;
    state.replace_character(&mut editor, 'a').unwrap();
    assert!(state.replace_counted(&mut editor, 'Z', 99).is_err());
    let mut input = state.begin_counted_insert(&mut editor, 'i', 1).unwrap();
    input.apply_input(&InputAction::Insert("Q".into())).unwrap();
    input.apply_input(&InputAction::Backspace).unwrap();
    input.commit(&mut state).unwrap();
    state.undo(&mut editor).unwrap();
    assert_eq!(state.column, 0);
    assert_eq!(editor.buffer().line(0), Some("a中Xdef"));
    state.column = 4;
    state.repeat_change(&mut editor, None).unwrap();
    state.undo(&mut editor).unwrap();
    assert_eq!(state.column, 4);
}

#[test]
fn u_and_uppercase_u_interoperate_without_replacing_dot() {
    let mut editor = editor();
    let mut state = VisualState::default();
    state.checkpoint_line(&editor);
    state.column = 2;
    state.replace_character(&mut editor, 'X').unwrap();
    state.restore_line(&mut editor).unwrap();
    state.undo(&mut editor).unwrap();
    assert_eq!(state.column, 0);
    assert_eq!(editor.buffer().line(0), Some("a中Xdef"));
    state.restore_line(&mut editor).unwrap();
    state.repeat_change(&mut editor, None).unwrap();
    assert_eq!(editor.buffer().line(0), Some("X中🙂def"));
}

#[test]
fn ex_edit_replaces_anchor_and_colon_undo_keeps_line_semantics() {
    let mut editor = editor();
    editor.set_current(0).unwrap();
    let mut state = VisualState::default();
    state.column = 2;
    state.replace_character(&mut editor, 'X').unwrap();
    state.execute_ex("u", &mut editor).unwrap();
    assert_eq!(state.column, 0);
    state.column = 3;
    state.replace_character(&mut editor, 'Z').unwrap();
    state.execute_ex("s/Z/Q/", &mut editor).unwrap();
    state.undo(&mut editor).unwrap();
    assert_eq!(state.column, 0);
    assert_eq!(editor.buffer().line(0), Some("a中🙂Zef"));
}

#[test]
fn multiline_and_reversed_split_keep_line_level_fallback() {
    let mut editor = editor();
    let mut state = VisualState::default();
    editor.set_current(0).unwrap();
    state.column = 2;
    let mut input = state.begin_counted_insert(&mut editor, 'i', 1).unwrap();
    input.paste("X\n").unwrap();
    input.apply_input(&InputAction::Backspace).unwrap();
    input.commit(&mut state).unwrap();
    state.undo(&mut editor).unwrap();
    assert_eq!((state.row, state.column), (0, 0));
}

#[test]
fn save_preserves_anchor_reload_clears_it_and_failed_undo_is_atomic() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("file");
    std::fs::write(&path, "abcdef").unwrap();
    let mut editor = Editor::default();
    let mut state = VisualState::default();
    state
        .execute_ex(&format!("e {}", path.display()), &mut editor)
        .unwrap();
    state.column = 3;
    state.replace_character(&mut editor, 'X').unwrap();
    state.execute_ex("w", &mut editor).unwrap();
    state.undo(&mut editor).unwrap();
    assert_eq!(state.column, 3);
    state.execute_ex("e!", &mut editor).unwrap();
    let before = editor.clone();
    let visual = format!("{state:?}");
    assert!(state.undo(&mut editor).is_err());
    assert_eq!(editor, before);
    assert_eq!(format!("{state:?}"), visual);
}

#[test]
fn ctrl_u_tracks_the_deleted_original_prefix_through_undo_redo_and_dot() {
    for original in ["old tail\n", "中🙂 tail\n"] {
        let column = original.find("tail").unwrap();
        let column = original[..column].chars().count();
        for count in [1, 3] {
            let mut editor = Editor::from_buffer(TextBuffer::from_text(original).unwrap());
            let mut state = VisualState::default();
            state.column = column;
            let mut input = state.begin_counted_insert(&mut editor, 'i', count).unwrap();
            input.apply_input(&InputAction::KillLine).unwrap();
            input.apply_input(&InputAction::Insert("X".into())).unwrap();
            input.commit(&mut state).unwrap();
            let edited = format!("{}tail\n", "X".repeat(count));
            assert_eq!(editor.buffer().to_text(), edited);
            state.move_cursor(Motion::End, editor.buffer(), 8);
            state.undo(&mut editor).unwrap();
            assert_eq!(editor.buffer().to_text(), original);
            assert_eq!((state.row, state.column), (0, 0));
            state.undo(&mut editor).unwrap();
            assert_eq!(editor.buffer().to_text(), edited);
            assert_eq!((state.row, state.column), (0, 0));
            state.undo(&mut editor).unwrap();
            state.column = column;
            state.repeat_change(&mut editor, None).unwrap();
            assert_eq!(editor.buffer().to_text(), edited);
            state.undo(&mut editor).unwrap();
            assert_eq!(editor.buffer().to_text(), original);
            assert_eq!((state.row, state.column), (0, 0));
        }
    }
}
