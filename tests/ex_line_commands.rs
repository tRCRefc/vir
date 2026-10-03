use vir::{
    buffer::TextBuffer,
    editor::{Editor, Register},
    ex::{
        command::{execute, validate_addresses},
        parser::parse,
    },
    visual::VisualState,
};

fn editor(text: &str) -> Editor {
    Editor::from_buffer(TextBuffer::from_text(text).unwrap())
}

#[test]
fn join_addresses_spacing_and_undo() {
    for (command, expected) in [
        ("1join", "One.  two\n )three\nlast"),
        ("1,3j", "One.  two)three\nlast"),
        ("%join", "One.  two)three last"),
        ("2,2join", "One.\n  two\n )three\nlast"),
        ("$j", "One.\n  two\n )three\nlast"),
    ] {
        let initial = "One.\n  two\n )three\nlast";
        let mut ed = editor(initial);
        ed.set_named_register('a', Register::Characters("stored".into()), false)
            .unwrap();
        let parsed = parse(command).unwrap();
        validate_addresses(&parsed, &ed).unwrap();
        execute(parsed, &mut ed).unwrap();
        assert_eq!(ed.buffer().to_text(), expected, "{command}");
        assert_eq!(
            ed.named_register('a'),
            Some(&Register::Characters("stored".into()))
        );
        assert_eq!(ed.numbered_register(1), None);
        ed.undo().unwrap();
        assert_eq!(ed.buffer().to_text(), initial);
    }
    for command in ["0j", "4,2j", "1,9j"] {
        let mut ed = editor("a\nb\nc\nd\n");
        let saved = ed.clone();
        assert!(execute(parse(command).unwrap(), &mut ed).is_err());
        assert_eq!(ed, saved);
    }
    assert!(execute(parse("j").unwrap(), &mut Editor::default()).is_err());
    for command in ["j!", "j 3", "r !cat", "r! cat"] {
        assert!(parse(command).is_err());
    }
}

#[test]
fn read_inserts_literal_file_lines_preserving_identity_and_undo() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source file");
    let target = dir.path().join("target");
    std::fs::write(&source, "  中\n\tlast").unwrap();
    std::fs::write(&target, "before\nafter\n").unwrap();
    for (address, expected, row) in [
        ("0", "  中\n\tlast\nbefore\nafter\n", 1),
        ("1", "before\n  中\n\tlast\nafter\n", 2),
        ("$", "before\nafter\n  中\n\tlast\n", 3),
    ] {
        let mut ed = Editor::default();
        ed.load_with_force(&target, true).unwrap();
        ed.set_autoindent(true);
        execute(
            parse(&format!("{address}read {}", source.display())).unwrap(),
            &mut ed,
        )
        .unwrap();
        assert_eq!(ed.buffer().to_text(), expected);
        assert_eq!(ed.current(), Some(row));
        assert_eq!(ed.path(), Some(target.as_path()));
        assert!(ed.is_modified());
        ed.undo().unwrap();
        assert_eq!(ed.buffer().to_text(), "before\nafter\n");
    }
    let mut empty = Editor::default();
    execute(
        parse(&format!("0r {}", source.display())).unwrap(),
        &mut empty,
    )
    .unwrap();
    assert_eq!(empty.buffer().to_text(), "  中\n\tlast\n");
    assert_eq!(empty.path(), None);
    empty.undo().unwrap();
    assert!(empty.buffer().is_empty());
}

#[test]
fn read_errors_and_empty_files_preserve_editor_state() {
    let dir = tempfile::tempdir().unwrap();
    for (name, bytes) in [
        ("empty", &b""[..]),
        ("crlf", &b"bad\r\n"[..]),
        ("invalid", &b"\xff"[..]),
    ] {
        std::fs::write(dir.path().join(name), bytes).unwrap();
    }
    let mut ed = editor("original\n");
    ed.insert_lines(1, ["edit"]).unwrap();
    let saved = ed.clone();
    for name in ["missing", "crlf", "invalid", ""] {
        assert!(
            execute(
                parse(&format!("r {}", dir.path().join(name).display())).unwrap(),
                &mut ed
            )
            .is_err()
        );
        assert_eq!(ed, saved);
    }
    execute(
        parse(&format!("r {}", dir.path().join("empty").display())).unwrap(),
        &mut ed,
    )
    .unwrap();
    assert_eq!(ed, saved);
    assert!(execute(parse("r").unwrap(), &mut ed).is_err());
    assert_eq!(ed, saved);
}

#[test]
fn visual_join_and_read_sync_cursor_and_allow_undo() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("read");
    std::fs::write(&source, "new\nlast\n").unwrap();
    let mut ed = editor("one\ntwo\nthree\n");
    let mut state = VisualState::default();
    state.execute_ex("1,2j", &mut ed).unwrap();
    assert_eq!(ed.buffer().to_text(), "one two\nthree\n");
    assert_eq!((state.row, state.column), (0, 0));
    state
        .execute_ex(&format!("0r {}", source.display()), &mut ed)
        .unwrap();
    assert_eq!((state.row, state.column), (1, 0));
    state.execute_ex("u", &mut ed).unwrap();
    assert_eq!(ed.buffer().to_text(), "one two\nthree\n");
    assert_eq!(state.row, ed.current().unwrap());
}

#[test]
fn read_default_path_keeps_original_file_conflict_guard() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("current");
    std::fs::write(&path, "old\n").unwrap();
    let mut ed = Editor::default();
    ed.load_with_force(&path, true).unwrap();
    std::fs::write(&path, "external\n").unwrap();
    execute(parse("r").unwrap(), &mut ed).unwrap();
    assert_eq!(ed.buffer().to_text(), "old\nexternal\n");
    assert!(execute(parse("w").unwrap(), &mut ed).is_err());
    assert_eq!(std::fs::read_to_string(path).unwrap(), "external\n");
}
