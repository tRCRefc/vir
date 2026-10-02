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
fn ex_register_arguments_select_named_and_numbered_slots() {
    assert_eq!(
        parse("1y a").unwrap().command,
        vir::ex::parser::Command::Yank {
            register: Some('a')
        }
    );
    assert_eq!(
        parse("put 1").unwrap().command,
        vir::ex::parser::Command::Put {
            register: Some('1')
        }
    );
    assert!(parse("y 1").is_err());
    assert!(parse("put 0").is_err());
}

#[test]
fn ex_named_yank_append_and_numbered_put_work() {
    let mut editor = Editor::from_buffer(TextBuffer::from_text("one\ntwo\nthree\n").unwrap());
    run(&mut editor, "1y a").unwrap();
    run(&mut editor, "2y A").unwrap();
    assert_eq!(
        editor.named_register('a'),
        Some(&Register::Lines(vec!["one".into(), "two".into()]))
    );
    run(&mut editor, "3d").unwrap();
    run(&mut editor, "1put 1").unwrap();
    assert_eq!(editor.buffer().to_text(), "one\nthree\ntwo\n");
}

#[test]
fn ex_put_failure_preserves_state_when_register_is_empty() {
    let mut editor = Editor::from_buffer(TextBuffer::from_text("one\ntwo\n").unwrap());
    let before = editor.clone();
    assert!(run(&mut editor, "1put").is_err());
    assert_eq!(editor, before);
}

#[test]
fn addressed_yank_preserves_current_line() {
    for command in ["1y", "1y a", "1y A"] {
        let mut editor = Editor::from_buffer(TextBuffer::from_text("one\ntwo\n").unwrap());
        editor.set_current(1).unwrap();
        run(&mut editor, command).unwrap();
        assert_eq!(editor.current(), Some(1), "{command}");
        assert!(!editor.is_modified());
    }
}

#[test]
fn ex_put_accepts_address_zero_and_empty_buffer() {
    for initial in ["", "tail\n"] {
        let mut editor = Editor::from_buffer(TextBuffer::from_text(initial).unwrap());
        editor
            .set_named_register('a', Register::Lines(vec!["saved".into()]), false)
            .unwrap();
        let parsed = parse("0put a").unwrap();
        vir::ex::command::validate_addresses(&parsed, &editor).unwrap();
        execute(parsed, &mut editor).unwrap();
        assert_eq!(editor.buffer().to_text(), format!("saved\n{initial}"));
        editor.undo().unwrap();
        assert_eq!(editor.buffer().to_text(), initial);
    }
    let mut editor = Editor::default();
    editor
        .set_named_register('a', Register::Characters("saved".into()), false)
        .unwrap();
    run(&mut editor, "put a").unwrap();
    assert_eq!(editor.buffer().to_text(), "saved\n");
    let before = editor.clone();
    assert!(run(&mut editor, "2put a").is_err());
    assert_eq!(editor, before);
}
