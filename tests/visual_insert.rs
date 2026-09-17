use vir::{buffer::TextBuffer, editor::Editor, visual::VisualState};

#[test]
fn insert_append_open_and_undo_are_whole_sessions() {
    let mut editor = Editor::from_buffer(TextBuffer::from_text("中x\nlast\n").unwrap());
    editor.set_current(0).unwrap();
    let mut state = VisualState::default();
    let mut edit = state.begin_insert(&mut editor, 'a').unwrap();
    edit.insert("🙂").unwrap();
    edit.insert("!").unwrap();
    state.finish_insert(edit.commit().unwrap());
    assert_eq!(editor.buffer().line(0), Some("中🙂!x"));
    assert_eq!(state.column, 2);
    editor.undo().unwrap();
    assert_eq!(editor.buffer().line(0), Some("中x"));
    state.column = 0;
    let mut edit = state.begin_insert(&mut editor, 'i').unwrap();
    edit.insert("before").unwrap();
    state.finish_insert(edit.commit().unwrap());
    assert_eq!(editor.buffer().line(0), Some("before中x"));
    let mut edit = state.begin_insert(&mut editor, 'o').unwrap();
    edit.insert("new").unwrap();
    state.finish_insert(edit.commit().unwrap());
    assert_eq!(editor.buffer().to_text(), "before中x\nnew\nlast\n");
    assert_eq!((state.row, state.column), (1, 2));
    editor.undo().unwrap();
    assert_eq!(editor.buffer().to_text(), "before中x\nlast\n");
}

#[test]
fn empty_open_and_character_delete_do_not_add_or_join_extra_lines() {
    for mode in ['i', 'a', 'o'] {
        let mut editor = Editor::default();
        let mut state = VisualState::default();
        let mut edit = state.begin_insert(&mut editor, mode).unwrap();
        edit.insert("中").unwrap();
        state.finish_insert(edit.commit().unwrap());
        assert_eq!(editor.buffer().to_text(), "中\n");
        state.delete_character(&mut editor).unwrap();
        assert_eq!(editor.buffer().to_text(), "\n");
        let before = editor.clone();
        state.delete_character(&mut editor).unwrap();
        assert_eq!(editor, before);
        editor.undo().unwrap();
        assert_eq!(editor.buffer().to_text(), "中\n");
    }
    let mut editor = Editor::from_buffer(TextBuffer::from_text("\nnext\n").unwrap());
    VisualState::default()
        .delete_character(&mut editor)
        .unwrap();
    assert_eq!(editor.buffer().to_text(), "\nnext\n");
    assert!(!editor.is_modified());
}

#[test]
fn insert_layout_retains_gap_and_scrolls_through_resize() {
    let buffer = TextBuffer::from_text("中\tlong text\n").unwrap();
    let mut state = VisualState::default();
    state.column = buffer.line(0).unwrap().chars().count();
    let expected = state.column;
    for (width, height) in [(5, 3), (0, 0), (1, 1), (2, 2), (80, 24)] {
        let frame = state.insert_frame(&buffer, "file", width, height);
        assert_eq!(state.column, expected);
        if let Some((x, y)) = frame.cursor {
            assert!(x < width - 1 && y < height - 1);
        }
    }
    assert!(
        state
            .insert_frame(&buffer, "file", 80, 24)
            .status
            .starts_with("[INSERT]")
    );
    state.frame(&buffer, "file", 80, 24);
    assert_eq!(state.column, expected - 1);
}

#[test]
fn uppercase_insert_and_counted_dd_follow_normal_mode_rules() {
    let mut editor = Editor::from_buffer(TextBuffer::from_text("one\ntwo\nthree\nfour\n").unwrap());
    let mut state = VisualState::default();
    editor.set_current(1).unwrap();
    state.row = 1;
    let mut edit = state.begin_insert(&mut editor, 'I').unwrap();
    edit.insert("!").unwrap();
    state.finish_insert(edit.commit().unwrap());
    assert_eq!(editor.buffer().line(1), Some("!two"));
    let mut edit = state.begin_insert(&mut editor, 'A').unwrap();
    edit.insert("!").unwrap();
    state.finish_insert(edit.commit().unwrap());
    assert_eq!(editor.buffer().line(1), Some("!two!"));
    let mut edit = state.begin_insert(&mut editor, 'O').unwrap();
    edit.insert("above").unwrap();
    state.finish_insert(edit.commit().unwrap());
    assert_eq!(editor.buffer().line(1), Some("above"));
    state.delete_lines(&mut editor, 2).unwrap();
    assert_eq!(editor.buffer().to_text(), "one\nthree\nfour\n");
    editor.undo().unwrap();
    assert_eq!(editor.buffer().line(1), Some("above"));
}

#[test]
fn indented_i_and_large_delete_counts_handle_boundaries() {
    let mut editor = Editor::from_buffer(TextBuffer::from_text(" \t中\nlast\n").unwrap());
    let mut state = VisualState::default();
    let mut edit = state.begin_insert(&mut editor, 'I').unwrap();
    edit.insert("!").unwrap();
    state.finish_insert(edit.commit().unwrap());
    assert_eq!(editor.buffer().line(0), Some(" \t!中"));
    state.row = 1;
    state.delete_lines(&mut editor, usize::MAX).unwrap();
    assert_eq!(editor.buffer().line_count(), 1);
    state.delete_lines(&mut editor, usize::MAX).unwrap();
    assert!(editor.buffer().is_empty());
    let before = editor.clone();
    assert!(state.delete_lines(&mut editor, 1).is_err());
    assert_eq!(editor, before);
    for mode in ['I', 'A', 'O'] {
        let mut editor = Editor::default();
        let mut edit = state.begin_insert(&mut editor, mode).unwrap();
        edit.insert("text").unwrap();
        edit.commit().unwrap();
        assert_eq!(editor.buffer().to_text(), "text\n");
    }
}
