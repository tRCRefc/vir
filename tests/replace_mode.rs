use vir::{buffer::TextBuffer, editor::Editor, visual::VisualState};

#[test]
fn replace_unicode_append_split_and_backtrack_in_one_transaction() {
    let mut editor = Editor::from_buffer(TextBuffer::from_text("a中z\nlast").unwrap());
    let mut state = VisualState::default();
    state.column = 1;
    let mut edit = state.begin_insert(&mut editor, 'R').unwrap();
    for c in "🙂XY".chars() {
        edit.replace_input(c).unwrap();
    }
    assert_eq!(edit.buffer().to_text(), "a🙂XY\nlast");
    edit.replace_backspace().unwrap();
    edit.replace_backspace().unwrap();
    assert_eq!(edit.buffer().to_text(), "a🙂z\nlast");
    edit.replace_input('\n').unwrap();
    edit.replace_input('Q').unwrap();
    assert_eq!(edit.buffer().to_text(), "a🙂\nQ\nlast");
    edit.replace_backspace().unwrap();
    edit.replace_backspace().unwrap();
    assert_eq!(edit.buffer().to_text(), "a🙂z\nlast");
    state.finish_insert(edit.commit().unwrap());
    assert_eq!(state.column, 1);
    editor.undo().unwrap();
    assert_eq!(editor.buffer().to_text(), "a中z\nlast");
    editor.undo().unwrap();
    assert_eq!(editor.buffer().to_text(), "a🙂z\nlast");
}

#[test]
fn replace_backtrack_to_entry_preserves_empty_state_and_old_undo() {
    for text in ["", "\n", "中\n", "中"] {
        let mut editor = Editor::from_buffer(TextBuffer::from_text(text).unwrap());
        let before = editor.clone();
        let state = VisualState::default();
        let mut edit = state.begin_insert(&mut editor, 'R').unwrap();
        edit.replace_backspace().unwrap();
        for c in "x\n\t".chars() {
            edit.replace_input(c).unwrap();
        }
        for _ in 0..4 {
            edit.replace_backspace().unwrap();
        }
        edit.commit().unwrap();
        assert_eq!(editor, before);
    }
    let mut editor = Editor::default();
    editor.insert_lines(0, ["abc"]).unwrap();
    let before = editor.clone();
    let mut edit = VisualState::default()
        .begin_insert(&mut editor, 'R')
        .unwrap();
    edit.replace_input('a').unwrap();
    assert!(edit.replace_input('\r').is_err());
    edit.commit().unwrap();
    assert_eq!(editor, before);
    editor.undo().unwrap();
    assert!(editor.buffer().is_empty());
}

#[test]
fn dropped_replace_draft_is_cancelled() {
    let mut editor = Editor::from_buffer(TextBuffer::from_text("original").unwrap());
    let before = editor.clone();
    {
        let mut edit = VisualState::default()
            .begin_insert(&mut editor, 'R')
            .unwrap();
        edit.replace_input('X').unwrap();
    }
    assert_eq!(editor, before);
}

#[test]
fn replace_layout_clips_mode_label_in_narrow_windows() {
    let buffer = TextBuffer::from_text("中\tlong").unwrap();
    let mut state = VisualState::default();
    for width in 0..16 {
        state.column = 6;
        let frame = state.replace_frame(&buffer, "file", width, 4);
        assert!(frame.status.chars().count() <= usize::from(width.saturating_sub(1)));
        if width >= 10 {
            assert!(frame.status.starts_with("[REPLACE]"));
        }
        if let Some((x, y)) = frame.cursor {
            assert!(x < width - 1 && y < 3);
        }
    }
}
