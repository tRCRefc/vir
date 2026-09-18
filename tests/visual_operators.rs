use vir::{
    buffer::TextBuffer,
    editor::Editor,
    visual::{Motion, VisualState},
};

#[test]
fn character_delete_ranges_are_unicode_aware_and_one_undo() {
    for (motion, count, expected) in [
        (Motion::Left, Some(2), "🙂z\nnext\nlast"),
        (Motion::Right, Some(2), "a中\nnext\nlast"),
        (Motion::Home, None, "🙂z\nnext\nlast"),
        (Motion::End, None, "a中\nnext\nlast"),
        (Motion::End, Some(2), "a中\nlast"),
    ] {
        let text = "a中🙂z\nnext\nlast";
        let mut editor = Editor::from_buffer(TextBuffer::from_text(text).unwrap());
        editor.set_current(0).unwrap();
        let mut state = VisualState::default();
        state.column = 2;
        state.delete_motion(&mut editor, motion, count).unwrap();
        assert_eq!(editor.buffer().to_text(), expected);
        editor.undo().unwrap();
        assert_eq!(editor.buffer().to_text(), text);
        editor.undo().unwrap();
        assert_eq!(editor.buffer().to_text(), expected);
    }
}

#[test]
fn vertical_delete_includes_both_lines_and_g_distinguishes_default_count() {
    for (motion, count, expected) in [
        (Motion::Up, None, "three\nfour\n"),
        (Motion::Down, None, "one\nfour\n"),
        (Motion::Last, None, "one\n"),
        (Motion::Last, Some(1), "three\nfour\n"),
        (Motion::Last, Some(2), "one\nthree\nfour\n"),
    ] {
        let mut editor =
            Editor::from_buffer(TextBuffer::from_text("one\ntwo\nthree\nfour\n").unwrap());
        editor.set_current(1).unwrap();
        let mut state = VisualState::default();
        state.row = 1;
        state.delete_motion(&mut editor, motion, count).unwrap();
        assert_eq!(editor.buffer().to_text(), expected);
        assert_eq!(state.column, 0);
        assert_eq!(editor.current(), Some(state.row));
    }
}

#[test]
fn invalid_delete_does_not_change_editor_cursor_or_history() {
    let mut editor = Editor::default();
    editor.insert_lines(0, ["abc", "next"]).unwrap();
    for (motion, count) in [
        (Motion::Left, None),
        (Motion::Home, None),
        (Motion::Up, None),
        (Motion::Down, Some(usize::MAX)),
        (Motion::End, Some(usize::MAX)),
        (Motion::Last, Some(99)),
        (Motion::First, None),
    ] {
        let mut state = VisualState::default();
        let before = editor.clone();
        let visual = format!("{state:?}");
        assert!(state.delete_motion(&mut editor, motion, count).is_err());
        assert_eq!(editor, before);
        assert_eq!(format!("{state:?}"), visual);
    }
    editor.undo().unwrap();
    assert!(editor.buffer().is_empty());
    assert!(
        VisualState::default()
            .delete_motion(&mut editor, Motion::Last, None)
            .is_err()
    );
}

#[test]
fn huge_horizontal_delete_clamps_and_delete_all_is_reversible() {
    let mut editor = Editor::from_buffer(TextBuffer::from_text("中abc").unwrap());
    let mut state = VisualState::default();
    state
        .delete_motion(&mut editor, Motion::Right, Some(usize::MAX))
        .unwrap();
    assert_eq!(editor.buffer().to_text(), "");
    assert_eq!(editor.buffer().line_count(), 1);
    editor.undo().unwrap();
    state
        .delete_motion(&mut editor, Motion::Last, None)
        .unwrap();
    assert!(editor.buffer().is_empty());
    editor.undo().unwrap();
    assert_eq!(editor.buffer().to_text(), "中abc");
}
