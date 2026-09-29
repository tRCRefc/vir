use vir::{
    buffer::TextBuffer,
    editor::{Editor, InputAction},
    ex::{command::execute, parser::parse},
    visual::{InputStart, Motion, VisualState, paste_input},
};

fn editor(text: &str) -> Editor {
    Editor::from_buffer(TextBuffer::from_text(text).unwrap())
}

#[test]
fn normal_paste_matches_reference_placement_and_dot_without_touching_registers() {
    for (column, once, twice) in [
        (0, "XYabcdef\n", "XYXYabcdef\n"),
        (1, "abXYcdef\n", "abXYXYcdef\n"),
    ] {
        let mut editor = editor("abcdef\n");
        let mut state = VisualState::default();
        state.yank(&mut editor, Some(Motion::End), None).unwrap();
        let register = editor.unnamed_register().cloned();
        state.column = column;
        state.paste(&mut editor, "XY").unwrap();
        assert_eq!(editor.buffer().to_text(), once);
        assert_eq!(editor.unnamed_register(), register.as_ref());
        state.repeat_change(&mut editor, None).unwrap();
        assert_eq!(editor.buffer().to_text(), twice);
        editor.undo().unwrap();
        assert_eq!(editor.buffer().to_text(), once);
        editor.undo().unwrap();
        assert_eq!(editor.buffer().to_text(), twice);
    }
}

#[test]
fn insert_paste_preserves_literal_controls_normalizes_newlines_and_shares_undo() {
    let mut editor = editor("tail");
    let mut state = VisualState::default();
    let mut edit = state.begin_insert(&mut editor, 'i').unwrap();
    edit.insert("typed").unwrap();
    paste_input(&mut edit, false, "中\r\n\t\x1b:q!\r\x03\x08\x7f\0").unwrap();
    edit.insert("end").unwrap();
    state.commit_input(edit, InputStart::Insert('i')).unwrap();
    let expected = "typed中\n\t\x1b:q!\n\x03\x08\x7f\0endtail";
    assert_eq!(editor.buffer().to_text(), expected);
    editor.undo().unwrap();
    assert_eq!(editor.buffer().to_text(), "tail");
    editor.undo().unwrap();
    assert_eq!(editor.buffer().to_text(), expected);
    let mut fresh = self::editor("tail");
    state.row = 0;
    state.column = 0;
    state.repeat_change(&mut fresh, None).unwrap();
    assert_eq!(fresh.buffer().to_text(), expected);
}

#[test]
fn change_and_paste_commit_as_one_command_and_dot_reuses_motion() {
    let mut editor = editor("one two three\n");
    let mut state = VisualState::default();
    let start = InputStart::Change {
        motion: Some(Motion::WordForward),
        count: None,
    };
    let mut edit = state
        .begin_change(&mut editor, Some(Motion::WordForward), None)
        .unwrap();
    paste_input(&mut edit, false, "中").unwrap();
    state.commit_input(edit, start).unwrap();
    assert_eq!(editor.buffer().to_text(), "中 two three\n");
    state.move_cursor(Motion::WordForward, editor.buffer(), 8);
    state.repeat_change(&mut editor, None).unwrap();
    assert_eq!(editor.buffer().to_text(), "中 中 three\n");
    editor.undo().unwrap();
    assert_eq!(editor.buffer().to_text(), "中 two three\n");
}

#[test]
fn bulk_replace_matches_existing_r_input_and_every_backspace_boundary() {
    for original in ["", "\n", "abcdef\nlast", "a中z", "abc"] {
        for payload in ["X\nY", "中\t🙂\n\nlong", "XYZ", "\n"] {
            let mut bulk_editor = editor(original);
            let mut typed_editor = editor(original);
            let state = VisualState::default();
            let mut bulk = state.begin_insert(&mut bulk_editor, 'R').unwrap();
            let mut typed = state.begin_insert(&mut typed_editor, 'R').unwrap();
            paste_input(&mut bulk, true, payload).unwrap();
            for c in payload.chars() {
                typed.replace_input(c).unwrap();
            }
            assert_eq!(bulk.buffer(), typed.buffer(), "{original:?} / {payload:?}");
            assert_eq!(bulk.cursor(), typed.cursor());
            for _ in payload.chars() {
                bulk.replace_backspace().unwrap();
                typed.replace_backspace().unwrap();
                assert_eq!(bulk.buffer(), typed.buffer());
                assert_eq!(bulk.cursor(), typed.cursor());
            }
            bulk.commit().unwrap();
            typed.commit().unwrap();
            assert_eq!(bulk_editor, typed_editor);
            assert_eq!(bulk_editor.buffer().to_text(), original);
        }
    }
}

#[test]
fn replace_literal_paste_backspace_dot_and_repeat_limits_are_transactional() {
    let mut editor = editor("abcdef\n");
    let mut state = VisualState::default();
    let mut edit = state.begin_insert(&mut editor, 'R').unwrap();
    paste_input(&mut edit, true, "X\nY\x1b\x03").unwrap();
    edit.replace_backspace().unwrap();
    state.commit_input(edit, InputStart::Insert('R')).unwrap();
    assert_eq!(editor.buffer().to_text(), "X\nY\x1bdef\n");
    editor.undo().unwrap();
    assert_eq!(editor.buffer().to_text(), "abcdef\n");
    state.row = 0;
    state.column = 0;
    state.repeat_change(&mut editor, None).unwrap();
    assert_eq!(editor.buffer().to_text(), "X\nY\x1bdef\n");
    let before = editor.clone();
    let visual = format!("{state:?}");
    assert!(state.repeat_change(&mut editor, Some(usize::MAX)).is_err());
    assert_eq!(editor, before);
    assert_eq!(format!("{state:?}"), visual);
}

#[test]
fn empty_rejected_and_cancelled_pastes_preserve_state_and_previous_repeat() {
    let mut editor = editor("abc");
    let mut state = VisualState::default();
    state.delete_character(&mut editor).unwrap();
    let before = editor.clone();
    let visual = format!("{state:?}");
    state.paste(&mut editor, "").unwrap();
    assert!(state.paste(&mut editor, &"x".repeat(65_537)).is_err());
    assert_eq!(editor, before);
    assert_eq!(format!("{state:?}"), visual);
    for mode in ['i', 'R'] {
        let mut edit = state.begin_insert(&mut editor, mode).unwrap();
        let buffer = edit.buffer().clone();
        let cursor = edit.cursor();
        assert!(paste_input(&mut edit, mode == 'R', &"x".repeat(65_537)).is_err());
        assert_eq!(edit.buffer(), &buffer);
        assert_eq!(edit.cursor(), cursor);
        paste_input(&mut edit, mode == 'R', "cancelled").unwrap();
        drop(edit);
        assert_eq!(editor, before);
    }
    state.repeat_change(&mut editor, None).unwrap();
    assert_eq!(editor.buffer().to_text(), "c");
}

#[test]
fn multiline_empty_and_no_final_lf_paste_preserve_buffer_conventions() {
    for (original, pasted, expected) in [
        ("", "中", "中\n"),
        ("tail", "X\r\nY", "X\nYtail"),
        ("tail\n", "X\n", "X\ntail\n"),
    ] {
        let mut editor = editor(original);
        let mut state = VisualState::default();
        state.paste(&mut editor, pasted).unwrap();
        assert_eq!(editor.buffer().to_text(), expected);
        editor.undo().unwrap();
        assert_eq!(editor.buffer().to_text(), original);
    }
}

#[test]
fn maximum_replace_paste_is_repeatable_with_byte_accounting() {
    let mut editor = Editor::default();
    let mut state = VisualState::default();
    let text = "x".repeat(65_536);
    let mut edit = state.begin_insert(&mut editor, 'R').unwrap();
    paste_input(&mut edit, true, &text).unwrap();
    state.commit_input(edit, InputStart::Insert('R')).unwrap();
    let before = editor.clone();
    assert!(state.repeat_change(&mut editor, Some(16)).is_err());
    assert_eq!(editor, before);
    state.repeat_change(&mut editor, Some(1)).unwrap();
    assert_eq!(editor.buffer().line(0).unwrap().len(), 131_071);
}

#[test]
fn autoindent_never_rewrites_paste_in_normal_input_or_counted_input() {
    for enabled in [false, true] {
        for entry in ['n', 'i', 'a', 'R'] {
            for count in [1, 3] {
                let mut editor = editor("  base\n  tail\n");
                if enabled {
                    execute(parse("set ai").unwrap(), &mut editor).unwrap();
                }
                let mut state = VisualState::default();
                state.column = 2;
                let payload = "字\r\n\tX\nY";
                if entry == 'n' {
                    state.paste(&mut editor, payload).unwrap();
                } else {
                    let mut session = state
                        .begin_counted_insert(&mut editor, entry, count)
                        .unwrap();
                    session.paste(payload).unwrap();
                    session.commit(&mut state).unwrap();
                }
                let expected = match entry {
                    'n' => "  b字\n\tX\nYase\n  tail\n".to_owned(),
                    'a' => format!("  b{}\n\tX\nYase\n  tail\n", "字".repeat(count)),
                    'R' => format!("  {}\n\tX\nY\n  tail\n", "字".repeat(count)),
                    _ => format!("  {}\n\tX\nYbase\n  tail\n", "字".repeat(count)),
                };
                assert_eq!(
                    editor.buffer().to_text(),
                    expected,
                    "{entry}/{count}/{enabled}"
                );
                editor.undo().unwrap();
                assert_eq!(editor.buffer().to_text(), "  base\n  tail\n");
                // Replay must stay literal even when ai changed after the paste.
                execute(
                    parse(if enabled { "set noai" } else { "set ai" }).unwrap(),
                    &mut editor,
                )
                .unwrap();
                state.row = 0;
                state.column = 2;
                state.repeat_change(&mut editor, None).unwrap();
                assert_eq!(editor.buffer().to_text(), expected);
            }
        }
    }
}

#[test]
fn mixed_typed_and_pasted_newlines_keep_their_meaning_on_dot() {
    for count in [1, 3] {
        let mut editor = editor("  base");
        execute(parse("set ai").unwrap(), &mut editor).unwrap();
        let mut state = VisualState::default();
        let mut session = state.begin_counted_insert(&mut editor, 'A', count).unwrap();
        session.paste("X").unwrap();
        session.apply_input(&InputAction::Backspace).unwrap();
        session
            .apply_input(&InputAction::Insert("T\nU".into()))
            .unwrap();
        session.paste("\n\tV").unwrap();
        session.commit(&mut state).unwrap();
        let expected = format!("  base{}\n  U\n\tV", "T".repeat(count));
        assert_eq!(editor.buffer().to_text(), expected);
        editor.undo().unwrap();
        state.row = 0;
        state.column = 0;
        state.repeat_change(&mut editor, None).unwrap();
        assert_eq!(editor.buffer().to_text(), expected);
    }
}

#[test]
fn direct_input_paste_remains_literal_in_replay_and_checks_expansion_budget() {
    let mut editor = editor("  base");
    execute(parse("set ai").unwrap(), &mut editor).unwrap();
    let mut state = VisualState::default();
    let mut edit = state.begin_insert(&mut editor, 'A').unwrap();
    paste_input(&mut edit, false, "\nchild").unwrap();
    state.commit_input(edit, InputStart::Insert('A')).unwrap();
    assert_eq!(editor.buffer().to_text(), "  base\nchild");
    editor.undo().unwrap();
    state.row = 0;
    state.column = 0;
    state.repeat_change(&mut editor, None).unwrap();
    assert_eq!(editor.buffer().to_text(), "  base\nchild");
    let before = editor.clone();
    assert!(state.repeat_change(&mut editor, Some(1_000_000)).is_err());
    assert_eq!(editor, before);
}
