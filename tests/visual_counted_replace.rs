use vir::{
    buffer::TextBuffer,
    editor::Editor,
    visual::{Motion, VisualState},
};

fn editor(text: &str) -> Editor {
    Editor::from_buffer(TextBuffer::from_text(text).unwrap())
}

#[test]
fn counted_unicode_replace_has_one_undo_and_preserves_register_and_final_lf() {
    for original in ["a中🙂def\n", "a中🙂def"] {
        let mut editor = editor(original);
        let mut state = VisualState::default();
        state.yank(&mut editor, Some(Motion::End), None).unwrap();
        let register = editor.unnamed_register().cloned();
        state.column = 1;
        state.replace_counted(&mut editor, '字', 3).unwrap();
        assert_eq!(
            editor.buffer().to_text(),
            if original.ends_with('\n') {
                "a字字字ef\n"
            } else {
                "a字字字ef"
            }
        );
        assert_eq!(state.column, 3);
        assert_eq!(editor.unnamed_register(), register.as_ref());
        let replaced = editor.buffer().clone();
        editor.undo().unwrap();
        assert_eq!(editor.buffer().to_text(), original);
        editor.undo().unwrap();
        assert_eq!(editor.buffer(), &replaced);
    }
}

#[test]
fn dot_retains_then_overrides_count_like_historical_r() {
    let mut editor = editor("abcdefghij\n");
    let mut state = VisualState::default();
    state.replace_counted(&mut editor, 'X', 3).unwrap();
    assert_eq!(editor.buffer().to_text(), "XXXdefghij\n");
    assert_eq!(state.column, 2);
    state.move_cursor(Motion::Right, editor.buffer(), 8);
    state.repeat_change(&mut editor, Some(2)).unwrap();
    assert_eq!(editor.buffer().to_text(), "XXXXXfghij\n");
    state.move_cursor(Motion::Right, editor.buffer(), 8);
    state.repeat_change(&mut editor, None).unwrap();
    assert_eq!(editor.buffer().to_text(), "XXXXXXXhij\n");
    assert_eq!(state.column, 6);
    editor.undo().unwrap();
    assert_eq!(editor.buffer().to_text(), "XXXXXfghij\n");
}

#[test]
fn short_line_overflow_and_invalid_character_fail_without_losing_previous_repeat() {
    let mut editor = editor("abcdef\nnext");
    let mut state = VisualState::default();
    state.replace_counted(&mut editor, 'X', 2).unwrap();
    state.column = 4;
    let before = editor.clone();
    let visual = format!("{state:?}");
    for (character, count) in [('Y', 3), ('Y', usize::MAX), ('\0', 1), ('\r', 1)] {
        assert!(
            state
                .replace_counted(&mut editor, character, count)
                .is_err()
        );
        assert_eq!(editor, before);
        assert_eq!(format!("{state:?}"), visual);
    }
    state.repeat_change(&mut editor, None).unwrap();
    assert_eq!(editor.buffer().to_text(), "XXcdXX\nnext");
    assert_eq!(state.column, 5);
    for text in ["", "\n"] {
        let mut empty = self::editor(text);
        let before = empty.clone();
        let mut empty_state = VisualState::default();
        assert!(empty_state.replace_counted(&mut empty, 'X', 1).is_err());
        assert_eq!(empty, before);
    }
}

#[test]
fn same_character_replacement_keeps_existing_noop_policy() {
    let mut editor = editor("abcdef");
    let mut state = VisualState::default();
    state.replace_counted(&mut editor, 'X', 3).unwrap();
    state.column = 0;
    let before = editor.clone();
    let visual = format!("{state:?}");
    state.replace_counted(&mut editor, 'X', 2).unwrap();
    assert_eq!(editor, before);
    assert_eq!(format!("{state:?}"), visual);
    state.column = 3;
    state.repeat_change(&mut editor, None).unwrap();
    assert_eq!(editor.buffer().to_text(), "XXXXXX");
}

#[test]
fn newline_replaces_counted_range_with_one_split_and_repeats_atomically() {
    for lf in ["", "\n"] {
        let original = format!("a中🙂def{lf}");
        let mut editor = editor(&original);
        let mut state = VisualState::default();
        state.yank(&mut editor, Some(Motion::End), None).unwrap();
        let register = editor.unnamed_register().cloned();
        state.column = 1;
        state.replace_counted(&mut editor, '\n', 2).unwrap();
        assert_eq!(editor.buffer().to_text(), format!("a\ndef{lf}"));
        assert_eq!((state.row, state.column), (1, 0));
        assert_eq!(editor.unnamed_register(), register.as_ref());
        state.repeat_change(&mut editor, Some(1)).unwrap();
        assert_eq!(editor.buffer().to_text(), format!("a\n\nef{lf}"));
        assert_eq!((state.row, state.column), (2, 0));
        editor.undo().unwrap();
        assert_eq!(editor.buffer().to_text(), format!("a\ndef{lf}"));
        state.row = 0;
        state.column = 0;
        let before = editor.clone();
        assert!(state.repeat_change(&mut editor, Some(2)).is_err());
        assert_eq!(editor, before);
    }
}

#[test]
fn tab_and_literal_controls_repeat_as_text_with_single_undo() {
    for c in ['\t', '\x1b', '\x03', '\x08', '\x7f'] {
        let mut editor = editor("abcdef\n");
        let mut state = VisualState::default();
        state.replace_counted(&mut editor, c, 3).unwrap();
        assert_eq!(
            editor.buffer().to_text(),
            format!("{}def\n", c.to_string().repeat(3))
        );
        assert_eq!(state.column, 2);
        state.column = 3;
        state.repeat_change(&mut editor, None).unwrap();
        assert_eq!(
            editor.buffer().to_text(),
            format!("{}\n", c.to_string().repeat(6))
        );
        editor.undo().unwrap();
        assert_eq!(
            editor.buffer().to_text(),
            format!("{}def\n", c.to_string().repeat(3))
        );
    }
}

#[test]
fn newline_at_line_end_leaves_a_valid_cursor_and_undo() {
    for original in ["abc", "abc\n", "abc\nnext"] {
        let mut editor = editor(original);
        let mut state = VisualState::default();
        state.column = 1;
        state.replace_counted(&mut editor, '\n', 2).unwrap();
        assert_eq!((state.row, state.column), (1, 0));
        assert_eq!(editor.buffer().line(1), Some(""));
        editor.undo().unwrap();
        assert_eq!(editor.buffer().to_text(), original);
    }
}
