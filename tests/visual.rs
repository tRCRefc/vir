use unicode_width::UnicodeWidthStr;
use vir::{
    buffer::TextBuffer,
    visual::{Motion, VisualState, clip},
};

#[test]
fn vertical_motion_retains_desired_column_across_short_lines() {
    let buffer = TextBuffer::from_text("abcd\nx\nabcdef\n").unwrap();
    let mut state = VisualState::default();
    state.move_cursor(Motion::Right, &buffer, 4);
    state.move_cursor(Motion::Right, &buffer, 4);
    state.move_cursor(Motion::Down, &buffer, 4);
    assert_eq!((state.row, state.column), (1, 0));
    state.move_cursor(Motion::Down, &buffer, 4);
    assert_eq!((state.row, state.column), (2, 2));
    state.move_cursor(Motion::End, &buffer, 4);
    state.move_cursor(Motion::Right, &buffer, 4);
    assert_eq!(state.column, 5);
    state.move_cursor(Motion::Home, &buffer, 4);
    state.move_cursor(Motion::Left, &buffer, 4);
    assert_eq!(state.column, 0);
}

#[test]
fn scrolling_and_resize_keep_cursor_in_text_area() {
    let buffer = TextBuffer::from_text("one\ntwo\nthree\nfour\nfive\n").unwrap();
    let mut state = VisualState::default();
    state.move_cursor(Motion::Last, &buffer, 3);
    let frame = state.frame(&buffer, "file", 20, 3);
    assert_eq!(state.top, 3);
    assert_eq!(frame.lines, ["four", "five"]);
    assert_eq!(frame.cursor, Some((0, 1)));
    for (width, height) in [(0, 0), (1, 1), (2, 2), (80, 24)] {
        let frame = state.frame(&buffer, "file", width, height);
        if let Some((x, y)) = frame.cursor {
            assert!(x < width - 1 && y < height - 1);
        }
    }
    state.move_cursor(Motion::PageUp, &buffer, 3);
    assert_eq!(state.row, 2);
    state.move_cursor(Motion::First, &buffer, 3);
    assert_eq!(state.frame(&buffer, "file", 20, 3).cursor, Some((0, 0)));
}

#[test]
fn rendering_controls_tabs_unicode_and_clipping_is_safe() {
    assert_eq!(clip("a\tb", 0, 10), "a       b");
    assert_eq!(clip("\x1b[31m\x7f", 0, 20), "^[[31m^?");
    assert_eq!(clip("中x", 1, 3), " x");
    assert_eq!(clip("中", 0, 1), " ");
    assert_eq!(clip("e\u{301}", 0, 10), "e�");
    let buffer = TextBuffer::from_text("中\ttext\n").unwrap();
    let mut state = VisualState::default();
    for _ in 0..6 {
        let frame = state.frame(&buffer, "\x1bmalicious", 5, 3);
        assert!(frame.lines.iter().all(|line| line.width() <= 4));
        assert!(!frame.status.contains('\x1b'));
        assert!(frame.cursor.unwrap().0 < 4);
        state.move_cursor(Motion::Right, &buffer, 3);
    }
    assert!(state.left > 0);
}

#[test]
fn empty_buffer_stays_valid_and_browsing_does_not_mutate_text() {
    let buffer = TextBuffer::default();
    let mut state = VisualState::default();
    for motion in [Motion::Left, Motion::Right, Motion::Last, Motion::PageDown] {
        state.move_cursor(motion, &buffer, 3);
    }
    let frame = state.frame(&buffer, "empty", 80, 3);
    assert_eq!(frame.lines, ["~", "~"]);
    assert_eq!(frame.cursor, Some((0, 0)));
    assert!(frame.status.contains("0/0"));
    assert_eq!(buffer, TextBuffer::default());
}
