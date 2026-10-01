use vir::{
    buffer::TextBuffer,
    editor::Editor,
    visual::{Motion, VisualState},
};

fn session(text: &str) -> (Editor, VisualState) {
    let mut editor = Editor::from_buffer(TextBuffer::from_text(text).unwrap());
    if !editor.buffer().is_empty() {
        editor.set_current(0).unwrap();
    }
    (editor, VisualState::default())
}

fn redraw(editor: &mut Editor, state: &mut VisualState) {
    let frame = state.frame(editor.buffer(), "file", 8, 3);
    let (x, y) = frame.cursor.unwrap();
    assert!(x < 7 && y < 2);
    assert_eq!(state.row, editor.current().unwrap_or(0));
    if !editor.buffer().is_empty() {
        editor.set_current(state.row).unwrap();
    }
}

#[test]
fn delete_to_eof_empty_buffer_and_undo_keep_dot_and_viewport_valid() {
    let (mut editor, mut state) = session("first\nsecond\nthird\nlast long line\n");
    state.move_cursor(Motion::Last, editor.buffer(), 3);
    state.move_cursor(Motion::End, editor.buffer(), 3);
    state.frame(editor.buffer(), "file", 8, 3);
    editor.set_current(state.row).unwrap();
    state.execute_ex("2,$d", &mut editor).unwrap();
    redraw(&mut editor, &mut state);
    assert_eq!(
        (state.row, state.column, state.left, state.top),
        (0, 0, 0, 0)
    );
    assert_eq!(editor.buffer().line_count(), 1);
    state.execute_ex("u", &mut editor).unwrap();
    redraw(&mut editor, &mut state);
    assert_eq!(state.row, 3);
    state.execute_ex("%d", &mut editor).unwrap();
    redraw(&mut editor, &mut state);
    assert!(editor.buffer().is_empty());
    assert!(editor.is_modified());
    assert!(state.execute_ex("q", &mut editor).is_err());
    state.execute_ex("u", &mut editor).unwrap();
    redraw(&mut editor, &mut state);
    assert_eq!(editor.buffer().line_count(), 4);
}

#[test]
fn substitution_resets_column_and_preferred_column_and_shares_history() {
    let (mut editor, mut state) = session("long value\nanother long value\n");
    state.move_cursor(Motion::End, editor.buffer(), 3);
    state.execute_ex("s/.*/中/", &mut editor).unwrap();
    redraw(&mut editor, &mut state);
    assert_eq!((state.row, state.column), (0, 0));
    state.move_cursor(Motion::Down, editor.buffer(), 3);
    assert_eq!(state.column, 0);
    assert_eq!(editor.last_search(), Some(".*"));
    state.execute_ex("u", &mut editor).unwrap();
    assert_eq!(editor.buffer().line(0), Some("long value"));
}

#[test]
fn errors_and_unsupported_multiline_commands_preserve_both_states() {
    let (mut editor, mut state) = session("first\nlast\n");
    state.execute_ex("d", &mut editor).unwrap();
    let before = editor.clone();
    let visual = format!("{state:?}");
    for text in [
        "999d",
        "s/missing/x/",
        "s/[/x/",
        "1u",
        "a",
        "i",
        "c",
        "p",
        "q",
        "e",
        "w",
    ] {
        assert!(state.execute_ex(text, &mut editor).is_err(), "{text}");
        assert_eq!(editor, before, "{text}");
        assert_eq!(format!("{state:?}"), visual, "{text}");
    }
    let (mut empty, mut empty_state) = session("");
    for text in ["d", "u", "s/a/b/"] {
        assert!(empty_state.execute_ex(text, &mut empty).is_err());
        assert_eq!(empty, Editor::default());
    }
}

#[test]
fn visual_set_autoindent_changes_following_input_sessions() {
    let (mut editor, mut state) = session("  base");
    state.execute_ex("set ai", &mut editor).unwrap();
    let mut input = state.begin_counted_insert(&mut editor, 'A', 1).unwrap();
    input
        .apply_input(&vir::editor::InputAction::Insert("X\nY".into()))
        .unwrap();
    input.commit(&mut state).unwrap();
    assert_eq!(editor.buffer().to_text(), "  baseX\n  Y");
    state.execute_ex("set noai", &mut editor).unwrap();
    editor.undo().unwrap();
    state.row = 0;
    state.column = 6;
    let mut input = state.begin_counted_insert(&mut editor, 'A', 1).unwrap();
    input
        .apply_input(&vir::editor::InputAction::Insert("X\nY".into()))
        .unwrap();
    input.commit(&mut state).unwrap();
    assert_eq!(editor.buffer().to_text(), "  baseX\nY");
}

#[test]
fn ex_register_bridge_supports_zero_put_undo_and_empty_buffers() {
    let (mut editor, mut state) = session("one\ntwo\n");
    state.execute_ex("2y a", &mut editor).unwrap();
    assert_eq!(editor.buffer().to_text(), "one\ntwo\n");
    assert!(!editor.is_modified());
    redraw(&mut editor, &mut state);
    assert_eq!(state.row, 1);
    state.execute_ex("0put a", &mut editor).unwrap();
    assert_eq!(editor.buffer().to_text(), "two\none\ntwo\n");
    redraw(&mut editor, &mut state);
    assert_eq!(state.row, 0);
    state.execute_ex("u", &mut editor).unwrap();
    assert_eq!(editor.buffer().to_text(), "one\ntwo\n");
    state.execute_ex("%d", &mut editor).unwrap();
    state.execute_ex("put a", &mut editor).unwrap();
    assert_eq!(editor.buffer().to_text(), "two\n");
    redraw(&mut editor, &mut state);
    let before = editor.clone();
    assert!(state.execute_ex("put z", &mut editor).is_err());
    assert_eq!(editor, before);
}

#[test]
fn file_commands_preserve_failures_and_sync_loads_names_and_saves() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("中文 file");
    std::fs::write(&path, "loaded\nlast\n").unwrap();
    let (mut editor, mut state) = session("original\n");
    state.execute_ex("d", &mut editor).unwrap();
    let before = editor.clone();
    let visual = format!("{state:?}");
    for text in [
        format!("e {}", path.display()),
        format!("e! {}", directory.path().display()),
    ] {
        assert!(state.execute_ex(&text, &mut editor).is_err());
        assert_eq!(editor, before);
        assert_eq!(format!("{state:?}"), visual);
    }
    state
        .execute_ex(&format!("e! {}", path.display()), &mut editor)
        .unwrap();
    redraw(&mut editor, &mut state);
    assert_eq!(state.row, 1);
    assert!(!editor.is_modified());
    assert!(state.execute_ex("u", &mut editor).is_err());
    let renamed = directory.path().join("renamed");
    let output = state
        .execute_ex(&format!("f {}", renamed.display()), &mut editor)
        .unwrap()
        .output;
    assert!(output.contains("renamed"));
    assert_eq!(editor.path(), Some(renamed.as_path()));
    state.execute_ex("s/last/changed/", &mut editor).unwrap();
    assert!(editor.is_modified());
    assert_eq!(
        state.execute_ex("w", &mut editor).unwrap().output,
        "Written"
    );
    assert!(!editor.is_modified());
    assert_eq!(
        std::fs::read_to_string(&renamed).unwrap(),
        "loaded\nchanged\n"
    );
    state.execute_ex("u", &mut editor).unwrap();
    assert!(editor.is_modified());
    assert!(state.execute_ex("q!", &mut editor).unwrap().quit);
    state
        .execute_ex(
            &format!("e! {}", directory.path().join("missing").display()),
            &mut editor,
        )
        .unwrap();
    redraw(&mut editor, &mut state);
    assert!(editor.buffer().is_empty());
}
