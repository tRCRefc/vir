use std::fs;
use vir::{
    editor::Editor,
    ex::{
        command::execute,
        parser::{Command, parse},
    },
};

fn command(editor: &mut Editor, text: &str) -> Result<vir::ex::command::ResultValue, String> {
    execute(parse(text).unwrap(), editor)
}

#[test]
fn parses_next_force_and_args_without_confusing_number_or_append() {
    for name in ["n!", "next!", "ne!"] {
        assert_eq!(parse(name).unwrap().command, Command::Next { force: true });
    }
    for name in ["ar", "arg", "args"] {
        assert_eq!(parse(name).unwrap().command, Command::Args);
    }
    assert_eq!(parse("a").unwrap().command, Command::Append);
    assert_eq!(parse("nu").unwrap().command, Command::Number);
    for name in ["args!", "args file", "next file"] {
        assert!(parse(name).is_err());
    }
}

#[test]
fn next_failure_preserves_index_text_and_undo_and_can_be_retried() {
    let dir = tempfile::tempdir().unwrap();
    let first = dir.path().join("first");
    let second = dir.path().join("second");
    fs::write(&first, "first\n").unwrap();
    fs::write(&second, [0xff]).unwrap();
    let mut editor = Editor::default();
    editor
        .open_arguments(vec![first.clone(), second.clone()])
        .unwrap();
    editor.insert_lines(1, ["unsaved"]).unwrap();
    let before = editor.clone();
    for cmd in ["n", "n!", "1n!", "%args"] {
        assert!(command(&mut editor, cmd).is_err());
        assert_eq!(editor, before);
    }
    fs::write(&second, "second\n").unwrap();
    command(&mut editor, "n!").unwrap();
    assert_eq!(editor.argument_index(), Some(1));
    assert_eq!(editor.path(), Some(second.as_path()));
    assert_eq!(editor.buffer().to_text(), "second\n");
    assert!(editor.undo().is_err());
    let before = editor.clone();
    assert_eq!(command(&mut editor, "n!").unwrap_err(), "no more files");
    assert_eq!(editor, before);
}

#[test]
fn args_and_edit_keep_argument_position_and_quit_warns_once() {
    let dir = tempfile::tempdir().unwrap();
    let paths: Vec<_> = ["one", "two", "three"]
        .iter()
        .map(|name| dir.path().join(name))
        .collect();
    let mut editor = Editor::default();
    editor.open_arguments(paths.clone()).unwrap();
    let before = editor.clone();
    assert_eq!(
        command(&mut editor, "args").unwrap().output,
        format!(
            "[{}] {} {}\n",
            paths[0].display(),
            paths[1].display(),
            paths[2].display()
        )
    );
    assert_eq!(editor, before);
    let extra = dir.path().join("extra");
    command(&mut editor, &format!("e {}", extra.display())).unwrap();
    assert_eq!(editor.argument_index(), Some(0));
    assert_eq!(
        command(&mut editor, "q").unwrap_err(),
        "2 more files to edit"
    );
    assert!(command(&mut editor, "q").unwrap().quit);
    command(&mut editor, "n").unwrap();
    assert_eq!(editor.path(), Some(paths[1].as_path()));
    assert_eq!(
        command(&mut editor, "q").unwrap_err(),
        "1 more file to edit"
    );
    assert!(command(&mut editor, "q!").unwrap().quit);
    command(&mut editor, "n").unwrap();
    assert!(command(&mut editor, "q").unwrap().quit);
}

#[test]
fn dirty_quit_does_not_acknowledge_remaining_files_and_wq_warns_after_save() {
    let dir = tempfile::tempdir().unwrap();
    let first = dir.path().join("first");
    let mut editor = Editor::default();
    editor
        .open_arguments(vec![first.clone(), dir.path().join("second")])
        .unwrap();
    editor.insert_lines(0, ["kept"]).unwrap();
    let before = editor.clone();
    assert_eq!(command(&mut editor, "q").unwrap_err(), "unsaved changes");
    assert_eq!(editor, before);
    assert_eq!(
        command(&mut editor, "wq").unwrap_err(),
        "1 more file to edit"
    );
    assert_eq!(fs::read_to_string(first).unwrap(), "kept\n");
    assert!(!editor.is_modified());
    assert!(command(&mut editor, "q").unwrap().quit);
}

#[test]
fn initial_load_failure_is_transactional_and_empty_list_is_valid() {
    let dir = tempfile::tempdir().unwrap();
    let mut editor = Editor::default();
    editor
        .open_arguments(vec![dir.path().join("first")])
        .unwrap();
    let before = editor.clone();
    assert!(editor.open_arguments(vec![dir.path().to_owned()]).is_err());
    assert_eq!(editor, before);
    editor.open_arguments(vec![]).unwrap();
    assert_eq!(editor, Editor::default());
    assert_eq!(command(&mut editor, "args").unwrap().output, "\n");
    assert_eq!(command(&mut editor, "n").unwrap_err(), "no more files");
}
