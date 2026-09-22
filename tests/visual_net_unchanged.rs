//! Cross-entry contracts: unchanged text does not imply unchanged session state.
use vir::{
    buffer::{Position, TextBuffer},
    editor::{Editor, InputAction, Register},
    visual::{InputStart, Motion, VisualState},
};

fn change(
    state: &mut VisualState,
    editor: &mut Editor,
    motion: Option<Motion>,
    count: usize,
    text: &str,
) {
    let mut edit = state.begin_change(editor, motion, Some(count)).unwrap();
    edit.insert(text).unwrap();
    state
        .commit_input(
            edit,
            InputStart::Change {
                motion,
                count: Some(count),
            },
        )
        .unwrap();
}

// Seed an effective count of three, an older undo transaction, and a distinct register.
fn fixture(saved: bool, lf: &str) -> (tempfile::TempDir, Editor, VisualState) {
    let directory = tempfile::tempdir().unwrap();
    let mut editor =
        Editor::from_buffer(TextBuffer::from_text(&format!("QQQ\n中🙂C\nZZZZ\nREG{lf}")).unwrap());
    let mut state = VisualState::default();
    change(&mut state, &mut editor, Some(Motion::Right), 3, "中🙂");
    state.row = 3;
    state.column = 0;
    state.yank(&mut editor, None, Some(1)).unwrap();
    state.row = 1;
    state.move_cursor(Motion::Home, editor.buffer(), 8);
    if saved {
        editor
            .write_to(Some(&directory.path().join("text")))
            .unwrap();
    }
    state.checkpoint_line(&editor);
    (directory, editor, state)
}

fn assert_old_history(editor: &Editor, state: &VisualState, lf: &str) {
    let mut editor = editor.clone();
    let mut state = state.clone();
    let before = editor.buffer().clone();
    let register = editor.unnamed_register().cloned();
    state.undo(&mut editor).unwrap();
    assert_eq!(
        editor.buffer().to_text(),
        format!("QQQ\n中🙂C\nZZZZ\nREG{lf}")
    );
    assert_eq!((state.row, state.column), (0, 0));
    state.undo(&mut editor).unwrap();
    assert_eq!(editor.buffer(), &before);
    assert_eq!(editor.unnamed_register(), register.as_ref());
}

fn assert_old_repeat(editor: &Editor, state: &VisualState) {
    let mut editor = editor.clone();
    let mut state = state.clone();
    state.row = 2;
    state.column = 0;
    state.repeat_change(&mut editor, None).unwrap();
    // A leaked count of two would produce 中🙂ZZ; a leaked cc would replace the line.
    assert_eq!(editor.buffer().line(2), Some("中🙂Z"));
}

fn assert_saved_text(editor: &Editor, directory: &tempfile::TempDir) {
    let mut editor = editor.clone();
    let path = directory.path().join("reopened");
    editor.write_to(Some(&path)).unwrap();
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        editor.buffer().to_text()
    );
    let mut reopened = Editor::default();
    reopened.load(&path).unwrap();
    assert_eq!(reopened.buffer(), editor.buffer());
    assert!(!reopened.is_modified());
}

#[test]
fn direct_unchanged_change_publishes_register_and_cursor_but_preserves_history() {
    for saved in [false, true] {
        for lf in ["", "\n"] {
            for (motion, count, text, register, end) in [
                (
                    Some(Motion::Right),
                    2,
                    "中🙂",
                    Register::Characters("中🙂".into()),
                    (1, 1),
                ),
                (
                    None,
                    1,
                    "中🙂C",
                    Register::Lines(vec!["中🙂C".into()]),
                    (1, 2),
                ),
                (
                    Some(Motion::End),
                    2,
                    "中🙂C\nZZZZ",
                    Register::Characters("中🙂C\nZZZZ".into()),
                    (2, 3),
                ),
                (
                    None,
                    2,
                    "中🙂C\nZZZZ",
                    Register::Lines(vec!["中🙂C".into(), "ZZZZ".into()]),
                    (2, 3),
                ),
            ] {
                let (directory, mut editor, mut state) = fixture(saved, lf);
                let before = editor.buffer().clone();
                let current = editor.current();
                change(&mut state, &mut editor, motion, count, text);
                assert_eq!(editor.buffer(), &before);
                assert_eq!(editor.current(), current);
                assert_eq!(editor.is_modified(), !saved);
                assert_eq!(editor.unnamed_register(), Some(&register));
                assert_eq!((state.row, state.column), end);
                assert_old_history(&editor, &state, lf);
                assert_old_repeat(&editor, &state);
                assert_saved_text(&editor, &directory);
                // Vertical navigation exposes the session's preferred column.
                state.move_cursor(Motion::Down, editor.buffer(), 8);
                assert_eq!(
                    state.column,
                    end.1
                        .min(editor.buffer().line(state.row).unwrap().chars().count() - 1)
                );
            }
        }
    }
}

#[test]
fn unchanged_dot_discards_register_cursor_and_explicit_count_together() {
    for saved in [false, true] {
        for lf in ["", "\n"] {
            let (directory, mut editor, mut state) = fixture(saved, lf);
            let before = editor.clone();
            let visual = format!("{state:?}");
            for _ in 0..2 {
                state.repeat_change(&mut editor, Some(2)).unwrap();
                assert_eq!(editor, before);
                assert_eq!(format!("{state:?}"), visual);
            }
            assert_old_history(&editor, &state, lf);
            assert_old_repeat(&editor, &state);
            assert_saved_text(&editor, &directory);
            // The old line register is still usable; the staged character register is absent.
            state.put(&mut editor, true, 1).unwrap();
            assert_eq!(editor.buffer().line(1), Some("REG"));
            assert_eq!(editor.buffer().line(2), Some("中🙂C"));
        }
    }
}

#[test]
fn empty_and_retracted_input_preserve_saved_state_undo_and_dot() {
    for saved in [false, true] {
        for lf in ["", "\n"] {
            for (mode, actions) in [
                ('i', vec![]),
                (
                    'i',
                    vec![InputAction::Insert("文".into()), InputAction::Backspace],
                ),
                (
                    'i',
                    vec![InputAction::Insert("\n".into()), InputAction::Backspace],
                ),
                ('R', vec![]),
                (
                    'R',
                    vec![InputAction::Replace('文'), InputAction::RestoreReplacement],
                ),
                (
                    'R',
                    vec![
                        InputAction::ReplaceText("文\n字".into()),
                        InputAction::RestoreReplacement,
                        InputAction::RestoreReplacement,
                        InputAction::RestoreReplacement,
                    ],
                ),
            ] {
                for count in [1, 3] {
                    let (directory, mut editor, mut state) = fixture(saved, lf);
                    state.column = 1;
                    let before = editor.clone();
                    let mut session = state
                        .begin_counted_insert(&mut editor, mode, count)
                        .unwrap();
                    for action in &actions {
                        session.apply_input(action).unwrap();
                    }
                    // The multiline R input generated count-1 extra copies of 文.
                    if mode == 'R'
                        && matches!(actions.first(), Some(InputAction::ReplaceText(text)) if text.contains('\n'))
                    {
                        for _ in 1..count {
                            session
                                .apply_input(&InputAction::RestoreReplacement)
                                .unwrap();
                        }
                    }
                    session.commit(&mut state).unwrap();
                    assert_eq!(editor, before, "{mode} count={count} actions={actions:?}");
                    assert_eq!((state.row, state.column), (1, 0));
                    assert_old_history(&editor, &state, lf);
                    assert_old_repeat(&editor, &state);
                    assert_saved_text(&editor, &directory);
                }
            }
        }
    }
}

#[test]
fn identical_r_failed_change_and_dropped_draft_preserve_all_published_state() {
    for saved in [false, true] {
        for lf in ["", "\n"] {
            let (_directory, mut editor, mut state) = fixture(saved, lf);
            let before = editor.clone();
            let visual = format!("{state:?}");
            state.replace_character(&mut editor, '中').unwrap();
            assert_eq!(editor, before);
            assert_eq!(format!("{state:?}"), visual);
            assert!(
                state
                    .begin_change(&mut editor, Some(Motion::Up), Some(3))
                    .is_err()
            );
            {
                let mut edit = state.begin_change(&mut editor, None, Some(2)).unwrap();
                edit.insert("replacement").unwrap();
            }
            assert_eq!(editor, before);
            assert_eq!(format!("{state:?}"), visual);
            assert_old_history(&editor, &state, lf);
            assert_old_repeat(&editor, &state);
        }
    }
}

#[test]
fn retracted_split_invalidates_u_only_for_the_direct_input_session() {
    for mode in ['i', 'R'] {
        for saved in [false, true] {
            for lf in ["", "\n"] {
                let directory = tempfile::tempdir().unwrap();
                let mut editor =
                    Editor::from_buffer(TextBuffer::from_text(&format!("abc{lf}")).unwrap());
                editor
                    .write_to(Some(&directory.path().join("text")))
                    .unwrap();
                let mut state = VisualState::default();
                let split = if mode == 'R' {
                    InputAction::Replace('\n')
                } else {
                    InputAction::Insert("\n".into())
                };
                let retract = if mode == 'R' {
                    InputAction::RestoreReplacement
                } else {
                    InputAction::Backspace
                };
                // Seed a successful split/join recipe whose net effect is rX.
                let mut session = state.begin_counted_insert(&mut editor, mode, 1).unwrap();
                session.apply_input(&split).unwrap();
                session.apply_input(&retract).unwrap();
                if mode == 'R' {
                    session.apply_input(&InputAction::Replace('X')).unwrap();
                } else {
                    session.apply_input(&InputAction::Delete).unwrap();
                    session
                        .apply_input(&InputAction::Insert("X".into()))
                        .unwrap();
                }
                session.commit(&mut state).unwrap();
                // Shared editor mutations preserve the visual dot recipe. Establish
                // an abc visit snapshot, then change only this line to Xbc.
                state.forget_line();
                for text in ["abc", "Xbc"] {
                    let mut edit = editor.begin_text_edit(Position::default()).unwrap();
                    edit.delete_range(Position::default(), Position { row: 0, column: 3 })
                        .unwrap();
                    edit.insert(text).unwrap();
                    edit.commit().unwrap();
                    state.checkpoint_line(&editor);
                }
                if saved {
                    editor.save().unwrap();
                }
                let before = editor.clone();
                let visual = format!("{state:?}");

                // Retracting text without crossing a line keeps the visit snapshot.
                let mut unsplit_editor = editor.clone();
                let mut unsplit_state = state.clone();
                let mut session = unsplit_state
                    .begin_counted_insert(&mut unsplit_editor, mode, 1)
                    .unwrap();
                session
                    .apply_input(&if mode == 'R' {
                        InputAction::Replace('Y')
                    } else {
                        InputAction::Insert("Y".into())
                    })
                    .unwrap();
                session.apply_input(&retract).unwrap();
                session.commit(&mut unsplit_state).unwrap();
                assert_eq!(unsplit_editor, before);
                unsplit_state.restore_line(&mut unsplit_editor).unwrap();
                assert_eq!(unsplit_editor.buffer().to_text(), format!("abc{lf}"));

                let mut direct_editor = editor.clone();
                let mut direct_state = state.clone();
                let mut session = direct_state
                    .begin_counted_insert(&mut direct_editor, mode, 1)
                    .unwrap();
                session.apply_input(&split).unwrap();
                session.apply_input(&retract).unwrap();
                session.commit(&mut direct_state).unwrap();
                assert_eq!(direct_editor, before);
                assert_eq!(
                    direct_state.restore_line(&mut direct_editor).unwrap_err(),
                    "no line changes to restore"
                );
                assert_eq!(direct_editor, before);
                let cursor = (direct_state.row, direct_state.column);
                assert!(direct_state.restore_line(&mut direct_editor).is_err());
                assert_eq!((direct_state.row, direct_state.column), cursor);
                direct_state.undo(&mut direct_editor).unwrap();
                assert_eq!(direct_editor.buffer().to_text(), format!("abc{lf}"));
                direct_state.undo(&mut direct_editor).unwrap();
                assert_eq!(direct_editor.buffer(), before.buffer());

                // The identical split/join on a staged replay must not discard U's snapshot.
                state.repeat_change(&mut editor, None).unwrap();
                assert_eq!(editor, before);
                assert_eq!(format!("{state:?}"), visual);
                state.restore_line(&mut editor).unwrap();
                assert_eq!(editor.buffer().to_text(), format!("abc{lf}"));
                let restored = editor.clone();
                let cursor = (state.row, state.column);
                assert!(state.restore_line(&mut editor).is_err());
                assert_eq!(editor, restored);
                assert_eq!((state.row, state.column), cursor);
                // U does not overwrite the split/join recipe.
                state.repeat_change(&mut editor, None).unwrap();
                assert_eq!(editor.buffer().to_text(), format!("Xbc{lf}"));
            }
        }
    }
}

#[test]
fn multiline_unchanged_dot_preserves_the_previous_transaction_and_line_snapshot() {
    for saved in [false, true] {
        for lf in ["", "\n"] {
            let directory = tempfile::tempdir().unwrap();
            let original = format!("aa\nbb\n中\n🙂\nREG{lf}");
            let mut editor = Editor::from_buffer(TextBuffer::from_text(&original).unwrap());
            let mut state = VisualState::default();
            change(&mut state, &mut editor, Some(Motion::End), 2, "中\n🙂");
            state.row = 4;
            state.column = 0;
            state.yank(&mut editor, None, Some(1)).unwrap();
            state.row = 2;
            if saved {
                editor
                    .write_to(Some(&directory.path().join("text")))
                    .unwrap();
            }
            state.checkpoint_line(&editor);
            let before = editor.clone();
            let visual = format!("{state:?}");
            state.repeat_change(&mut editor, None).unwrap();
            assert_eq!(editor, before);
            assert_eq!(format!("{state:?}"), visual);
            state.undo(&mut editor).unwrap();
            assert_eq!(editor.buffer().to_text(), original);
            state.undo(&mut editor).unwrap();
            assert_eq!(editor.buffer(), before.buffer());
            assert_eq!(
                editor.unnamed_register(),
                Some(&Register::Lines(vec!["REG".into()]))
            );
            assert_saved_text(&editor, &directory);
        }
    }
}
