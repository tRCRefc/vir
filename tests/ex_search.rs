use std::io::Cursor;
use vir::{
    buffer::TextBuffer,
    editor::Editor,
    ex::{command::execute, parser::parse},
};
fn editor(text: &str) -> Editor {
    Editor::from_buffer(TextBuffer::from_text(text).unwrap())
}
fn run(editor: &mut Editor, text: &str) -> Result<String, String> {
    execute(parse(text).unwrap(), editor).map(|result| result.output)
}

#[test]
fn searches_wrap_in_both_directions_and_repeat_across_directions() {
    let mut editor = editor("hit one\nmiddle\nhit two\n");
    assert_eq!(run(&mut editor, "/hit/").unwrap(), "hit one\n");
    assert_eq!(run(&mut editor, "//").unwrap(), "hit two\n");
    assert_eq!(run(&mut editor, "??").unwrap(), "hit one\n");
    assert_eq!(run(&mut editor, "?hit?").unwrap(), "hit two\n");
    assert!(!editor.is_modified());
    assert!(editor.undo().is_err());
    let mut single = self::editor("only\n");
    assert_eq!(run(&mut single, "/only/").unwrap(), "only\n");
}

#[test]
fn search_ranges_share_original_dot_and_support_offsets() {
    let mut editor = editor("start\nmatch\nend\nmatch\n");
    editor.set_current(0).unwrap();
    assert_eq!(run(&mut editor, "/match/,/end/p").unwrap(), "match\nend\n");
    assert_eq!(editor.last_search(), Some("end"));
    assert_eq!(run(&mut editor, "?match?-1p").unwrap(), "start\n");
    assert_eq!(run(&mut editor, "/match/+1").unwrap(), "end\n");
    editor.set_current(0).unwrap();
    assert_eq!(run(&mut editor, "/match/,//p").unwrap(), "match\n");
}

#[test]
fn unicode_regex_and_escaped_delimiters_work() {
    let mut editor = editor("中文123\na/b\nwhy?\n");
    assert_eq!(run(&mut editor, "/^中文[0-9]+$/").unwrap(), "中文123\n");
    assert_eq!(run(&mut editor, r"/a\/b/").unwrap(), "a/b\n");
    assert_eq!(run(&mut editor, r"?why\??").unwrap(), "why?\n");
    for input in [
        "/missing",
        "?missing",
        "/dangling\\",
        "/ok/+999999999999999999999999",
    ] {
        assert!(parse(input).is_err(), "{input}");
    }
}

#[test]
fn failed_search_or_command_preserves_editor_and_pattern() {
    let mut editor = editor("old\ntarget\n");
    run(&mut editor, "/old/").unwrap();
    editor.insert_lines(2, ["new"]).unwrap();
    let before = editor.clone();
    for input in [
        "/absent/",
        "/[/",
        r"/(old)\1/",
        "/target/+99",
        "/target/q!",
        "/new/,/old/p",
        "/target/w",
    ] {
        assert!(run(&mut editor, input).is_err(), "{input}");
        assert_eq!(editor, before, "{input}");
    }
    let mut empty = Editor::default();
    for input in ["//", "/x/", "??"] {
        assert!(run(&mut empty, input).is_err());
        assert_eq!(empty, Editor::default());
    }
}

#[test]
fn invalid_search_edit_does_not_consume_commands_or_publish_history() {
    let mut editor = editor("old\n");
    let mut output = Vec::new();
    vir::ex::repl::run(
        &mut Cursor::new("/missing/c\n%p\nq!\n"),
        &mut output,
        &mut editor,
    )
    .unwrap();
    assert_eq!(output, b"? pattern not found\nold\n");
    assert_eq!(editor.last_search(), None);
    let before = editor.clone();
    let mut output = Vec::new();
    vir::ex::repl::run(
        &mut Cursor::new("/old/c\nbad\r\r\n.\nq!\n"),
        &mut output,
        &mut editor,
    )
    .unwrap();
    assert_eq!(editor, before);
}

#[test]
fn search_history_survives_undo_and_file_switching() {
    let dir = tempfile::tempdir().unwrap();
    let first = dir.path().join("first");
    let next = dir.path().join("next");
    std::fs::write(&first, "target\nother\n").unwrap();
    std::fs::write(&next, "new target\n").unwrap();
    let mut editor = Editor::default();
    editor.open_arguments(vec![first, next]).unwrap();
    run(&mut editor, "/target/d").unwrap();
    assert_eq!(editor.buffer().to_text(), "other\n");
    run(&mut editor, "u").unwrap();
    assert_eq!(editor.last_search(), Some("target"));
    run(&mut editor, "n!").unwrap();
    assert_eq!(run(&mut editor, "//").unwrap(), "new target\n");
}
