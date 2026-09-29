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
