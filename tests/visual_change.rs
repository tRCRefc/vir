use vir::{
    buffer::TextBuffer,
    editor::Editor,
    visual::{Motion, VisualState},
};

#[test]
fn character_changes_and_typed_text_are_one_transaction() {
    for (motion, count, expected) in [
        (Motion::Left, None, "a文🙂z\nnext\nlast"),
        (Motion::Right, Some(2), "a中文\nnext\nlast"),
        (Motion::Home, None, "文🙂z\nnext\nlast"),
        (Motion::End, Some(2), "a中文\nlast"),
    ] {
        let original = "a中🙂z\nnext\nlast";
        let mut editor = Editor::from_buffer(TextBuffer::from_text(original).unwrap());
        let mut state = VisualState::default();
        state.column = 2;
        let mut edit = state
            .begin_change(&mut editor, Some(motion), count)
            .unwrap();
        edit.insert("文").unwrap();
        state.finish_insert(edit.commit().unwrap());
        assert_eq!(editor.buffer().to_text(), expected);
        editor.undo().unwrap();
        assert_eq!(editor.buffer().to_text(), original);
        editor.undo().unwrap();
        assert_eq!(editor.buffer().to_text(), expected);
    }
}

#[test]
fn line_changes_keep_one_input_line_and_preserve_tail() {
    for (motion, count, expected) in [
        (None, Some(2), "one\nNEW\nfour"),
        (Some(Motion::Down), None, "one\nNEW\nfour"),
        (Some(Motion::Up), None, "NEW\nthree\nfour"),
        (Some(Motion::Last), None, "one\nNEW"),
        (Some(Motion::Last), Some(1), "NEW\nthree\nfour"),
    ] {
        let original = "one\ntwo\nthree\nfour";
        let mut editor = Editor::from_buffer(TextBuffer::from_text(original).unwrap());
        let mut state = VisualState::default();
        state.row = 1;
        let mut edit = state.begin_change(&mut editor, motion, count).unwrap();
        edit.insert("NEW").unwrap();
        state.finish_insert(edit.commit().unwrap());
        assert_eq!(editor.buffer().to_text(), expected);
        assert_eq!(editor.current(), Some(state.row));
        editor.undo().unwrap();
        assert_eq!(editor.buffer().to_text(), original);
    }
}

#[test]
fn immediate_exit_empty_change_and_cancel_preserve_expected_state() {
    let mut editor = Editor::from_buffer(TextBuffer::from_text("one\ntwo\n").unwrap());
    let state = VisualState::default();
    let before = editor.clone();
    drop(state.begin_change(&mut editor, None, Some(2)).unwrap());
    assert_eq!(editor, before);
    assert!(
        state
            .begin_change(&mut editor, Some(Motion::Up), None)
            .is_err()
    );
    assert_eq!(editor, before);
    state
        .begin_change(&mut editor, None, Some(usize::MAX))
        .unwrap()
        .commit()
        .unwrap();
    assert_eq!(editor.buffer().to_text(), "\n");
    editor.undo().unwrap();
    assert_eq!(editor.buffer().to_text(), "one\ntwo\n");
    for motion in [None, Some(Motion::End)] {
        let mut empty = Editor::default();
        state
            .begin_change(&mut empty, motion, None)
            .unwrap()
            .commit()
            .unwrap();
        assert_eq!(empty, Editor::default());
        let mut edit = state.begin_change(&mut empty, motion, None).unwrap();
        edit.insert("中").unwrap();
        edit.commit().unwrap();
        assert_eq!(empty.buffer().to_text(), "中\n");
        empty.undo().unwrap();
        assert!(empty.buffer().is_empty());
    }
}
