use std::io::Cursor;

use vir::buffer::TextBuffer;
use vir::editor::Editor;
use vir::ex::command::{execute, execute_with_lines};
use vir::ex::parser::parse;

#[test]
fn append_accepts_zero_and_ranges_use_the_last_address() {
    for (command, expected) in [
        ("0a", "new\none\ntwo\n"),
        ("1-1a", "new\none\ntwo\n"),
        ("0,1a", "one\nnew\ntwo\n"),
        ("1,2a", "one\ntwo\nnew\n"),
        ("1,2i", "one\nnew\ntwo\n"),
        ("%i", "one\nnew\ntwo\n"),
    ] {
        let mut editor = Editor::from_buffer(TextBuffer::from_text("one\ntwo\n").unwrap());
        let before = editor.clone();
        execute_with_lines(parse(command).unwrap(), &mut editor, &["new".into()]).unwrap();
        assert_eq!(editor.buffer().to_text(), expected, "{command}");
        editor.undo().unwrap();
        assert_eq!(editor.buffer(), before.buffer(), "{command}");
        assert_eq!(editor.current(), before.current(), "{command}");
        assert!(editor.is_modified());
        editor.undo().unwrap();
        assert_eq!(editor.buffer().to_text(), expected, "{command}");
    }
}

#[test]
fn empty_insertion_accepts_only_addresses_resolving_to_zero() {
    for command in ["a", "0a", ".a", "$a", "0,0a", "1-1a", "i", "0i", ".i", "$i"] {
        let mut editor = Editor::default();
        execute_with_lines(parse(command).unwrap(), &mut editor, &["new".into()]).unwrap();
        assert_eq!(editor.buffer().to_text(), "new\n", "{command}");
        assert_eq!(editor.current(), Some(0));
    }
    for command in ["999a", "1i", "%a", "%i", "0,1a", ".-1a", "$+1i", "1,0a"] {
        let mut editor = Editor::default();
        assert!(
            execute_with_lines(parse(command).unwrap(), &mut editor, &["new".into()]).is_err(),
            "{command}"
        );
        assert_eq!(editor, Editor::default(), "{command}");
    }
}

#[test]
fn invalid_addresses_preserve_text_dot_modified_and_history() {
    let mut editor = Editor::from_buffer(TextBuffer::from_text("one\ntwo\n").unwrap());
    editor.insert_lines(2, ["three"]).unwrap();
    let before = editor.clone();
    for command in [
        "4a", "4i", "4c", "4d", "0i", "0c", "0d", "3,1a", "3,1i", "3,1c", "0,1i", "1u", "%u",
        "1q!", "1next",
    ] {
        assert!(
            execute_with_lines(parse(command).unwrap(), &mut editor, &["new".into()]).is_err(),
            "{command}"
        );
        assert_eq!(editor, before, "{command}");
    }
    execute(parse("u").unwrap(), &mut editor).unwrap();
    assert_eq!(editor.buffer().to_text(), "one\ntwo\n");
}

#[test]
fn invalid_input_commands_do_not_consume_following_commands() {
    for command in ["999a", "999i", "999c", "2,1a", "0i"] {
        let mut editor = Editor::from_buffer(TextBuffer::from_text("one\ntwo\n").unwrap());
        let before = editor.clone();
        let mut input = Cursor::new(format!("{command}\n%p\nq!\n"));
        let mut output = Vec::new();
        vir::ex::repl::run(&mut input, &mut output, &mut editor).unwrap();
        let output = String::from_utf8(output).unwrap();
        assert!(output.starts_with("? "), "{command}: {output}");
        assert!(output.ends_with("one\ntwo\n"), "{command}: {output}");
        assert_eq!(editor, before);
    }
}

#[test]
fn empty_invalid_input_command_returns_to_command_mode() {
    let mut editor = Editor::default();
    let mut input = Cursor::new("999a\na\nkept\n.\n%p\nq!\n");
    let mut output = Vec::new();
    vir::ex::repl::run(&mut input, &mut output, &mut editor).unwrap();
    assert_eq!(output, b"? OutOfBounds\nkept\n");
    assert_eq!(editor.buffer().to_text(), "kept\n");
}
