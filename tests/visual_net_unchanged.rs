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
fn direct_unchanged_change_publishes_register_cursor_and_history() {
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
                change(&mut state, &mut editor, motion, count, text);
                assert_eq!(editor.buffer(), &before);
                assert_eq!(editor.current(), Some(end.0));
                assert!(editor.is_modified());
                assert_eq!(editor.unnamed_register(), Some(&register));
                assert_eq!((state.row, state.column), end);
                let mut undone = editor.clone();
                for _ in 0..2 {
                    undone.undo().unwrap();
                    assert_eq!(undone.buffer(), &before);
                }
                let mut repeated = editor.clone();
                let mut repeat_state = state.clone();
                repeat_state.row = 2;
                repeat_state.column = 0;
                repeat_state.repeat_change(&mut repeated, None).unwrap();
                assert_eq!(
                    repeated.buffer().line(2),
                    Some(if motion == Some(Motion::Right) {
                        "中🙂ZZ"
                    } else {
                        "中🙂C"
                    })
                );
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
fn unchanged_dot_publishes_register_cursor_and_explicit_count_together() {
    for saved in [false, true] {
        for lf in ["", "\n"] {
            let (directory, mut editor, mut state) = fixture(saved, lf);
            let before = editor.buffer().clone();
            for _ in 0..2 {
                state.column = 0;
                state.repeat_change(&mut editor, Some(2)).unwrap();
                assert_eq!(editor.buffer(), &before);
                assert!(editor.is_modified());
                assert_eq!(state.column, 1);
                assert_eq!(
                    editor.unnamed_register(),
                    Some(&Register::Characters("中🙂".into()))
                );
                for _ in 0..2 {
                    state.undo(&mut editor).unwrap();
                    assert_eq!(editor.buffer(), &before);
                }
            }
            assert_saved_text(&editor, &directory);
            let mut repeat_editor = editor.clone();
            let mut repeat_state = state.clone();
            repeat_state.row = 2;
            repeat_state.column = 0;
            repeat_state
                .repeat_change(&mut repeat_editor, None)
                .unwrap();
            assert_eq!(repeat_editor.buffer().line(2), Some("中🙂ZZ"));
            state.column = 0;
            state.put(&mut editor, true, 1).unwrap();
            assert_eq!(editor.buffer().line(1), Some("中🙂中🙂C"));
        }
    }
}

#[test]
fn retracted_multiline_input_preserves_saved_state_undo_and_dot() {
    for saved in [false, true] {
        for lf in ["", "\n"] {
            for (mode, actions) in [
                (
                    'i',
                    vec![InputAction::Insert("\n".into()), InputAction::Backspace],
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
fn failed_change_and_dropped_draft_preserve_all_published_state() {
    for saved in [false, true] {
        for lf in ["", "\n"] {
            let (_directory, mut editor, state) = fixture(saved, lf);
            let before = editor.clone();
            let visual = format!("{state:?}");
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
                assert_eq!(unsplit_editor.buffer(), before.buffer());
                assert!(unsplit_editor.is_modified());
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
                // The reset snapshot is the current line; U may now publish
                // an identical edit but cannot recover the pre-split visit.
                direct_state.restore_line(&mut direct_editor).unwrap();
                assert_eq!(direct_editor.buffer(), before.buffer());
                assert!(direct_state.restore_line(&mut direct_editor).is_err());
                for _ in 0..2 {
                    direct_state.undo(&mut direct_editor).unwrap();
                    assert_eq!(direct_editor.buffer(), before.buffer());
                }

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
fn multiline_unchanged_dot_replaces_history_and_register() {
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
            state.repeat_change(&mut editor, None).unwrap();
            assert_eq!(editor.buffer(), before.buffer());
            assert!(editor.is_modified());
            assert_eq!((state.row, state.column), (3, 0));
            for _ in 0..2 {
                state.undo(&mut editor).unwrap();
                assert_eq!(editor.buffer(), before.buffer());
                assert_eq!(state.row, 2);
            }
            assert_eq!(
                editor.unnamed_register(),
                Some(&Register::Characters("中\n🙂".into()))
            );
            assert_saved_text(&editor, &directory);
        }
    }
}

#[test]
fn empty_input_replaces_history_and_dot_even_after_saving() {
    for mode in ['i', 'a', 'I', 'A', 'R'] {
        for saved in [false, true] {
            for lf in ["", "\n"] {
                for count in [1, 3] {
                    for column in [0, 1, 2] {
                        let (_directory, mut editor, mut state) = fixture(saved, lf);
                        state.column = column;
                        let before = editor.buffer().clone();
                        let register = editor.unnamed_register().cloned();
                        state
                            .begin_counted_insert(&mut editor, mode, count)
                            .unwrap()
                            .commit(&mut state)
                            .unwrap();
                        assert_eq!(editor.buffer(), &before);
                        assert!(editor.is_modified());
                        assert_eq!(
                            state.column,
                            match mode {
                                'a' => column,
                                'I' => 0,
                                'A' => 2,
                                _ => column.saturating_sub(1),
                            }
                        );
                        for _ in 0..2 {
                            state.undo(&mut editor).unwrap();
                            assert_eq!(editor.buffer(), &before);
                        }
                        // Replay at a different line must publish its cursor and undo,
                        // rather than replay the older change or discard the empty result.
                        state.row = 2;
                        state.column = 2;
                        state.repeat_change(&mut editor, Some(2)).unwrap();
                        assert_eq!(
                            (state.row, state.column),
                            (
                                2,
                                match mode {
                                    'a' => 2,
                                    'I' => 0,
                                    'A' => 3,
                                    _ => 1,
                                }
                            )
                        );
                        assert_eq!(editor.buffer(), &before);
                        state.undo(&mut editor).unwrap();
                        assert_eq!(state.row, 2);
                        assert_eq!(editor.buffer(), &before);
                        state.column = 2;
                        state.repeat_change(&mut editor, None).unwrap();
                        assert_eq!(
                            state.column,
                            match mode {
                                'a' => 2,
                                'I' => 0,
                                'A' => 3,
                                _ => 1,
                            }
                        );
                        assert_eq!(editor.unnamed_register(), register.as_ref());
                    }
                }
            }
        }
    }
}

#[test]
fn empty_legacy_input_commit_and_dropped_session_have_distinct_history() {
    for mode in ['i', 'a', 'I', 'A', 'R'] {
        let (_directory, mut editor, mut state) = fixture(true, "\n");
        let before = editor.clone();
        drop(state.begin_counted_insert(&mut editor, mode, 3).unwrap());
        assert_eq!(editor, before);
        assert_old_history(&editor, &state, "\n");
        assert_old_repeat(&editor, &state);
        let edit = state.begin_insert(&mut editor, mode).unwrap();
        state.commit_input(edit, InputStart::Insert(mode)).unwrap();
        assert!(editor.is_modified());
        state.undo(&mut editor).unwrap();
        assert_eq!(editor.buffer(), before.buffer());
        state.row = 2;
        state.column = 2;
        state.repeat_change(&mut editor, None).unwrap();
        assert_eq!(editor.buffer(), before.buffer());
        assert_eq!(
            state.column,
            match mode {
                'a' => 2,
                'I' => 0,
                'A' => 3,
                _ => 1,
            }
        );
    }
}

#[test]
fn empty_append_and_first_nonblank_replay_recompute_entry_positions() {
    for (mode, direct_column, replay_column) in [('a', 3, 1), ('I', 1, 0), ('A', 4, 4)] {
        for count in [1, 3] {
            let directory = tempfile::tempdir().unwrap();
            let mut editor =
                Editor::from_buffer(TextBuffer::from_text("  ab中\n\t🙂xyz\nREG\n").unwrap());
            let mut state = VisualState::default();
            state.replace_character(&mut editor, ' ').unwrap();
            state.row = 2;
            state.yank(&mut editor, None, Some(1)).unwrap();
            state.row = 0;
            state.column = 3;
            editor
                .write_to(Some(&directory.path().join("text")))
                .unwrap();
            let before = editor.buffer().clone();
            let register = editor.unnamed_register().cloned();
            state
                .begin_counted_insert(&mut editor, mode, count)
                .unwrap()
                .commit(&mut state)
                .unwrap();
            assert_eq!(state.column, direct_column);
            assert!(editor.is_modified());
            assert_eq!(editor.buffer(), &before);
            editor.save().unwrap();
            state.row = 1;
            state.column = 1;
            let saved = editor.clone();
            let visual = format!("{state:?}");
            assert!(state.repeat_change(&mut editor, Some(usize::MAX)).is_err());
            assert_eq!(editor, saved);
            assert_eq!(format!("{state:?}"), visual);
            state.repeat_change(&mut editor, Some(2)).unwrap();
            assert_eq!(state.column, replay_column);
            assert!(editor.is_modified());
            assert_eq!(editor.buffer(), &before);
            for _ in 0..2 {
                state.undo(&mut editor).unwrap();
                assert_eq!(editor.buffer(), &before);
                assert_eq!(state.row, 1);
            }
            assert_eq!(editor.unnamed_register(), register.as_ref());
        }
    }
}

#[test]
fn fully_retracted_single_line_input_publishes_an_empty_repeat() {
    for mode in ['i', 'a', 'I', 'A', 'R'] {
        for count in [1, 3] {
            for saved in [false, true] {
                for lf in ["", "\n"] {
                    let (_directory, mut editor, mut state) = fixture(saved, lf);
                    state.column = 1;
                    let before = editor.buffer().clone();
                    let register = editor.unnamed_register().cloned();
                    let mut input = state
                        .begin_counted_insert(&mut editor, mode, count)
                        .unwrap();
                    let typed = if mode == 'R' {
                        InputAction::ReplaceText("文🙂".into())
                    } else {
                        InputAction::Insert("文🙂".into())
                    };
                    let erase = if mode == 'R' {
                        InputAction::RestoreReplacement
                    } else {
                        InputAction::Backspace
                    };
                    // Repeat the cycle to cover retyping after returning to the entry.
                    for _ in 0..2 {
                        input.apply_input(&typed).unwrap();
                        input.apply_input(&erase).unwrap();
                        input.apply_input(&erase).unwrap();
                    }
                    input.commit(&mut state).unwrap();
                    assert_eq!(editor.buffer(), &before);
                    assert!(editor.is_modified());
                    assert_eq!(
                        state.column,
                        match mode {
                            'a' => 1,
                            'A' => 2,
                            _ => 0,
                        }
                    );
                    for _ in 0..2 {
                        state.undo(&mut editor).unwrap();
                        assert_eq!(editor.buffer(), &before);
                    }
                    state.row = 2;
                    state.column = 1;
                    // Empty historical INS remains cheap even if all removed text
                    // would exceed the replay budget with this new count.
                    state.repeat_change(&mut editor, Some(1_000_000)).unwrap();
                    assert_eq!(editor.buffer(), &before);
                    assert_eq!(
                        state.column,
                        match mode {
                            'a' => 1,
                            'A' => 3,
                            _ => 0,
                        }
                    );
                    assert_eq!(editor.unnamed_register(), register.as_ref());
                    state.repeat_change(&mut editor, None).unwrap();
                    assert_eq!(editor.buffer(), &before);
                }
            }
        }
    }
}

#[test]
fn legacy_retracted_input_publishes_history_but_original_text_erasure_does_not() {
    for mode in ['i', 'a', 'I', 'A', 'R'] {
        let (_directory, mut editor, mut state) = fixture(true, "\n");
        let before = editor.buffer().clone();
        let mut edit = state.begin_insert(&mut editor, mode).unwrap();
        if mode == 'R' {
            edit.replace_input('文').unwrap();
            edit.replace_backspace().unwrap();
        } else {
            edit.insert("文").unwrap();
            edit.backspace().unwrap();
        }
        state.commit_input(edit, InputStart::Insert(mode)).unwrap();
        assert!(editor.is_modified());
        state.undo(&mut editor).unwrap();
        assert_eq!(editor.buffer(), &before);
        state.row = 2;
        state.repeat_change(&mut editor, Some(1_000_000)).unwrap();
        assert_eq!(editor.buffer(), &before);
    }
    // Deleting original text then rebuilding it is not vgetline erasure.
    let (_directory, mut editor, mut state) = fixture(true, "\n");
    state.column = 1;
    let before = editor.clone();
    let mut input = state.begin_counted_insert(&mut editor, 'i', 1).unwrap();
    input.apply_input(&InputAction::Backspace).unwrap();
    input
        .apply_input(&InputAction::Insert("中".into()))
        .unwrap();
    input.commit(&mut state).unwrap();
    assert_eq!(editor, before);
    assert_old_history(&editor, &state, "\n");
    assert_old_repeat(&editor, &state);
}

#[test]
fn identical_replace_input_publishes_history_and_normalized_counted_dot() {
    for saved in [false, true] {
        for lf in ["", "\n"] {
            for corrected in [false, true] {
                let (_directory, mut editor, mut state) = fixture(saved, lf);
                let before = editor.buffer().clone();
                let register = editor.unnamed_register().cloned();
                let mut input = state.begin_counted_insert(&mut editor, 'R', 1).unwrap();
                input
                    .paste(if corrected { "中🙂X" } else { "中🙂" })
                    .unwrap();
                if corrected {
                    input.apply_input(&InputAction::RestoreReplacement).unwrap();
                }
                input.commit(&mut state).unwrap();
                assert_eq!(editor.buffer(), &before);
                assert!(editor.is_modified());
                assert_eq!(state.column, 1);
                for _ in 0..2 {
                    state.undo(&mut editor).unwrap();
                    assert_eq!(editor.buffer(), &before);
                }
                state.column = 0;
                state.repeat_change(&mut editor, Some(1)).unwrap();
                assert_eq!(state.column, 1);
                assert_eq!(editor.buffer(), &before);
                state.row = 2;
                state.column = 0;
                state.repeat_change(&mut editor, Some(2)).unwrap();
                assert_eq!(editor.buffer().line(2), Some("中🙂中🙂ZZ"));
                state.row = 3;
                state.column = 0;
                state.repeat_change(&mut editor, None).unwrap();
                assert_eq!(editor.buffer().line(3), Some("中🙂中🙂G"));
                assert_eq!(editor.unnamed_register(), register.as_ref());
            }
        }
    }
}

#[test]
fn partially_erased_input_replays_only_remaining_text_at_large_counts() {
    for mode in ['i', 'a', 'I', 'A', 'R'] {
        let mut editor = Editor::from_buffer(TextBuffer::from_text("abc").unwrap());
        let mut state = VisualState::default();
        let mut edit = state.begin_insert(&mut editor, mode).unwrap();
        if mode == 'R' {
            edit.replace_input_text("X🙂").unwrap();
            edit.replace_backspace().unwrap();
        } else {
            edit.insert("X🙂").unwrap();
            edit.backspace().unwrap();
        }
        state.commit_input(edit, InputStart::Insert(mode)).unwrap();
        // X costs one byte plus one overhead unit. Keeping erased emoji in
        // the recipe would exceed the budget, although the net input fits.
        state.repeat_change(&mut editor, Some(500_000)).unwrap();
        assert_eq!(
            editor.buffer().to_text().matches('X').count(),
            if mode == 'R' { 500_000 } else { 500_001 }
        );
        assert!(!editor.buffer().to_text().contains('🙂'));
    }
}

#[test]
fn identical_replace_dot_publishes_explicit_count_and_saved_state() {
    for (typed, original) in [("X", "abc\nXbc\nzzz\nzzz"), ("中", "abc\n中bc\nzzz\nzzz")] {
        let directory = tempfile::tempdir().unwrap();
        let mut editor = Editor::from_buffer(TextBuffer::from_text(original).unwrap());
        let mut state = VisualState::default();
        let mut input = state.begin_counted_insert(&mut editor, 'R', 2).unwrap();
        input.paste(typed).unwrap();
        input.commit(&mut state).unwrap();
        editor
            .write_to(Some(&directory.path().join("text")))
            .unwrap();
        let before = editor.buffer().clone();
        state.row = 1;
        state.column = 0;
        let saved = editor.clone();
        let visual = format!("{state:?}");
        assert!(state.repeat_change(&mut editor, Some(usize::MAX)).is_err());
        assert_eq!(editor, saved);
        assert_eq!(format!("{state:?}"), visual);
        state.repeat_change(&mut editor, Some(1)).unwrap();
        assert_eq!(editor.buffer(), &before);
        assert!(editor.is_modified());
        for _ in 0..2 {
            state.undo(&mut editor).unwrap();
            assert_eq!(editor.buffer(), &before);
            assert_eq!(state.row, 1);
        }
        state.row = 2;
        state.column = 0;
        state.repeat_change(&mut editor, None).unwrap();
        assert_eq!(editor.buffer().line(2), Some(format!("{typed}zz").as_str()));
        // The legacy input API must publish identical R as well.
        let before = editor.buffer().clone();
        state.row = 1;
        state.column = 0;
        let mut edit = state.begin_insert(&mut editor, 'R').unwrap();
        edit.replace_input_text(typed).unwrap();
        state.commit_input(edit, InputStart::Insert('R')).unwrap();
        state.undo(&mut editor).unwrap();
        assert_eq!(editor.buffer(), &before);
    }
}

#[test]
fn unchanged_c_entry_normalizes_corrections_and_preserves_failures() {
    let (directory, mut editor, mut state) = fixture(true, "\n");
    state.column = 1;
    let before = editor.buffer().clone();
    let mut input = state.begin_counted_insert(&mut editor, 'C', 1).unwrap();
    input.paste("🙂CX").unwrap();
    input.apply_input(&InputAction::Backspace).unwrap();
    input.commit(&mut state).unwrap();
    assert_eq!(editor.buffer(), &before);
    assert!(editor.is_modified());
    assert_eq!(
        editor.unnamed_register(),
        Some(&Register::Characters("🙂C".into()))
    );
    state.undo(&mut editor).unwrap();
    assert_eq!(editor.buffer(), &before);
    let published = editor.clone();
    let visual = format!("{state:?}");
    assert!(state.repeat_change(&mut editor, Some(usize::MAX)).is_err());
    assert_eq!(editor, published);
    assert_eq!(format!("{state:?}"), visual);
    state.row = 2;
    state.column = 1;
    state.repeat_change(&mut editor, None).unwrap();
    assert_eq!(editor.buffer().line(2), Some("Z🙂C"));
    assert_saved_text(&editor, &directory);
}
