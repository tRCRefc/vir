use vir::{
    buffer::TextBuffer,
    editor::{Editor, InputAction},
    visual::{Motion, VisualState},
};
fn editor(text: &str) -> Editor {
    Editor::from_buffer(TextBuffer::from_text(text).unwrap())
}

#[test]
fn historical_open_counts_do_not_repeat_text_or_newlines() {
    for mode in ['o', 'O'] {
        for count in [1, 3, 1_000_001, usize::MAX] {
            for original in ["abc", "abc\n"] {
                let mut editor = editor(original);
                let mut state = VisualState::default();
                state.yank(&mut editor, Some(Motion::End), None).unwrap();
                let register = editor.unnamed_register().cloned();
                let mut input = state
                    .begin_counted_insert(&mut editor, mode, count)
                    .unwrap();
                input
                    .apply_input(&InputAction::Insert("字🙂\nY".into()))
                    .unwrap();
                let expected = if mode == 'o' {
                    "abc\n字🙂\nY\n"
                } else {
                    "字🙂\nY\nabc\n"
                };
                assert_eq!(input.buffer().to_text(), expected);
                input.commit(&mut state).unwrap();
                assert_eq!(editor.buffer().to_text(), expected);
                assert_eq!(state.row, if mode == 'o' { 2 } else { 1 });
                assert_eq!(state.column, 0);
                assert_eq!(editor.unnamed_register(), register.as_ref());
                editor.undo().unwrap();
                assert_eq!(editor.buffer().to_text(), original);
                editor.undo().unwrap();
                assert_eq!(editor.buffer().to_text(), expected);
            }
        }
    }
}

#[test]
fn open_dot_count_is_ignored_even_for_multiline_input() {
    for mode in ['o', 'O'] {
        let mut editor = editor("abc\n");
        let mut state = VisualState::default();
        let mut input = state.begin_counted_insert(&mut editor, mode, 3).unwrap();
        input.paste("X\r\nY").unwrap();
        input.commit(&mut state).unwrap();
        state.row = 0;
        state.column = 0;
        state.repeat_change(&mut editor, Some(usize::MAX)).unwrap();
        assert_eq!(
            editor.buffer().to_text(),
            if mode == 'o' {
                "abc\nX\nY\nX\nY\n"
            } else {
                "X\nY\nX\nY\nabc\n"
            }
        );
        state.repeat_change(&mut editor, None).unwrap();
        assert_eq!(editor.buffer().line_count(), 7);
        editor.undo().unwrap();
        assert_eq!(editor.buffer().line_count(), 5);
    }
}

#[test]
fn empty_open_commits_one_line_and_dropped_draft_cancels_open() {
    for mode in ['o', 'O'] {
        for original in ["", "abc\n"] {
            let mut editor = editor(original);
            let mut state = VisualState::default();
            let before = editor.clone();
            drop(state.begin_counted_insert(&mut editor, mode, 7).unwrap());
            assert_eq!(editor, before);
            state
                .begin_counted_insert(&mut editor, mode, 7)
                .unwrap()
                .commit(&mut state)
                .unwrap();
            assert_eq!(
                editor.buffer().line_count(),
                if original.is_empty() { 1 } else { 2 }
            );
            state.repeat_change(&mut editor, Some(9)).unwrap();
            assert_eq!(
                editor.buffer().line_count(),
                if original.is_empty() { 2 } else { 3 }
            );
            editor.undo().unwrap();
            assert_eq!(
                editor.buffer().line_count(),
                if original.is_empty() { 1 } else { 2 }
            );
        }
    }
}

#[test]
fn open_backspace_and_rejected_paste_preserve_the_transaction() {
    let mut editor = editor("abc\n");
    let mut state = VisualState::default();
    let mut input = state
        .begin_counted_insert(&mut editor, 'o', 500_000)
        .unwrap();
    input
        .apply_input(&InputAction::Insert("字🙂".into()))
        .unwrap();
    input.apply_input(&InputAction::Backspace).unwrap();
    assert!(input.paste(&"X".repeat(65_537)).is_err());
    assert_eq!(input.buffer().to_text(), "abc\n字\n");
    input.commit(&mut state).unwrap();
    state.repeat_change(&mut editor, Some(3)).unwrap();
    assert_eq!(editor.buffer().to_text(), "abc\n字\n字\n");
    editor.undo().unwrap();
    assert_eq!(editor.buffer().to_text(), "abc\n字\n");
}
