use vir::{
    buffer::{Position, TextBuffer},
    editor::Editor,
    visual::VisualState,
};

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
        assert!(state.delete_character(&mut editor).is_err());
        assert_eq!(editor, before);
        editor.undo().unwrap();
        assert_eq!(editor.buffer().to_text(), "中\n");
    }
    let mut editor = Editor::from_buffer(TextBuffer::from_text("\nnext\n").unwrap());
    assert!(
        VisualState::default()
            .delete_character(&mut editor)
            .is_err()
    );
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
fn delete_to_line_end_and_change_share_text_edit_semantics() {
    let mut editor = Editor::from_buffer(TextBuffer::from_text("ab中cd\nnext\n").unwrap());
    let mut edit = editor
        .begin_text_edit(Position { row: 0, column: 2 })
        .unwrap();
    edit.delete_to_line_end().unwrap();
    edit.insert("XY").unwrap();
    edit.commit().unwrap();
    assert_eq!(editor.buffer().to_text(), "abXY\nnext\n");
    editor.undo().unwrap();
    assert_eq!(editor.buffer().to_text(), "ab中cd\nnext\n");
}

#[test]
fn delete_to_line_end_at_end_is_a_noop() {
    let mut editor = Editor::from_buffer(TextBuffer::from_text("abc\n").unwrap());
    let mut edit = editor
        .begin_text_edit(Position { row: 0, column: 3 })
        .unwrap();
    edit.delete_to_line_end().unwrap();
    edit.commit().unwrap();
    assert_eq!(editor.buffer().to_text(), "abc\n");
    assert!(!editor.is_modified());
}

#[test]
fn replace_character_is_one_edit_and_rejects_line_end() {
    let mut editor = Editor::from_buffer(TextBuffer::from_text("a中c\n").unwrap());
    let mut state = VisualState::default();
    state.row = 0;
    state.column = 1;
    state.replace_character(&mut editor, '🙂').unwrap();
    assert_eq!(editor.buffer().to_text(), "a🙂c\n");
    editor.undo().unwrap();
    assert_eq!(editor.buffer().to_text(), "a中c\n");
    state.column = 3;
    assert!(state.replace_character(&mut editor, 'x').is_err());
    assert_eq!(editor.buffer().to_text(), "a中c\n");
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

#[test]
fn join_whitespace_cursor_and_final_newline_follow_ex_rules() {
    for (input, expected, column) in [
        ("中.\n \tnext", "中.  next", 2),
        ("word\n )tail\n", "word)tail\n", 4),
        ("word \n next\n", "word next\n", 5),
        ("\n  next\n", "next\n", 0),
        ("word\n \t\n", "word\n", 3),
        ("\n\n", "\n", 0),
    ] {
        let mut editor = Editor::from_buffer(TextBuffer::from_text(input).unwrap());
        let mut state = VisualState::default();
        state.join_lines(&mut editor, 2).unwrap();
        assert_eq!(editor.buffer().to_text(), expected);
        assert_eq!(state.column, column);
        editor.undo().unwrap();
        assert_eq!(editor.buffer().to_text(), input);
        editor.undo().unwrap();
        assert_eq!(editor.buffer().to_text(), expected);
    }
}

#[test]
fn counted_join_clamps_and_failure_preserves_history() {
    let mut editor = Editor::from_buffer(TextBuffer::from_text("one\ntwo\nthree").unwrap());
    let mut state = VisualState::default();
    state.join_lines(&mut editor, usize::MAX).unwrap();
    assert_eq!(editor.buffer().to_text(), "one two three");
    let before = editor.clone();
    assert!(state.join_lines(&mut editor, 2).is_err());
    assert_eq!(editor, before);
    editor.undo().unwrap();
    assert_eq!(editor.buffer().to_text(), "one\ntwo\nthree");
    let mut empty = Editor::default();
    assert!(VisualState::default().join_lines(&mut empty, 2).is_err());
    assert_eq!(empty, Editor::default());
}

#[test]
fn change_entry_immediate_exit_and_empty_buffer_preserve_transactions() {
    let mut editor = Editor::from_buffer(TextBuffer::from_text("中tail").unwrap());
    let mut state = VisualState::default();
    state.column = 1;
    let edit = state.begin_insert(&mut editor, 'C').unwrap();
    edit.commit().unwrap();
    assert_eq!(editor.buffer().to_text(), "中");
    editor.undo().unwrap();
    assert_eq!(editor.buffer().to_text(), "中tail");
    editor.undo().unwrap();
    assert_eq!(editor.buffer().to_text(), "中");
    let before = editor.clone();
    assert!(state.replace_character(&mut editor, '\n').is_err());
    assert_eq!(editor, before);
    let mut empty = Editor::default();
    VisualState::default()
        .begin_insert(&mut empty, 'C')
        .unwrap()
        .commit()
        .unwrap();
    assert_eq!(empty, Editor::default());
}
