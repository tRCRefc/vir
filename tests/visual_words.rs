use vir::{
    buffer::TextBuffer,
    editor::Editor,
    visual::{Motion, VisualState},
};

fn walk(text: &str, start: (usize, usize), motion: Motion, targets: &[(usize, usize)]) {
    let buffer = TextBuffer::from_text(text).unwrap();
    let mut state = VisualState::default();
    (state.row, state.column) = start;
    for target in targets {
        state.move_cursor(motion, &buffer, 8);
        assert_eq!((state.row, state.column), *target);
    }
}

#[test]
fn words_distinguish_unicode_punctuation_and_whitespace() {
    let text = "ab_2中文..🙂!\txy\u{2003}z";
    walk(
        text,
        (0, 0),
        Motion::WordForward,
        &[(0, 6), (0, 11), (0, 14), (0, 14)],
    );
    walk(
        text,
        (0, 14),
        Motion::WordBackward,
        &[(0, 11), (0, 6), (0, 0), (0, 0)],
    );
    walk(
        text,
        (0, 0),
        Motion::WordEnd,
        &[(0, 5), (0, 9), (0, 12), (0, 14), (0, 14)],
    );
    walk("abc def", (0, 1), Motion::WordBackward, &[(0, 0)]);
    walk("abc def", (0, 2), Motion::WordEnd, &[(0, 6)]);
}

#[test]
fn cross_line_words_stop_at_empty_lines_but_ends_skip_them() {
    let text = "one\n\n \t\ntwo three\n\nlast";
    walk(
        text,
        (0, 0),
        Motion::WordForward,
        &[(1, 0), (3, 0), (3, 4), (4, 0), (5, 0), (5, 3)],
    );
    walk(
        text,
        (5, 3),
        Motion::WordBackward,
        &[(5, 0), (4, 0), (3, 4), (3, 0), (1, 0), (0, 0)],
    );
    walk(
        text,
        (0, 0),
        Motion::WordEnd,
        &[(0, 2), (3, 2), (3, 8), (5, 3), (5, 3)],
    );
    walk("a\nb", (0, 0), Motion::WordForward, &[(1, 0)]);
    walk("a\nb", (1, 0), Motion::WordBackward, &[(0, 0)]);
    walk("a\n\n", (0, 0), Motion::WordEnd, &[(1, 0), (1, 0)]);
}

#[test]
fn counts_clamp_and_empty_or_whitespace_buffers_are_safe() {
    for text in ["", "\n", " \t", "\n\n", "a\n\n", "one two\n三四!!\t"] {
        let buffer = TextBuffer::from_text(text).unwrap();
        let mut state = VisualState::default();
        for motion in [
            Motion::WordForward,
            Motion::WordEnd,
            Motion::BigWordForward,
            Motion::BigWordEnd,
        ] {
            state.move_counted(motion, Some(usize::MAX), &buffer, 0);
            assert_eq!(state.row, buffer.line_count().saturating_sub(1));
            assert_eq!(
                state.column,
                buffer
                    .line(state.row)
                    .unwrap_or("")
                    .chars()
                    .count()
                    .saturating_sub(1)
            );
        }
        for backward in [Motion::WordBackward, Motion::BigWordBackward] {
            state.move_cursor(Motion::Last, &buffer, 0);
            state.move_cursor(Motion::End, &buffer, 0);
            state.move_counted(backward, Some(usize::MAX), &buffer, 0);
            assert_eq!((state.row, state.column), (0, 0));
        }
    }
    let buffer = TextBuffer::from_text("one two three\nfour").unwrap();
    let mut state = VisualState::default();
    state.move_counted(Motion::WordForward, Some(2), &buffer, 8);
    assert_eq!((state.row, state.column), (0, 8));
    state.move_counted(Motion::WordBackward, Some(0), &buffer, 8);
    assert_eq!((state.row, state.column), (0, 4));
    state.move_counted(Motion::WordEnd, Some(2), &buffer, 8);
    assert_eq!((state.row, state.column), (0, 12));
}

#[test]
fn word_navigation_preserves_edit_history_register_and_updates_layout() {
    let mut editor = Editor::default();
    editor
        .insert_lines(0, ["one two three", "x", "long enough line"])
        .unwrap();
    let mut state = VisualState::default();
    state.yank(&mut editor, None, None).unwrap();
    let register = editor.unnamed_register().cloned();
    let text = editor.buffer().to_text();
    let dot = editor.current();
    let modified = editor.is_modified();
    state.move_counted(Motion::WordForward, Some(2), editor.buffer(), 3);
    state.move_cursor(Motion::Down, editor.buffer(), 3);
    assert_eq!((state.row, state.column), (1, 0));
    state.move_cursor(Motion::Down, editor.buffer(), 3);
    assert_eq!((state.row, state.column), (2, 8));
    let frame = state.frame(editor.buffer(), "test", 8, 3);
    assert!(frame.cursor.is_some());
    assert!(state.top > 0 && state.left > 0);
    assert_eq!(editor.buffer().to_text(), text);
    assert_eq!(editor.current(), dot);
    assert_eq!(editor.is_modified(), modified);
    assert_eq!(editor.unnamed_register(), register.as_ref());
    editor.undo().unwrap();
    assert!(editor.buffer().is_empty());
}

#[test]
fn big_words_group_punctuation_and_unicode_until_whitespace() {
    let text = "ab_2中文..🙂!\txy\u{2003}z";
    walk(
        text,
        (0, 0),
        Motion::BigWordForward,
        &[(0, 11), (0, 14), (0, 14)],
    );
    walk(
        text,
        (0, 14),
        Motion::BigWordBackward,
        &[(0, 11), (0, 0), (0, 0)],
    );
    walk(
        text,
        (0, 0),
        Motion::BigWordEnd,
        &[(0, 9), (0, 12), (0, 14), (0, 14)],
    );
    walk(text, (0, 8), Motion::BigWordBackward, &[(0, 0)]);
    walk("a..b c!d", (0, 3), Motion::BigWordEnd, &[(0, 7)]);
    walk("a..b c!d", (0, 1), Motion::BigWordForward, &[(0, 5)]);
    let buffer = TextBuffer::from_text(text).unwrap();
    let mut state = VisualState::default();
    state.move_counted(Motion::BigWordForward, Some(2), &buffer, 8);
    assert_eq!((state.row, state.column), (0, 14));
    state.move_counted(Motion::BigWordBackward, Some(2), &buffer, 8);
    assert_eq!((state.row, state.column), (0, 0));
    state.move_counted(Motion::BigWordEnd, Some(2), &buffer, 8);
    assert_eq!((state.row, state.column), (0, 12));
}

#[test]
fn big_words_cross_lines_preserve_history_and_desired_column() {
    let text = "a.b\n\n \t\nc!d e?f\n\nlast";
    walk(
        text,
        (0, 0),
        Motion::BigWordForward,
        &[(1, 0), (3, 0), (3, 4), (4, 0), (5, 0)],
    );
    walk(
        text,
        (5, 0),
        Motion::BigWordBackward,
        &[(4, 0), (3, 4), (3, 0), (1, 0), (0, 0)],
    );
    walk(
        text,
        (0, 0),
        Motion::BigWordEnd,
        &[(0, 2), (3, 2), (3, 6), (5, 3)],
    );
    let mut editor = Editor::from_buffer(TextBuffer::from_text("a.b c!d\nx\nlong enough").unwrap());
    let mut state = VisualState::default();
    state.yank(&mut editor, None, None).unwrap();
    let register = editor.unnamed_register().cloned();
    let dot = editor.current();
    state.move_cursor(Motion::BigWordForward, editor.buffer(), 3);
    assert!(!editor.is_modified());
    assert_eq!(editor.current(), dot);
    state.replace_character(&mut editor, 'X').unwrap();
    let dot = editor.current();
    state.move_cursor(Motion::BigWordEnd, editor.buffer(), 3);
    state.move_cursor(Motion::BigWordBackward, editor.buffer(), 3);
    state.move_counted(Motion::Down, Some(2), editor.buffer(), 3);
    assert_eq!((state.row, state.column), (2, 4));
    assert!(state.frame(editor.buffer(), "test", 5, 3).cursor.is_some());
    assert!(state.top > 0 && state.left > 0);
    assert_eq!(editor.current(), dot);
    assert_eq!(editor.unnamed_register(), register.as_ref());
    assert_eq!(editor.buffer().to_text(), "a.b X!d\nx\nlong enough");
    editor.undo().unwrap();
    assert_eq!(editor.buffer().to_text(), "a.b c!d\nx\nlong enough");
}
