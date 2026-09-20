use vir::{
    buffer::TextBuffer,
    editor::{Editor, InputAction},
    visual::{InputStart, Motion, VisualState},
};

fn setup(text: &str, row: usize, column: usize) -> (Editor, VisualState) {
    let mut editor = Editor::from_buffer(TextBuffer::from_text(text).unwrap());
    editor.set_current(row).unwrap();
    let mut state = VisualState::default();
    state.row = row;
    state.column = column;
    (editor, state)
}

#[test]
fn join_undo_targets_first_restored_line_in_both_directions() {
    for (original, restored_column, joined_column) in [
        ("before\n  字abc\n\tdef\nlast", 2, 2),
        ("before\n  字abc\n\tdef\nlast\n", 2, 2),
        ("before\n\n  def\nlast", 0, 0),
        ("before\n \n\t\nlast", 0, 0),
    ] {
        let (mut editor, mut state) = setup(original, 1, 0);
        state.yank(&mut editor, None, None).unwrap();
        let register = editor.unnamed_register().cloned();
        state.join_lines(&mut editor, 2).unwrap();
        let joined = editor.buffer().clone();
        for toggle in 0..4 {
            state.row = editor.buffer().line_count() - 1;
            editor.set_current(state.row).unwrap();
            state.undo(&mut editor).unwrap();
            let expected = if toggle % 2 == 0 {
                TextBuffer::from_text(original).unwrap()
            } else {
                joined.clone()
            };
            assert_eq!(editor.buffer(), &expected);
            assert_eq!(
                (state.row, state.column),
                (
                    1,
                    if toggle % 2 == 0 {
                        restored_column
                    } else {
                        joined_column
                    }
                )
            );
            assert_eq!(editor.current(), Some(1));
            assert_eq!(editor.unnamed_register(), register.as_ref());
        }
    }
}

#[test]
fn whole_line_change_tracks_range_start_and_restored_indentation() {
    for (row, motion, count, start) in [
        (1, None, Some(2), 1),
        (1, Some(Motion::Down), Some(1), 1),
        (2, Some(Motion::Up), Some(1), 1),
        (0, None, Some(usize::MAX), 0),
    ] {
        for typed in ["", "\tX", "\tX\nY\nZ"] {
            let original = "  first\n  字abc\n\tdef\nlast";
            let (mut editor, mut state) = setup(original, row, 1);
            let mut edit = state.begin_change(&mut editor, motion, count).unwrap();
            edit.insert(typed).unwrap();
            state
                .commit_input(edit, InputStart::Change { motion, count })
                .unwrap();
            let changed = editor.buffer().clone();
            let register = editor.unnamed_register().cloned();
            editor
                .set_current(editor.buffer().line_count() - 1)
                .unwrap();
            state.undo(&mut editor).unwrap();
            assert_eq!(editor.buffer().to_text(), original);
            assert_eq!((state.row, state.column), (start, 2));
            state.undo(&mut editor).unwrap();
            assert_eq!(editor.buffer(), &changed);
            assert_eq!(
                (state.row, state.column),
                (start, usize::from(!typed.is_empty()))
            );
            assert_eq!(editor.unnamed_register(), register.as_ref());
        }
    }
}

#[test]
fn dot_count_uses_new_join_or_change_range_and_failure_keeps_history() {
    for change in [false, true] {
        let original = "  a\n  b\n  c\n  d\n  e\n  f\nlast";
        let (mut editor, mut state) = setup(original, 0, 1);
        if change {
            let mut edit = state.begin_change(&mut editor, None, Some(2)).unwrap();
            edit.insert("\tX").unwrap();
            state
                .commit_input(
                    edit,
                    InputStart::Change {
                        motion: None,
                        count: Some(2),
                    },
                )
                .unwrap();
        } else {
            state.join_lines(&mut editor, 2).unwrap();
        }
        let before_dot = editor.buffer().clone();
        state.row = 2;
        state.column = 0;
        editor.set_current(2).unwrap();
        state.repeat_change(&mut editor, Some(3)).unwrap();
        state.row = editor.buffer().line_count() - 1;
        editor.set_current(state.row).unwrap();
        assert!(state.join_lines(&mut editor, 2).is_err());
        state.undo(&mut editor).unwrap();
        assert_eq!(editor.buffer(), &before_dot);
        assert_eq!((state.row, state.column), (2, 2));
        state.undo(&mut editor).unwrap();
        assert_eq!((state.row, state.column), (2, if change { 1 } else { 2 }));
    }
}

#[test]
fn saved_join_and_mixed_undo_apis_keep_visual_targets_and_preferred_column() {
    let (mut editor, mut state) = setup("first\n  abc\n def\nlong last", 1, 4);
    state.join_lines(&mut editor, usize::MAX).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("joined");
    state
        .execute_ex(&format!("w {}", path.display()), &mut editor)
        .unwrap();
    editor.undo().unwrap();
    state.undo(&mut editor).unwrap();
    assert_eq!((state.row, state.column), (1, 2));
    state.execute_ex("u", &mut editor).unwrap();
    state.undo(&mut editor).unwrap();
    assert_eq!((state.row, state.column), (1, 2));
    state.undo(&mut editor).unwrap();
    state.move_cursor(Motion::Down, editor.buffer(), 8);
    assert_eq!((state.row, state.column), (2, 2));
    assert_eq!(
        std::fs::read_to_string(path).unwrap(),
        "first\n  abc def long last"
    );
}

#[test]
fn canceled_change_and_cross_boundary_input_preserve_appropriate_targets() {
    let original = "first\n  abc\n def\nlast";
    let (mut editor, mut state) = setup(original, 1, 0);
    state.replace_character(&mut editor, 'X').unwrap();
    drop(state.begin_change(&mut editor, None, Some(2)).unwrap());
    state.undo(&mut editor).unwrap();
    assert_eq!(editor.buffer().to_text(), original);
    for motion in [None, Some(Motion::End)] {
        let (mut editor, mut state) = setup(original, 1, 2);
        let mut edit = state.begin_change(&mut editor, motion, Some(2)).unwrap();
        if motion.is_none() {
            edit.apply_input(&InputAction::Backspace).unwrap(); // destroys preceding original line
        }
        edit.insert("X\nY").unwrap();
        state
            .commit_input(
                edit,
                InputStart::Change {
                    motion,
                    count: Some(2),
                },
            )
            .unwrap();
        state.undo(&mut editor).unwrap();
        assert_eq!(editor.buffer().to_text(), original);
        assert_eq!(
            (state.row, state.column),
            (1, if motion.is_some() { 2 } else { 0 })
        );
    }
}
