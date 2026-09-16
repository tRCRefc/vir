use std::fs;
use vir::editor::Editor;
use vir::ex::{
    command::execute,
    parser::{Command, parse},
};

fn run(editor: &mut Editor, command: &str) -> Result<vir::ex::command::ResultValue, String> {
    execute(parse(command).unwrap(), editor)
}

#[test]
fn parses_literal_paths_and_rejects_unsupported_forms() {
    for (input, command, path) in [
        ("w 文件 name", Command::Write, "文件 name"),
        ("wq output", Command::WriteQuit, "output"),
        ("edit! next", Command::Edit { force: true }, "next"),
        ("file next", Command::File, "next"),
    ] {
        let parsed = parse(input).unwrap();
        assert_eq!(parsed.command, command);
        assert_eq!(parsed.path.as_deref(), Some(path));
    }
    for input in [
        "w! file", "wq! file", "w !shell", "w >>file", "e!file", "f!", "q file", "w a\nb", "w a\0b",
    ] {
        assert!(parse(input).is_err(), "{input}");
    }
}

#[test]
fn unnamed_write_adopts_name_only_after_success_and_keeps_undo() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("new file");
    let mut editor = Editor::default();
    editor.insert_lines(0, ["one"]).unwrap();
    let before = editor.clone();
    assert!(run(&mut editor, "w").is_err());
    assert!(
        run(
            &mut editor,
            &format!("w {}/missing/child", dir.path().display())
        )
        .is_err()
    );
    assert_eq!(editor, before);
    run(&mut editor, &format!("w {}", target.display())).unwrap();
    assert_eq!(editor.path(), Some(target.as_path()));
    assert!(!editor.is_modified());
    editor.undo().unwrap();
    assert!(editor.buffer().is_empty());
    assert_eq!(fs::read_to_string(target).unwrap(), "one\n");
}

#[test]
fn export_keeps_current_name_and_refuses_existing_destinations() {
    let dir = tempfile::tempdir().unwrap();
    let current = dir.path().join("current");
    let other = dir.path().join("other");
    fs::write(&current, "old\n").unwrap();
    fs::write(&other, "protected\n").unwrap();
    let mut editor = Editor::default();
    editor.load(&current).unwrap();
    editor.insert_lines(1, ["new"]).unwrap();
    let before = editor.clone();
    assert!(run(&mut editor, &format!("w {}", other.display())).is_err());
    assert_eq!(editor, before);
    assert_eq!(fs::read_to_string(&other).unwrap(), "protected\n");
    fs::remove_file(&other).unwrap();
    run(&mut editor, &format!("w {}", other.display())).unwrap();
    assert_eq!(editor.path(), Some(current.as_path()));
    assert!(!editor.is_modified());
    assert_eq!(fs::read_to_string(&current).unwrap(), "old\n");
    assert_eq!(fs::read_to_string(other).unwrap(), "old\nnew\n");
}

#[test]
fn forced_edit_is_transactional_and_reload_clears_undo() {
    let dir = tempfile::tempdir().unwrap();
    let original = dir.path().join("original");
    let bad = dir.path().join("bad");
    fs::write(&original, "disk\n").unwrap();
    fs::write(&bad, [0xff]).unwrap();
    let mut editor = Editor::default();
    editor.load(&original).unwrap();
    editor.insert_lines(1, ["unsaved"]).unwrap();
    let before = editor.clone();
    assert!(run(&mut editor, "e").is_err());
    assert!(run(&mut editor, &format!("e {}", bad.display())).is_err());
    assert!(run(&mut editor, &format!("e! {}", bad.display())).is_err());
    assert!(run(&mut editor, &format!("e! {}", dir.path().display())).is_err());
    assert_eq!(editor, before);
    run(&mut editor, "e!").unwrap();
    assert_eq!(editor.buffer().to_text(), "disk\n");
    assert!(!editor.is_modified());
    assert!(editor.undo().is_err());
    let missing = dir.path().join("new");
    run(&mut editor, &format!("e {}", missing.display())).unwrap();
    assert!(editor.buffer().is_empty());
    assert_eq!(editor.path(), Some(missing.as_path()));
    assert!(!missing.exists());
}

#[test]
fn file_status_and_rename_preserve_text_and_protect_existing_file() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("target");
    fs::write(&target, "protected\n").unwrap();
    let mut editor = Editor::default();
    assert_eq!(
        run(&mut editor, "f").unwrap().output,
        "No file line 0 of 0\n"
    );
    editor.insert_lines(0, ["new"]).unwrap();
    let status = run(&mut editor, &format!("f {}", target.display())).unwrap();
    assert!(status.output.contains("[Modified] line 1 of 1"));
    let before = editor.clone();
    assert!(run(&mut editor, "w").is_err());
    assert_eq!(editor, before);
    assert_eq!(fs::read_to_string(&target).unwrap(), "protected\n");
    editor.undo().unwrap();
    assert!(editor.buffer().is_empty());
    fs::remove_file(&target).unwrap();
    run(&mut editor, "w").unwrap();
    run(&mut editor, "w").unwrap();
}

#[test]
fn file_commands_reject_addresses_and_wq_quits_only_after_success() {
    let dir = tempfile::tempdir().unwrap();
    let mut editor = Editor::default();
    editor.insert_lines(0, ["new"]).unwrap();
    let before = editor.clone();
    for command in ["1e!", "1f", "%wq", "wq", "e!"] {
        assert!(run(&mut editor, command).is_err());
        assert_eq!(editor, before);
    }
    let target = dir.path().join("new");
    assert!(
        run(&mut editor, &format!("wq {}", target.display()))
            .unwrap()
            .quit
    );
    assert_eq!(fs::read_to_string(target).unwrap(), "new\n");
    assert!(!editor.is_modified());
}
