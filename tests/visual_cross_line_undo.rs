use vir::{
    buffer::TextBuffer,
    editor::Editor,
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
fn cross_line_delete_uses_range_column_once_then_first_nonblank() {
    for tail in ["", "\n"] {
        let original = format!("first\n  字abc\n  def\nlast{tail}");
        let (mut editor, mut state) = setup(&original, 1, 4);
        state
            .delete_motion(&mut editor, Motion::End, Some(2))
            .unwrap();
        assert_eq!(
            editor.buffer().to_text(),
            format!("first\n  字a\nlast{tail}")
        );
        let edited = editor.buffer().clone();
        let register = editor.unnamed_register().cloned();
        for toggle in 0..4 {
            state.row = editor.buffer().line_count() - 1;
            editor.set_current(state.row).unwrap();
            state.undo(&mut editor).unwrap();
            assert_eq!(
                (state.row, state.column),
                (1, if toggle == 0 { 4 } else { 2 })
            );
            assert_eq!(editor.current(), Some(1));
            assert_eq!(editor.unnamed_register(), register.as_ref());
            if toggle % 2 == 0 {
                assert_eq!(editor.buffer().to_text(), original);
            } else {
                assert_eq!(editor.buffer(), &edited);
            }
        }
    }
}

#[test]
fn backward_delete_and_change_use_normalized_range_start() {
    for change in [false, true] {
        let original = "first\n  abc def\n  ghi\nlast";
        let (mut editor, mut state) = setup(original, 2, 2);
        if change {
            let mut edit = state
                .begin_change(&mut editor, Some(Motion::WordBackward), None)
                .unwrap();
            edit.insert("X\nY").unwrap();
            state
                .commit_input(
                    edit,
                    InputStart::Change {
                        motion: Some(Motion::WordBackward),
                        count: None,
                    },
                )
                .unwrap();
        } else {
            state
                .delete_motion(&mut editor, Motion::WordBackward, None)
                .unwrap();
            assert_eq!(editor.buffer().to_text(), "first\n  abc ghi\nlast");
        }
        state.undo(&mut editor).unwrap();
        assert_eq!(editor.buffer().to_text(), original);
        assert_eq!((state.row, state.column), (1, 6));
        state.undo(&mut editor).unwrap();
        assert_eq!((state.row, state.column), (1, 2));
        state.undo(&mut editor).unwrap();
        assert_eq!((state.row, state.column), (1, 2));
    }
}

#[test]
fn change_empty_or_multiline_input_and_dot_publish_fresh_initial_columns() {
    for typed in ["", "X", "X\nY\nZ"] {
        let (mut editor, mut state) = setup("first\n  abc\n  def\n  ghi\n  jkl\nlast", 1, 3);
        let mut edit = state
            .begin_change(&mut editor, Some(Motion::End), Some(2))
            .unwrap();
        edit.insert(typed).unwrap();
        state
            .commit_input(
                edit,
                InputStart::Change {
                    motion: Some(Motion::End),
                    count: Some(2),
                },
            )
            .unwrap();
        state.undo(&mut editor).unwrap();
        assert_eq!((state.row, state.column), (1, 3));
        state.row = 3;
        state.column = 4;
        editor.set_current(3).unwrap();
        state.repeat_change(&mut editor, None).unwrap();
        assert!(state.execute_ex("999d", &mut editor).is_err());
        state.undo(&mut editor).unwrap();
        assert_eq!((state.row, state.column), (3, 4));
        state.undo(&mut editor).unwrap();
        assert_eq!((state.row, state.column), (3, 2));
    }
}

#[test]
fn shared_undo_consumes_initial_column_and_save_preserves_it() {
    for direct in [false, true] {
        let (mut editor, mut state) = setup("first\n  abc\n  def\nlast", 1, 4);
        state
            .delete_motion(&mut editor, Motion::End, Some(2))
            .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("text");
        state
            .execute_ex(&format!("w {}", path.display()), &mut editor)
            .unwrap();
        if direct {
            editor.undo().unwrap();
        } else {
            state.execute_ex("u", &mut editor).unwrap();
        }
        state.undo(&mut editor).unwrap();
        assert_eq!((state.row, state.column), (1, 2));
        state.undo(&mut editor).unwrap();
        assert_eq!((state.row, state.column), (1, 2));
        assert_eq!(std::fs::read_to_string(path).unwrap(), "first\n  ab\nlast");
    }
    let (mut editor, mut state) = setup("first\n  abc\n  def\nlast", 1, 4);
    state
        .delete_motion(&mut editor, Motion::End, Some(2))
        .unwrap();
    drop(
        state
            .begin_change(&mut editor, Some(Motion::End), Some(2))
            .unwrap(),
    );
    state.undo(&mut editor).unwrap();
    assert_eq!((state.row, state.column), (1, 4));
    state.move_cursor(Motion::Down, editor.buffer(), 8);
    assert_eq!((state.row, state.column), (2, 4));
}

#[test]
fn multiline_character_put_uses_first_restored_line_in_both_directions() {
    for before in [false, true] {
        for count in [1, 3] {
            let original = "first\n  abc\n  def\n\ttarget\nlast";
            let (mut editor, mut state) = setup(original, 1, 3);
            state.yank(&mut editor, Some(Motion::End), Some(2)).unwrap();
            let register = editor.unnamed_register().cloned();
            state.row = 3;
            state.column = 3;
            editor.set_current(3).unwrap();
            state.put(&mut editor, before, count).unwrap();
            let edited = editor.buffer().clone();
            for toggle in 0..4 {
                state.row = editor.buffer().line_count() - 1;
                editor.set_current(state.row).unwrap();
                state.undo(&mut editor).unwrap();
                assert_eq!((state.row, state.column), (3, 1));
                assert_eq!(editor.unnamed_register(), register.as_ref());
                if toggle % 2 == 0 {
                    assert_eq!(editor.buffer().to_text(), original);
                } else {
                    assert_eq!(editor.buffer(), &edited);
                }
            }
            state.row = 0;
            state.column = 2;
            editor.set_current(0).unwrap();
            state.repeat_change(&mut editor, Some(2)).unwrap();
            state.undo(&mut editor).unwrap();
            assert_eq!((state.row, state.column), (0, 0));
        }
    }
}

#[test]
fn empty_line_cross_delete_and_new_single_line_edit_do_not_reuse_initial_column() {
    let (mut editor, mut state) = setup("\n\nlast", 0, 0);
    state
        .delete_motion(&mut editor, Motion::End, Some(2))
        .unwrap();
    state.undo(&mut editor).unwrap();
    assert_eq!((state.row, state.column), (0, 0));
    state.undo(&mut editor).unwrap();
    assert_eq!((state.row, state.column), (0, 0));
    let (mut editor, mut state) = setup("first\n  abc\n  def\nlast", 1, 4);
    state
        .delete_motion(&mut editor, Motion::End, Some(2))
        .unwrap();
    state.column = 2;
    state.replace_character(&mut editor, 'X').unwrap();
    for _ in 0..3 {
        state.undo(&mut editor).unwrap();
        assert_eq!((state.row, state.column), (1, 2));
    }
}
