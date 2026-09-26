use vir::{
    buffer::TextBuffer,
    editor::{Editor, Register},
    ex::{command::execute, parser::parse},
};

fn run(editor: &mut Editor, command: &str) -> Result<(), String> {
    execute(parse(command).unwrap(), editor).map(|_| ())
}

#[test]
fn ex_yank_and_put_use_the_unnamed_register() {
    let mut editor = Editor::from_buffer(TextBuffer::from_text("one\ntwo\nthree\n").unwrap());
    run(&mut editor, "1,2y").unwrap();
    assert_eq!(
        editor.unnamed_register(),
        Some(&Register::Lines(vec!["one".into(), "two".into()]))
    );
    run(&mut editor, "3pu").unwrap();
    assert_eq!(editor.buffer().to_text(), "one\ntwo\nthree\none\ntwo\n");
}

#[test]
fn ex_put_failure_preserves_state_when_register_is_empty() {
    let mut editor = Editor::from_buffer(TextBuffer::from_text("one\ntwo\n").unwrap());
    let before = editor.clone();
    assert!(run(&mut editor, "1put").is_err());
    assert_eq!(editor, before);
}
