use vir::{
    buffer::TextBuffer,
    visual::{Motion, VisualState},
};

#[test]
fn counted_motions_preserve_desired_column_and_handle_unicode() {
    let buffer = TextBuffer::from_text("a中🙂z\nx\n\nabcdef\nlast").unwrap();
    let mut state = VisualState::default();
    state.move_counted(Motion::Right, Some(2), &buffer, 8);
    assert_eq!((state.row, state.column), (0, 2));
    state.move_counted(Motion::Down, Some(2), &buffer, 8);
    assert_eq!((state.row, state.column), (2, 0));
    state.move_cursor(Motion::Down, &buffer, 8);
    assert_eq!((state.row, state.column), (3, 2));
    state.move_counted(Motion::Up, Some(3), &buffer, 8);
    assert_eq!((state.row, state.column), (0, 2));
    state.move_counted(Motion::End, Some(4), &buffer, 8);
    assert_eq!((state.row, state.column), (3, 5));
    state.move_counted(Motion::Last, Some(2), &buffer, 8);
    assert_eq!((state.row, state.column), (1, 0));
    state.move_cursor(Motion::Last, &buffer, 8);
    assert_eq!(state.row, 4);
}

#[test]
fn huge_counts_clamp_without_iteration_or_overflow() {
    let buffer = TextBuffer::from_text("abc\n中\nlast").unwrap();
    let mut state = VisualState::default();
    for motion in [
        Motion::Right,
        Motion::Down,
        Motion::PageDown,
        Motion::End,
        Motion::Last,
        Motion::First,
    ] {
        state.move_counted(motion, Some(usize::MAX), &buffer, u16::MAX);
        assert!(state.row < buffer.line_count());
        assert!(state.column < buffer.line(state.row).unwrap().chars().count());
    }
    state.move_counted(Motion::PageUp, Some(usize::MAX), &buffer, 0);
    assert_eq!(state.row, 0);
    state.move_counted(Motion::Left, Some(usize::MAX), &buffer, 0);
    assert_eq!(state.column, 0);
    state.move_counted(Motion::Down, Some(usize::MAX), &TextBuffer::default(), 0);
    assert_eq!(state.row, 0);
}
