use vir::{
    buffer::TextBuffer,
    editor::{Editor, InputAction, Register},
    visual::{InputStart, Motion, VisualState},
};

fn editor(text: &str) -> Editor {
    Editor::from_buffer(TextBuffer::from_text(text).unwrap())
}

fn input(state: &mut VisualState, editor: &mut Editor, start: InputStart, actions: &[InputAction]) {
    let mut edit = match start {
        InputStart::Insert(mode) => state.begin_insert(editor, mode).unwrap(),
        InputStart::Change { motion, count } => state.begin_change(editor, motion, count).unwrap(),
        InputStart::ChangeNamed {
            motion,
            count,
            register,
        } => state
            .begin_change_register(editor, motion, count, Some(register))
            .unwrap(),
    };
    for action in actions {
        edit.apply_input(action).unwrap();
    }
    state.commit_input(edit, start).unwrap();
}

#[test]
fn counted_delete_reuses_then_replaces_count_and_preserves_one_undo() {
    let mut editor = editor("a b c d e f g h");
    let mut state = VisualState::default();
    state
        .delete_motion(&mut editor, Motion::WordForward, Some(2))
        .unwrap();
    assert_eq!(editor.buffer().to_text(), "c d e f g h");
    state.repeat_change(&mut editor, None).unwrap();
    assert_eq!(editor.buffer().to_text(), "e f g h");
    state.repeat_change(&mut editor, Some(1)).unwrap();
    assert_eq!(editor.buffer().to_text(), "f g h");
    state.repeat_change(&mut editor, None).unwrap();
    assert_eq!(editor.buffer().to_text(), "g h");
    editor.undo().unwrap();
    assert_eq!(editor.buffer().to_text(), "f g h");
    editor.undo().unwrap();
    assert_eq!(editor.buffer().to_text(), "g h");
    assert_eq!(
        editor.unnamed_register(),
        Some(&Register::Characters("f ".into()))
    );
}

#[test]
fn line_deletion_and_line_end_motion_repeat_at_new_location() {
    let mut editor = editor("a\nb\nc\nd\ne\nf");
    let mut state = VisualState::default();
    state.delete_lines(&mut editor, 2).unwrap();
    state.repeat_change(&mut editor, Some(1)).unwrap();
    assert_eq!(editor.buffer().to_text(), "d\ne\nf");
    state.repeat_change(&mut editor, None).unwrap();
    assert_eq!(editor.buffer().to_text(), "e\nf");
    editor.undo().unwrap();
    assert_eq!(editor.buffer().to_text(), "d\ne\nf");
    let mut editor = self::editor("one\ntwo\nthree");
    state.column = 1;
    state.delete_motion(&mut editor, Motion::End, None).unwrap();
    state.move_cursor(Motion::Down, editor.buffer(), 8);
    state.move_cursor(Motion::Right, editor.buffer(), 8);
    state.repeat_change(&mut editor, Some(2)).unwrap();
    assert_eq!(editor.buffer().to_text(), "o\nt");
    editor.undo().unwrap();
    assert_eq!(editor.buffer().to_text(), "o\ntwo\nthree");
}

#[test]
fn insert_modes_repeat_logical_input_and_count_without_compounding() {
    for (mode, initial, first, repeated) in [
        ('i', "ab", "Xab", "XXXab"),
        ('a', "ab", "aXb", "aXXXb"),
        ('I', "  ab", "  Xab", "  XXXab"),
        ('A', "ab", "abX", "abXXX"),
        ('o', "ab", "ab\nX\n", "ab\nX\nX\n"),
        ('O', "ab", "X\nab\n", "X\nX\nab\n"),
    ] {
        let mut editor = editor(initial);
        let mut state = VisualState::default();
        input(
            &mut state,
            &mut editor,
            InputStart::Insert(mode),
            &[InputAction::Insert("X".into())],
        );
        assert_eq!(editor.buffer().to_text(), first, "{mode}");
        let position = (state.row, state.column);
        state.repeat_change(&mut editor, Some(2)).unwrap();
        assert_eq!(editor.buffer().to_text(), repeated, "{mode}");
        editor.undo().unwrap();
        assert_eq!(editor.buffer().to_text(), first);
        (state.row, state.column) = position;
        state.repeat_change(&mut editor, None).unwrap();
        assert_eq!(
            editor.buffer().to_text(),
            repeated,
            "repeat count must stay two for {mode}"
        );
    }
}

#[test]
fn input_deletion_newlines_and_unicode_replay_as_one_transaction() {
    let mut editor = editor("abc\ndef");
    let mut state = VisualState::default();
    state.column = 1;
    let actions = [
        InputAction::Backspace,
        InputAction::Insert("中\n文".into()),
        InputAction::Delete,
        InputAction::Insert("!".into()),
    ];
    input(&mut state, &mut editor, InputStart::Insert('i'), &actions);
    assert_eq!(editor.buffer().to_text(), "中\n文!c\ndef");
    state.row = 2;
    state.column = 1;
    state.repeat_change(&mut editor, None).unwrap();
    assert_eq!(editor.buffer().to_text(), "中\n文!c\n中\n文!f");
    editor.undo().unwrap();
    assert_eq!(editor.buffer().to_text(), "中\n文!c\ndef");
}

#[test]
fn change_count_overrides_range_and_types_once_including_c_alias() {
    let mut editor = editor("one two three four");
    let mut state = VisualState::default();
    input(
        &mut state,
        &mut editor,
        InputStart::Change {
            motion: Some(Motion::WordForward),
            count: None,
        },
        &[InputAction::Insert("X".into())],
    );
    assert_eq!(editor.buffer().to_text(), "X two three four");
    state.move_cursor(Motion::WordForward, editor.buffer(), 8);
    state.repeat_change(&mut editor, Some(2)).unwrap();
    assert_eq!(editor.buffer().to_text(), "X X four");
    editor.undo().unwrap();
    assert_eq!(editor.buffer().to_text(), "X two three four");
    let mut editor = self::editor("abc\ndef\nghi");
    let mut state = VisualState::default();
    state.column = 1;
    input(
        &mut state,
        &mut editor,
        InputStart::Insert('C'),
        &[InputAction::Insert("X".into())],
    );
    state.move_cursor(Motion::Down, editor.buffer(), 8);
    state.repeat_change(&mut editor, Some(2)).unwrap();
    assert_eq!(editor.buffer().to_text(), "aX\ndX");
    editor.undo().unwrap();
    assert_eq!(editor.buffer().to_text(), "aX\ndef\nghi");
}

#[test]
fn replace_mode_replays_restoration_and_counted_replacement() {
    let mut editor = editor("abcdef\nghijkl");
    let mut state = VisualState::default();
    input(
        &mut state,
        &mut editor,
        InputStart::Insert('R'),
        &[
            InputAction::Replace('中'),
            InputAction::Replace('文'),
            InputAction::RestoreReplacement,
            InputAction::Replace('!'),
        ],
    );
    assert_eq!(editor.buffer().to_text(), "中!cdef\nghijkl");
    state.row = 1;
    state.column = 0;
    state.repeat_change(&mut editor, Some(2)).unwrap();
    assert_eq!(editor.buffer().to_text(), "中!cdef\n中!中!ijkl");
    editor.undo().unwrap();
    assert_eq!(editor.buffer().to_text(), "中!cdef\nghijkl");
    state.column = 0;
    state.replace_character(&mut editor, 'Z').unwrap();
    state.column = 2;
    state.repeat_change(&mut editor, Some(3)).unwrap();
    assert_eq!(editor.buffer().line(1), Some("ZhZZZl"));
    assert_eq!(state.column, 4);
    editor.undo().unwrap();
    assert_eq!(editor.buffer().line(1), Some("Zhijkl"));
}

#[test]
fn puts_use_current_register_and_join_repeat_is_one_undo() {
    let mut editor = editor("a b c");
    let mut state = VisualState::default();
    state.yank(&mut editor, Some(Motion::Right), None).unwrap();
    state.put(&mut editor, false, 1).unwrap();
    assert_eq!(editor.buffer().to_text(), "aa b c");
    state.move_cursor(Motion::WordForward, editor.buffer(), 8);
    state.yank(&mut editor, Some(Motion::Right), None).unwrap();
    state.repeat_change(&mut editor, Some(2)).unwrap();
    assert_eq!(editor.buffer().to_text(), "aa bbb c");
    editor.undo().unwrap();
    assert_eq!(editor.buffer().to_text(), "aa b c");
    let mut editor = self::editor("a\nb\nc\nd\ne\nf");
    let mut state = VisualState::default();
    state.join_lines(&mut editor, 2).unwrap();
    state.move_cursor(Motion::Down, editor.buffer(), 8);
    state.repeat_change(&mut editor, Some(3)).unwrap();
    assert_eq!(editor.buffer().to_text(), "a b\nc d e\nf");
    editor.undo().unwrap();
    assert_eq!(editor.buffer().to_text(), "a b\nc\nd\ne\nf");
}

#[test]
fn failed_and_cancelled_edits_keep_the_previous_record() {
    let mut editor = editor("abc\nx");
    let mut state = VisualState::default();
    let initial = editor.clone();
    assert!(state.repeat_change(&mut editor, None).is_err());
    assert_eq!(editor, initial);
    state.replace_character(&mut editor, 'Z').unwrap();
    let before = editor.clone();
    drop(state.begin_insert(&mut editor, 'o').unwrap());
    assert_eq!(editor, before);
    state.row = 1;
    state.column = 0;
    let before = editor.clone();
    let visual = format!("{state:?}");
    assert!(state.repeat_change(&mut editor, Some(3)).is_err());
    assert_eq!(editor, before);
    assert_eq!(format!("{state:?}"), visual);
    state.repeat_change(&mut editor, None).unwrap();
    assert_eq!(editor.buffer().to_text(), "Zbc\nZ");
    editor.undo().unwrap();
    assert_eq!(editor.buffer().to_text(), "Zbc\nx");
}

#[test]
fn huge_input_count_fails_atomically_and_does_not_replace_record() {
    let mut editor = editor("a");
    let mut state = VisualState::default();
    input(
        &mut state,
        &mut editor,
        InputStart::Insert('a'),
        &[InputAction::Insert("中".into())],
    );
    let before = editor.clone();
    let visual = format!("{state:?}");
    assert!(state.repeat_change(&mut editor, Some(usize::MAX)).is_err());
    assert_eq!(editor, before);
    assert_eq!(format!("{state:?}"), visual);
    state.repeat_change(&mut editor, None).unwrap();
    assert_eq!(editor.buffer().to_text(), "a中中");
}

#[test]
fn search_undo_save_and_file_switch_preserve_visual_repeat() {
    let directory = tempfile::tempdir().unwrap();
    let first = directory.path().join("first");
    let second = directory.path().join("second");
    std::fs::write(&first, "one two").unwrap();
    std::fs::write(&second, "three four").unwrap();
    let mut editor = Editor::default();
    let mut state = VisualState::default();
    state
        .execute_ex(&format!("e {}", first.display()), &mut editor)
        .unwrap();
    state
        .delete_motion(&mut editor, Motion::WordForward, None)
        .unwrap();
    state.execute_ex("u", &mut editor).unwrap();
    state.search(&mut editor, "two", true).unwrap();
    state.execute_ex("w", &mut editor).unwrap();
    state
        .execute_ex(&format!("e {}", second.display()), &mut editor)
        .unwrap();
    state.repeat_change(&mut editor, None).unwrap();
    assert_eq!(editor.buffer().to_text(), "four");
    state.execute_ex("w", &mut editor).unwrap();
    assert_eq!(std::fs::read_to_string(second).unwrap(), "four");
}

#[test]
fn change_without_input_repeats_deletion_and_line_changes_keep_one_transaction() {
    let mut editor = editor("one two three");
    let mut state = VisualState::default();
    input(
        &mut state,
        &mut editor,
        InputStart::Change {
            motion: Some(Motion::WordForward),
            count: None,
        },
        &[],
    );
    assert_eq!(editor.buffer().to_text(), " two three");
    state.move_cursor(Motion::WordForward, editor.buffer(), 8);
    state.repeat_change(&mut editor, None).unwrap();
    assert_eq!(editor.buffer().to_text(), "  three");
    let mut editor = self::editor("a\nb\nc\nd");
    let mut state = VisualState::default();
    input(
        &mut state,
        &mut editor,
        InputStart::Change {
            motion: None,
            count: Some(1),
        },
        &[InputAction::Insert("X".into())],
    );
    state.move_cursor(Motion::Down, editor.buffer(), 8);
    state.repeat_change(&mut editor, Some(2)).unwrap();
    assert_eq!(editor.buffer().to_text(), "X\nX\nd");
    editor.undo().unwrap();
    assert_eq!(editor.buffer().to_text(), "X\nb\nc\nd");
}

#[test]
fn x_and_line_put_repeat_while_empty_register_failures_keep_previous_change() {
    let mut editor = editor("abcd");
    let mut state = VisualState::default();
    state.delete_character(&mut editor).unwrap();
    state.repeat_change(&mut editor, Some(2)).unwrap();
    assert_eq!(editor.buffer().to_text(), "d");
    assert_eq!(
        editor.unnamed_register(),
        Some(&Register::Characters("bc".into()))
    );
    let mut editor = self::editor("a\nb\n");
    let mut state = VisualState::default();
    state.yank(&mut editor, None, None).unwrap();
    state.put(&mut editor, true, 1).unwrap();
    state.repeat_change(&mut editor, Some(2)).unwrap();
    assert_eq!(editor.buffer().to_text(), "a\na\na\na\nb\n");
    editor.undo().unwrap();
    assert_eq!(editor.buffer().to_text(), "a\na\nb\n");
    // Replay against an editor with no register must not publish partial state.
    let mut empty_register = self::editor("xyz");
    let before = empty_register.clone();
    let visual = format!("{state:?}");
    assert!(state.repeat_change(&mut empty_register, None).is_err());
    assert_eq!(empty_register, before);
    assert_eq!(format!("{state:?}"), visual);
}

#[test]
fn explicit_g_counts_and_replace_newlines_keep_replay_semantics() {
    let mut editor = editor("a\nb\nc\nd");
    let mut state = VisualState::default();
    state.row = 2;
    state
        .delete_motion(&mut editor, Motion::Last, Some(2))
        .unwrap();
    assert_eq!(editor.buffer().to_text(), "a\nd");
    state.execute_ex("u", &mut editor).unwrap();
    state.row = 2;
    state.repeat_change(&mut editor, None).unwrap();
    assert_eq!(editor.buffer().to_text(), "a\nd");
    let mut editor = self::editor("abc\ndef");
    let mut state = VisualState::default();
    input(
        &mut state,
        &mut editor,
        InputStart::Insert('R'),
        &[
            InputAction::Replace('中'),
            InputAction::Replace('\n'),
            InputAction::Replace('X'),
            InputAction::RestoreReplacement,
        ],
    );
    assert_eq!(editor.buffer().to_text(), "中\nbc\ndef");
    state.row = 2;
    state.column = 0;
    state.repeat_change(&mut editor, None).unwrap();
    assert_eq!(editor.buffer().to_text(), "中\nbc\n中\nef");
    editor.undo().unwrap();
    assert_eq!(editor.buffer().to_text(), "中\nbc\ndef");
}
