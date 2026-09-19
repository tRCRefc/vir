use std::fs;
use vir::{
    editor::Editor,
    ex::{command::execute, parser::parse},
    visual::{Motion, VisualState},
};

#[test]
fn conflicts_preserve_editor_visual_history_and_external_bytes() {
    for change in [
        "rewrite",
        "replace",
        "delete",
        "timestamp-preserving",
        "touch",
    ] {
        if change == "replace" && !cfg!(unix) {
            continue; // Portable metadata has no file identity in this implementation.
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("text");
        fs::write(&path, "one two three\n").unwrap();
        let mut editor = Editor::default();
        editor.load(&path).unwrap();
        let mut state = VisualState::default();
        state
            .delete_motion(&mut editor, Motion::WordForward, None)
            .unwrap();
        let before = editor.clone();
        let visual = format!("{state:?}");
        let time = fs::metadata(&path).unwrap().modified().unwrap();
        match change {
            "rewrite" => fs::write(&path, "external\n").unwrap(),
            "replace" => {
                let replacement = dir.path().join("replacement");
                fs::write(&replacement, "one two three\n").unwrap();
                fs::File::options()
                    .write(true)
                    .open(&replacement)
                    .unwrap()
                    .set_modified(time)
                    .unwrap();
                fs::rename(replacement, &path).unwrap();
            }
            "delete" => fs::remove_file(&path).unwrap(),
            "timestamp-preserving" => {
                fs::write(&path, "ONE TWO THREE\n").unwrap();
                fs::File::options()
                    .write(true)
                    .open(&path)
                    .unwrap()
                    .set_modified(time)
                    .unwrap();
            }
            "touch" => fs::File::options()
                .write(true)
                .open(&path)
                .unwrap()
                .set_modified(time + std::time::Duration::from_secs(10))
                .unwrap(),
            _ => unreachable!(),
        }
        let disk = fs::read(&path).ok();
        for command in ["w", "wq"] {
            assert!(
                state
                    .execute_ex(command, &mut editor)
                    .unwrap_err()
                    .contains("file changed"),
                "{change}"
            );
            assert_eq!(editor, before);
            assert_eq!(format!("{state:?}"), visual);
            assert_eq!(fs::read(&path).ok(), disk);
        }
        assert_eq!(
            fs::read_dir(dir.path()).unwrap().count(),
            usize::from(disk.is_some())
        );
        editor.undo().unwrap();
        assert_eq!(editor.buffer().to_text(), "one two three\n");
        editor.undo().unwrap();
        state.repeat_change(&mut editor, None).unwrap();
        assert_eq!(editor.buffer().to_text(), "three\n");
    }
}

#[test]
fn successful_save_refreshes_baseline_and_reload_resolves_conflict() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("text");
    let mut editor = Editor::default();
    editor.load(&path).unwrap();
    editor.insert_lines(0, ["new"]).unwrap();
    for _ in 0..3 {
        editor.save().unwrap();
    }
    editor.insert_lines(1, ["more"]).unwrap();
    editor.save().unwrap();
    fs::write(&path, "external\n").unwrap();
    // Even a clean buffer must not overwrite external edits.
    assert!(editor.save().is_err());
    editor.load_with_force(&path, true).unwrap();
    editor.insert_lines(1, ["accepted"]).unwrap();
    editor.save().unwrap();
    editor.undo().unwrap();
    editor.save().unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), "external\n");
    fs::write(&path, "later\n").unwrap();
    assert!(editor.save().is_err());
}

#[test]
fn exports_do_not_rebase_current_file_and_rename_does_not_grant_overwrite() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("text");
    fs::write(&path, "original\n").unwrap();
    let mut editor = Editor::default();
    editor.load(&path).unwrap();
    editor.insert_lines(1, ["unsaved"]).unwrap();
    fs::write(&path, "external\n").unwrap();
    for command in ["1w", "w"] {
        let export = dir.path().join(command);
        execute(
            parse(&format!("{command} {}", export.display())).unwrap(),
            &mut editor,
        )
        .unwrap();
        assert_eq!(editor.path(), Some(path.as_path()));
        assert!(editor.save().is_err());
    }
    editor.set_path(&path); // Same spelling retains the baseline.
    assert!(editor.save().is_err());
    let new = dir.path().join("renamed");
    editor.set_path(&new);
    editor.save().unwrap();
    editor.save().unwrap();
    editor.set_path(&path);
    assert!(editor.save().is_err());
    assert_eq!(fs::read_to_string(&path).unwrap(), "external\n");
}

#[test]
fn unnamed_save_and_next_file_establish_independent_baselines() {
    let dir = tempfile::tempdir().unwrap();
    let first = dir.path().join("first");
    let second = dir.path().join("second");
    let mut editor = Editor::default();
    editor.insert_lines(0, ["first"]).unwrap();
    editor.write_to(Some(&first)).unwrap();
    editor.save().unwrap();
    fs::write(&first, "external\n").unwrap();
    assert!(editor.save().is_err());
    fs::write(&second, "second\n").unwrap();
    editor
        .open_arguments(vec![first.clone(), second.clone()])
        .unwrap();
    execute(parse("next").unwrap(), &mut editor).unwrap();
    editor.insert_lines(1, ["new"]).unwrap();
    editor.save().unwrap();
    fs::write(&second, "changed\n").unwrap();
    assert!(editor.save().is_err());
    assert_eq!(fs::read_to_string(&first).unwrap(), "external\n");
}

#[test]
fn ex_cli_continues_after_conflicting_write_quit_and_can_export() {
    use std::io::{BufRead, BufReader, Write};
    use std::process::{Command, Stdio};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("text");
    let export = dir.path().join("export");
    fs::write(&path, "original\n").unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_vir"))
        .arg("--ex")
        .arg(&path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap());
    input.write_all(b"p\n").unwrap();
    let mut line = String::new();
    output.read_line(&mut line).unwrap();
    assert_eq!(line, "original\n"); // Loaded before the external edit.
    fs::write(&path, "external\n").unwrap();
    writeln!(input, "a\nlocal\n.\nwq\nw {}\nq", export.display()).unwrap();
    drop(input);
    line.clear();
    output.read_line(&mut line).unwrap();
    assert!(line.contains("file changed"));
    assert!(child.wait_with_output().unwrap().status.success());
    assert_eq!(fs::read_to_string(path).unwrap(), "external\n");
    assert_eq!(fs::read_to_string(export).unwrap(), "original\nlocal\n");
}
