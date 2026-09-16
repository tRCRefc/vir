use std::io::{self, BufReader, Cursor, Read};

use vir::buffer::TextBuffer;
use vir::editor::Editor;
use vir::ex::command::execute;
use vir::ex::parser::parse;

fn editor(text: &str) -> Editor {
    Editor::from_buffer(TextBuffer::from_text(text).unwrap())
}

fn run(editor: &mut Editor, script: &str) -> String {
    let mut output = Vec::new();
    vir::ex::repl::run(&mut Cursor::new(script), &mut output, editor).unwrap();
    String::from_utf8(output).unwrap()
}

#[test]
fn empty_append_and_insert_preserve_state_and_previous_undo() {
    for text in ["", "one\n", "one"] {
        for command in ["a", "i"] {
            for end in [".\nq!\n", "", "partial"] {
                let mut editor = editor(text);
                let before = editor.clone();
                assert_eq!(run(&mut editor, &format!("{command}\n{end}")), "");
                assert_eq!(editor, before);
            }
        }
    }
    let mut editor = editor("one\n");
    editor.insert_lines(1, ["two"]).unwrap();
    let before = editor.clone();
    assert_eq!(run(&mut editor, "1a\n.\n1i\n.\nq!\n"), "");
    assert_eq!(editor, before);
    editor.undo().unwrap();
    assert_eq!(editor.buffer().to_text(), "one\n");
}

#[test]
fn empty_change_deletes_range_in_one_reversible_edit() {
    for terminator in [".\n", "", "partial"] {
        let mut editor = editor("one\ntwo\nthree\n");
        assert_eq!(run(&mut editor, &format!("2,3c\n{terminator}")), "");
        assert_eq!(editor.buffer().to_text(), "one\n");
        assert_eq!(editor.current(), Some(0));
        editor.undo().unwrap();
        assert_eq!(editor.buffer().to_text(), "one\ntwo\nthree\n");
        editor.undo().unwrap();
        assert_eq!(editor.buffer().to_text(), "one\n");
    }
    let mut editor = editor("one\n");
    assert_eq!(run(&mut editor, "c\n.\n"), "");
    assert!(editor.buffer().is_empty());
    assert_eq!(editor.buffer().to_text(), "");
    assert!(!editor.buffer().has_trailing_newline());
    assert_eq!(editor.current(), None);
    editor.undo().unwrap();
    assert_eq!(editor.buffer().to_text(), "one\n");
}

#[test]
fn input_eof_commits_complete_lines_as_one_edit_and_discards_partial_line() {
    for command in ["a", "i", "c"] {
        for tail in ["", "discarded", "."] {
            let mut editor = editor("old\n");
            assert_eq!(
                run(&mut editor, &format!("{command}\nfirst\nsecond\n{tail}")),
                ""
            );
            let edited = editor.buffer().clone();
            assert!(edited.to_text().contains("first\nsecond\n"));
            assert!(!edited.to_text().contains("discarded"));
            editor.undo().unwrap();
            assert_eq!(editor.buffer().to_text(), "old\n");
            editor.undo().unwrap();
            assert_eq!(editor.buffer(), &edited);
        }
    }
}

#[test]
fn input_delimiters_preserve_blank_lines_and_literal_dots() {
    let mut editor = Editor::default();
    assert_eq!(run(&mut editor, "a\r\n\r\n..\r\n. \r\n.\r\nq!\r\n"), "");
    assert_eq!(editor.buffer().to_text(), "\n..\n. \n");
}

#[test]
fn invalid_input_rolls_back_entire_edit_and_keeps_undo() {
    for command in ["a", "i", "c"] {
        let mut editor = editor("old\n");
        editor.insert_lines(1, ["previous"]).unwrap();
        let before = editor.clone();
        let output = run(&mut editor, &format!("{command}\nvalid\nbad\r\r\n.\nq!\n"));
        assert!(output.starts_with("? "));
        assert_eq!(editor, before);
        editor.undo().unwrap();
        assert_eq!(editor.buffer().to_text(), "old\n");
    }
}

struct BrokenInput;
impl Read for BrokenInput {
    fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
        Err(io::Error::other("injected read failure"))
    }
}

#[test]
fn read_error_aborts_pending_edit_without_changing_state() {
    for command in ["a", "i", "c"] {
        let mut editor = editor("old\n");
        editor.insert_lines(1, ["previous"]).unwrap();
        let before = editor.clone();
        let prefix = Cursor::new(format!("{command}\ncomplete\npartial"));
        let mut input = BufReader::new(prefix.chain(BrokenInput));
        let error = vir::ex::repl::run(&mut input, &mut Vec::new(), &mut editor).unwrap_err();
        assert_eq!(error.to_string(), "injected read failure");
        assert_eq!(editor, before);
    }
}

#[test]
fn new_edit_replaces_toggle_history_and_failed_edit_preserves_it() {
    let mut editor = editor("one\n");
    assert!(editor.undo().is_err());
    editor.insert_lines(1, ["two"]).unwrap();
    editor.undo().unwrap();
    editor.insert_lines(1, ["new"]).unwrap();
    let before = editor.clone();
    assert!(editor.insert_lines(99, Vec::<String>::new()).is_err());
    assert!(editor.replace_lines(0, 0, ["invalid\nline"]).is_err());
    assert_eq!(editor, before);
    editor.undo().unwrap();
    assert_eq!(editor.buffer().to_text(), "one\n");
    editor.undo().unwrap();
    assert_eq!(editor.buffer().to_text(), "one\nnew\n");
}

#[test]
fn script_eof_returns_with_unsaved_state_but_quit_checks_it() {
    let mut editor = editor("one\n");
    assert_eq!(run(&mut editor, "a\ntwo\n"), "");
    let before = editor.clone();
    assert_eq!(run(&mut editor, ""), "");
    assert_eq!(editor, before);
    editor.undo().unwrap();
    assert!(editor.is_modified());
    assert!(execute(parse("q").unwrap(), &mut editor).is_err());
    assert!(execute(parse("q!").unwrap(), &mut editor).unwrap().quit);
}
