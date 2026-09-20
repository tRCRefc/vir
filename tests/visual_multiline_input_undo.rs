use vir::{
    buffer::TextBuffer,
    editor::{Editor, InputAction},
    visual::{InputStart, Motion, VisualState},
};

fn setup(text: &str) -> (Editor, VisualState) {
    let mut editor = Editor::from_buffer(TextBuffer::from_text(text).unwrap());
    let mut state = VisualState::default();
    state.row = 1;
    state.column = 3;
    editor.set_current(1).unwrap();
    (editor, state)
}

#[test]
fn multiline_input_restores_first_replaced_line_in_both_directions() {
    for mode in ['i', 'a', 'I', 'A', 'R', 'C'] {
        for tail in ["", "\n"] {
            let original = format!("first\n  字abc\nlast{tail}");
            let (mut editor, mut state) = setup(&original);
            let mut input = state
                .begin_counted_insert(&mut editor, mode, if mode == 'C' { 1 } else { 3 })
                .unwrap();
            input.paste("X\n\tY\nZ").unwrap();
            input.commit(&mut state).unwrap();
            let edited = editor.buffer().clone();
            let register = editor.unnamed_register().cloned();
            for toggle in 0..4 {
                state.row = editor.buffer().line_count() - 1;
                editor.set_current(state.row).unwrap();
                state.undo(&mut editor).unwrap();
                assert_eq!((state.row, state.column), (1, 2), "{mode}, {toggle}");
                assert_eq!(editor.current(), Some(1));
                if toggle % 2 == 0 {
                    assert_eq!(editor.buffer().to_text(), original);
                } else {
                    assert_eq!(editor.buffer(), &edited);
                }
                assert_eq!(editor.unnamed_register(), register.as_ref());
            }
        }
    }
}

#[test]
fn same_line_change_and_legacy_input_share_multiline_targets() {
    for motion in [None, Some(Motion::Right), Some(Motion::End)] {
        let (mut editor, mut state) = setup("first\n  字abc\nlast");
        let mut edit = state.begin_change(&mut editor, motion, Some(1)).unwrap();
        edit.insert("\tX\nY").unwrap();
        state
            .commit_input(
                edit,
                InputStart::Change {
                    motion,
                    count: Some(1),
                },
            )
            .unwrap();
        let edited = editor.buffer().clone();
        state.undo(&mut editor).unwrap();
        assert_eq!((state.row, state.column), (1, 2));
        state.undo(&mut editor).unwrap();
        assert_eq!(state.row, 1);
        assert_eq!(state.column, if motion.is_none() { 1 } else { 2 });
        assert_eq!(editor.buffer(), &edited);
    }
}

#[test]
fn dot_and_colon_undo_keep_actual_multiline_range() {
    let (mut editor, mut state) = setup("first\n  abc\n  def\nlast");
    let mut input = state.begin_counted_insert(&mut editor, 'i', 2).unwrap();
    input.paste("X\nY").unwrap();
    input.commit(&mut state).unwrap();
    state.row = 3;
    state.column = 2;
    editor.set_current(3).unwrap();
    state.repeat_change(&mut editor, Some(3)).unwrap();
    assert!(state.execute_ex("999d", &mut editor).is_err());
    state.execute_ex("u", &mut editor).unwrap();
    state.undo(&mut editor).unwrap();
    assert_eq!((state.row, state.column), (3, 2));
    state.undo(&mut editor).unwrap();
    assert_eq!((state.row, state.column), (3, 2));
    assert_eq!(editor.buffer().line(3), Some("  def"));
}

#[test]
fn changed_neighbor_and_split_rejoined_line_keep_fallback() {
    for neighbor in [false, true] {
        let (mut editor, mut state) = setup("first\n  abc\nlast");
        let mut input = state.begin_counted_insert(&mut editor, 'A', 1).unwrap();
        if neighbor {
            input.apply_input(&InputAction::Delete).unwrap();
            input.paste("X\nY\nZ").unwrap();
        } else {
            input.paste("\n").unwrap();
            input.apply_input(&InputAction::Backspace).unwrap();
            input.paste("X").unwrap();
        }
        input.commit(&mut state).unwrap();
        state.undo(&mut editor).unwrap();
        assert_eq!((state.row, state.column), (1, 0));
    }
}

#[test]
fn empty_first_or_last_line_and_empty_buffer_are_safe() {
    for original in ["", "\n", "  字", "  字\n", "\t\nlast"] {
        let mut editor = Editor::from_buffer(TextBuffer::from_text(original).unwrap());
        let mut state = VisualState::default();
        let mut input = state.begin_counted_insert(&mut editor, 'i', 1).unwrap();
        input.paste(" X\nY").unwrap();
        input.commit(&mut state).unwrap();
        state.undo(&mut editor).unwrap();
        assert_eq!(editor.buffer().to_text(), original);
        assert_eq!(state.row, 0);
        assert_eq!(
            state.column,
            if original.starts_with("  字") { 2 } else { 0 }
        );
        state.undo(&mut editor).unwrap();
        assert_eq!(state.row, usize::from(original.is_empty()));
        // Empty-buffer insertion deliberately retains the previous fallback.
        assert_eq!(state.column, usize::from(!original.is_empty()));
    }
}
