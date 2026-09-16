use vir::{
    buffer::TextBuffer,
    editor::Editor,
    ex::{command::execute, parser::parse},
};
fn editor(text: &str) -> Editor {
    Editor::from_buffer(TextBuffer::from_text(text).unwrap())
}
fn run(editor: &mut Editor, text: &str) -> Result<(), String> {
    execute(parse(text).unwrap(), editor).map(|_| ())
}

#[test]
fn defaults_to_dot_and_replaces_first_match_per_line() {
    let mut editor = editor("a a\na a\n");
    run(&mut editor, "s/a/X/").unwrap();
    assert_eq!(editor.buffer().to_text(), "a a\nX a\n");
    run(&mut editor, "%s/a/Y/").unwrap();
    assert_eq!(editor.buffer().to_text(), "Y a\nX Y\n");
}

#[test]
fn global_range_is_one_reversible_transaction_and_dot_is_last_match() {
    let mut editor = editor("a a\na a\nnone\n");
    let original = editor.buffer().clone();
    run(&mut editor, "%s/a/X/g").unwrap();
    assert_eq!(editor.buffer().to_text(), "X X\nX X\nnone\n");
    assert_eq!(editor.current(), Some(1));
    run(&mut editor, "u").unwrap();
    assert_eq!(editor.buffer(), &original);
    run(&mut editor, "u").unwrap();
    assert_eq!(editor.buffer().to_text(), "X X\nX X\nnone\n");
}

#[test]
fn replacement_expands_ex_captures_and_preserves_literal_dollars() {
    let mut editor = editor("ab\n");
    run(&mut editor, r"s/(a)(b)/\2\1-&-\&-\\-\/-$1/").unwrap();
    assert_eq!(editor.buffer().to_text(), "ba-ab-&-\\-/-$1\n");
    run(&mut editor, r"s/ba//").unwrap();
    assert_eq!(editor.buffer().to_text(), "-ab-&-\\-/-$1\n");
}

#[test]
fn empty_patterns_reuse_search_and_same_command_address_pattern() {
    let mut editor = editor("cat\ndog\n");
    run(&mut editor, "/cat/s//kitten/").unwrap();
    assert_eq!(editor.buffer().to_text(), "kitten\ndog\n");
    assert_eq!(editor.last_search(), Some("cat"));
    run(&mut editor, "/dog/").unwrap();
    run(&mut editor, "s//puppy/").unwrap();
    assert_eq!(editor.buffer().to_text(), "kitten\npuppy\n");
    run(&mut editor, "%s/kitten/cat/").unwrap();
    assert_eq!(editor.last_search(), Some("kitten"));
}

#[test]
fn zero_width_unicode_and_optional_captures_terminate_correctly() {
    let mut editor = editor("中文\n");
    run(&mut editor, "s/(?:)/_/g").unwrap();
    assert_eq!(editor.buffer().to_text(), "_中_文_\n");
    let mut editor = self::editor("b\n");
    run(&mut editor, r"s/(a)?b/<\1>/").unwrap();
    assert_eq!(editor.buffer().to_text(), "<>\n");
    run(&mut editor, "s/^/start/").unwrap();
    run(&mut editor, "s/$/end/").unwrap();
    assert_eq!(editor.buffer().to_text(), "start<>end\n");
}

#[test]
fn errors_preserve_text_cursor_history_and_undo() {
    let mut editor = editor("old\n");
    run(&mut editor, "/old/").unwrap();
    editor.insert_lines(1, ["new"]).unwrap();
    let before = editor.clone();
    for text in [
        "%s/absent/x/",
        "%s/[/x/",
        r"%s/old/\1/",
        r"%s/old/\n/",
        "3s/old/x/",
        "2,1s/old/x/",
        "/new/s/absent/x/",
    ] {
        assert!(run(&mut editor, text).is_err(), "{text}");
        assert_eq!(editor, before, "{text}");
    }
    for text in [
        "s",
        "s/a/b",
        "s/a/b/gg",
        "s/a/b/c",
        "s#a#b#",
        "s/a/line\nbreak/",
    ] {
        assert!(parse(text).is_err(), "{text}");
    }
    let mut empty = Editor::default();
    assert!(run(&mut empty, "s/a/b/").is_err());
    assert_eq!(empty, Editor::default());
    let mut no_history = self::editor("old\n");
    let before = no_history.clone();
    assert!(run(&mut no_history, "s//x/").is_err());
    assert_eq!(no_history, before);
}
