use vir::{
    buffer::TextBuffer,
    editor::{Editor, Register},
    visual::VisualState,
};

#[test]
#[ignore = "run through tests/put_budget_pty.py with an address-space limit"]
fn oversized_source_is_rejected_before_copying() {
    let mode = std::env::var("VIR_PUT_PROBE").expect("controlled subprocess required");
    let mut editor = Editor::from_buffer(TextBuffer::from_text("abc\n").unwrap());
    let mut state = VisualState::default();
    editor
        .set_named_register('a', Register::Characters("X".into()), false)
        .unwrap();
    state
        .put_selected(&mut editor, false, 1, Some('a'))
        .unwrap();
    let text = editor.buffer().to_text();
    let view = format!("{state:?}");
    let source = "x".repeat(72 * 1024 * 1024);
    let register = if mode.contains("lines") {
        Register::Lines(vec![source])
    } else {
        Register::Characters(source)
    };
    editor.set_named_register('a', register, false).unwrap();
    let result = if mode.starts_with("dot") {
        state.repeat_change(&mut editor, None)
    } else {
        state.put_selected(&mut editor, false, 1, Some('a'))
    };
    assert_eq!(result, Err("paste too large".into()));
    assert_eq!(editor.buffer().to_text(), text);
    assert_eq!(format!("{state:?}"), view);
    let size = match editor.named_register('a').unwrap() {
        Register::Characters(text) => text.len(),
        Register::Lines(lines) => lines[0].len(),
    };
    assert_eq!(size, 72 * 1024 * 1024);
    editor.undo().unwrap();
    assert_eq!(editor.buffer().to_text(), "abc\n");
}
