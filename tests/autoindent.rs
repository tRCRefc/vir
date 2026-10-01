use vir::{
    buffer::{Position, TextBuffer},
    editor::{Editor, InputAction},
    ex::{command::execute, parser::parse},
};

#[test]
fn autoindent_is_opt_in_and_applies_to_typed_newlines() {
    let mut editor = Editor::from_buffer(TextBuffer::from_text("  base").unwrap());
    {
        let mut edit = editor
            .begin_text_edit(Position { row: 0, column: 6 })
            .unwrap();
        edit.apply_input(&InputAction::Insert("\nchild".into()))
            .unwrap();
        edit.commit().unwrap();
    }
    assert_eq!(editor.buffer().to_text(), "  base\nchild");

    execute(parse("set ai").unwrap(), &mut editor).unwrap();
    {
        let mut edit = editor
            .begin_text_edit(Position { row: 0, column: 6 })
            .unwrap();
        edit.apply_input(&InputAction::Insert("\nchild".into()))
            .unwrap();
        edit.commit().unwrap();
    }
    assert_eq!(editor.buffer().to_text(), "  base\n  child\nchild");
    execute(parse("set noai").unwrap(), &mut editor).unwrap();
    assert!(!editor.autoindent());
}

#[test]
fn autoindent_commands_reject_addresses_and_unknown_options() {
    let mut editor = Editor::default();
    assert!(execute(parse("1set ai").unwrap(), &mut editor).is_err());
    assert!(parse("set unknown").is_err());
}

#[test]
fn generated_indent_is_removed_when_open_input_is_empty() {
    let mut editor = Editor::from_buffer(TextBuffer::from_text("  base\n").unwrap());
    execute(parse("set ai").unwrap(), &mut editor).unwrap();
    let mut state = vir::visual::VisualState::default();
    let input = state.begin_counted_insert(&mut editor, 'o', 1).unwrap();
    assert_eq!(input.buffer().to_text(), "  base\n  \n");
    input.commit(&mut state).unwrap();
    assert_eq!(editor.buffer().to_text(), "  base\n\n");
}

#[test]
fn open_line_above_uses_original_line_indentation() {
    let mut editor = Editor::from_buffer(TextBuffer::from_text("  base\n    next\n").unwrap());
    execute(parse("set ai").unwrap(), &mut editor).unwrap();
    let mut state = vir::visual::VisualState::default();
    state.row = 1;
    let input = state.begin_counted_insert(&mut editor, 'O', 1).unwrap();
    assert_eq!(input.buffer().to_text(), "  base\n    \n    next\n");
    input.commit(&mut state).unwrap();
    assert_eq!(editor.buffer().to_text(), "  base\n\n    next\n");
}

#[test]
fn generated_indent_uses_tabs_for_each_full_tabstop() {
    let mut editor = Editor::from_buffer(TextBuffer::from_text("          base\n").unwrap());
    execute(parse("set ai").unwrap(), &mut editor).unwrap();
    let mut state = vir::visual::VisualState::default();
    let input = state.begin_counted_insert(&mut editor, 'o', 1).unwrap();
    assert_eq!(input.buffer().to_text(), "          base\n\t  \n");
    input.commit(&mut state).unwrap();
    assert_eq!(editor.buffer().to_text(), "          base\n\n");
}

#[test]
fn whole_line_change_keeps_the_deleted_line_indent() {
    let mut editor = Editor::from_buffer(TextBuffer::from_text("  base\n").unwrap());
    execute(parse("set ai").unwrap(), &mut editor).unwrap();
    let mut state = vir::visual::VisualState::default();
    let mut input = state.begin_change(&mut editor, None, Some(1)).unwrap();
    assert_eq!(input.buffer().to_text(), "  \n");
    input.apply_input(&InputAction::Insert("X".into())).unwrap();
    state
        .commit_input(
            input,
            vir::visual::InputStart::Change {
                motion: None,
                count: Some(1),
            },
        )
        .unwrap();
    assert_eq!(editor.buffer().to_text(), "  X\n");
}

#[test]
fn replace_newline_reindents_forwarded_tail() {
    let mut editor = Editor::from_buffer(TextBuffer::from_text("  ab   tail").unwrap());
    execute(parse("set ai").unwrap(), &mut editor).unwrap();
    let mut state = vir::visual::VisualState::default();
    state.column = 2;
    let mut input = state.begin_counted_insert(&mut editor, 'R', 1).unwrap();
    input
        .apply_input(&InputAction::Replace("\n".chars().next().unwrap()))
        .unwrap();
    input.commit(&mut state).unwrap();
    assert_eq!(editor.buffer().to_text(), "  \n  b   tail");
}

#[test]
fn low_level_text_insertion_is_literal_even_with_autoindent() {
    let mut editor = Editor::from_buffer(TextBuffer::from_text("  base").unwrap());
    execute(parse("set ai").unwrap(), &mut editor).unwrap();
    let mut edit = editor
        .begin_text_edit(Position { row: 0, column: 6 })
        .unwrap();
    edit.insert("\n\tchild\nnext").unwrap();
    edit.commit().unwrap();
    assert_eq!(editor.buffer().to_text(), "  base\n\tchild\nnext");
}
