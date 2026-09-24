use vir::{
    buffer::TextBuffer,
    editor::{Editor, InputAction},
    visual::{Motion, VisualState},
};

fn editor(text: &str) -> Editor {
    Editor::from_buffer(TextBuffer::from_text(text).unwrap())
}
fn insert(state: &mut VisualState, editor: &mut Editor, mode: char, count: usize, text: &str) {
    let mut input = state.begin_counted_insert(editor, mode, count).unwrap();
    input
        .apply_input(&InputAction::Insert(text.into()))
        .unwrap();
    input.commit(state).unwrap();
}

#[test]
fn entry_positions_unicode_single_undo_and_register_preservation() {
    for (mode, expected) in [
        ('i', "  字🙂字🙂abc"),
        ('a', "  a字🙂字🙂bc"),
        ('I', "  字🙂字🙂abc"),
        ('A', "  abc字🙂字🙂"),
    ] {
        for lf in ["", "\n"] {
            let original = format!("  abc{lf}");
            let mut editor = editor(&original);
            let mut state = VisualState::default();
            state.yank(&mut editor, Some(Motion::End), None).unwrap();
            let register = editor.unnamed_register().cloned();
            state.column = 2;
            insert(&mut state, &mut editor, mode, 2, "字🙂");
            assert_eq!(editor.buffer().to_text(), format!("{expected}{lf}"));
            assert_eq!(editor.unnamed_register(), register.as_ref());
            editor.undo().unwrap();
            assert_eq!(editor.buffer().to_text(), original);
        }
    }
}

#[test]
fn first_newline_consumes_count_in_live_draft_and_dot() {
    let mut editor = editor("abc");
    let mut state = VisualState::default();
    let mut input = state.begin_counted_insert(&mut editor, 'i', 3).unwrap();
    input.apply_input(&InputAction::Insert("X".into())).unwrap();
    assert_eq!(input.buffer().to_text(), "Xabc");
    input
        .apply_input(&InputAction::Insert("\nY\nZ".into()))
        .unwrap();
    assert_eq!(input.buffer().to_text(), "XXX\nY\nZabc");
    input.commit(&mut state).unwrap();
    state.row = 0;
    state.column = 0;
    state.repeat_change(&mut editor, Some(2)).unwrap();
    assert_eq!(editor.buffer().to_text(), "XX\nY\nZXXX\nY\nZabc");
    editor.undo().unwrap();
    assert_eq!(editor.buffer().to_text(), "XXX\nY\nZabc");
}

#[test]
fn backspace_repeats_net_input_but_original_deletions_happen_once() {
    let mut editor = editor("abc");
    let mut state = VisualState::default();
    state.column = 1;
    let mut input = state.begin_counted_insert(&mut editor, 'i', 3).unwrap();
    for action in [
        InputAction::Backspace,
        InputAction::Delete,
        InputAction::Insert("字🙂".into()),
        InputAction::Backspace,
        InputAction::Insert("X".into()),
    ] {
        input.apply_input(&action).unwrap();
    }
    input.commit(&mut state).unwrap();
    assert_eq!(editor.buffer().to_text(), "字X字X字Xc");
    editor.undo().unwrap();
    assert_eq!(editor.buffer().to_text(), "abc");
    state.column = 1;
    state.repeat_change(&mut editor, None).unwrap();
    assert_eq!(editor.buffer().to_text(), "字X字X字Xc");
}

#[test]
fn dot_overrides_count_without_squaring_and_preserves_effective_count() {
    let mut editor = editor("abc");
    let mut state = VisualState::default();
    insert(&mut state, &mut editor, 'A', 3, "X");
    state.repeat_change(&mut editor, None).unwrap();
    assert_eq!(editor.buffer().to_text(), "abcXXXXXX");
    state.repeat_change(&mut editor, Some(2)).unwrap();
    state.repeat_change(&mut editor, None).unwrap();
    assert_eq!(editor.buffer().to_text(), "abcXXXXXXXXXX");
    let before = editor.clone();
    assert!(state.repeat_change(&mut editor, Some(usize::MAX)).is_err());
    assert_eq!(editor, before);
}

#[test]
fn uncounted_multiline_dot_uses_historical_first_line_count() {
    let mut editor = editor("abc");
    let mut state = VisualState::default();
    insert(&mut state, &mut editor, 'i', 1, "X\nY");
    state.row = 0;
    state.column = 0;
    state.repeat_change(&mut editor, Some(2)).unwrap();
    assert_eq!(editor.buffer().to_text(), "XX\nYX\nYabc");
}

#[test]
fn budget_errors_are_atomic_and_backspace_can_correct_at_limit() {
    let mut editor = editor("abc");
    let mut state = VisualState::default();
    assert!(
        state
            .begin_counted_insert(&mut editor, 'i', 1_000_001)
            .is_err()
    );
    let mut input = state
        .begin_counted_insert(&mut editor, 'i', 500_000)
        .unwrap();
    input.apply_input(&InputAction::Insert("X".into())).unwrap();
    assert!(input.paste("Y").is_err());
    assert_eq!(input.buffer().to_text(), "Xabc");
    input.apply_input(&InputAction::Backspace).unwrap();
    assert_eq!(input.buffer().to_text(), "abc");
    input.apply_input(&InputAction::Insert("Y".into())).unwrap();
    input.commit(&mut state).unwrap();
    assert_eq!(
        editor.buffer().to_text(),
        format!("{}abc", "Y".repeat(500_000))
    );
    editor.undo().unwrap();
    assert_eq!(editor.buffer().to_text(), "abc");
}

#[test]
fn empty_cancelled_and_pasted_sessions_use_one_transaction() {
    let mut editor = editor("");
    let mut state = VisualState::default();
    let before = editor.clone();
    let mut input = state.begin_counted_insert(&mut editor, 'i', 3).unwrap();
    input.apply_input(&InputAction::Backspace).unwrap();
    input.apply_input(&InputAction::Delete).unwrap();
    input.commit(&mut state).unwrap();
    assert_eq!(editor.buffer(), before.buffer());
    assert!(editor.is_modified());
    let before = editor.clone();
    {
        let mut input = state.begin_counted_insert(&mut editor, 'i', 3).unwrap();
        input.paste("X\r\nY").unwrap();
        assert_eq!(input.buffer().to_text(), "XXX\nY\n");
    }
    assert_eq!(editor, before);
    let mut input = state.begin_counted_insert(&mut editor, 'i', 3).unwrap();
    input.paste("X\r\nY").unwrap();
    input.commit(&mut state).unwrap();
    assert_eq!(editor.buffer().to_text(), "XXX\nY\n");
    editor.undo().unwrap();
    assert_eq!(editor.buffer().to_text(), "");
}

#[test]
fn backspace_after_first_newline_remains_available_at_counted_budget_limit() {
    for mode in ['i', 'R'] {
        let mut editor = editor("abc");
        let mut state = VisualState::default();
        let mut input = state
            .begin_counted_insert(&mut editor, mode, 333_333)
            .unwrap();
        input.paste("X\n").unwrap();
        assert_eq!(input.buffer().line_count(), 2);
        let erase = if mode == 'R' {
            InputAction::RestoreReplacement
        } else {
            InputAction::Backspace
        };
        input.apply_input(&erase).unwrap();
        assert_eq!(input.buffer().line_count(), 1);
        input.apply_input(&erase).unwrap();
        input.commit(&mut state).unwrap();
        let expected = format!(
            "{}{}",
            "X".repeat(333_332),
            if mode == 'R' { "bc" } else { "abc" }
        );
        assert_eq!(editor.buffer().to_text(), expected);
        editor.undo().unwrap();
        assert_eq!(editor.buffer().to_text(), "abc");
        state.row = 0;
        state.column = 0;
        state.repeat_change(&mut editor, None).unwrap();
        assert_eq!(editor.buffer().to_text(), expected);
    }
}

#[test]
fn original_text_and_cross_line_backspace_remain_explicit_extensions() {
    let mut editor = editor("ab\ncd");
    let mut state = VisualState::default();
    state.row = 1;
    let mut input = state.begin_counted_insert(&mut editor, 'i', 3).unwrap();
    input.apply_input(&InputAction::Backspace).unwrap(); // join original lines
    input.apply_input(&InputAction::Backspace).unwrap(); // erase original b
    input.paste("X").unwrap();
    input.commit(&mut state).unwrap();
    assert_eq!(editor.buffer().to_text(), "aXXXcd");
    editor.undo().unwrap();
    assert_eq!(editor.buffer().to_text(), "ab\ncd");
    state.row = 1;
    state.column = 0;
    state.repeat_change(&mut editor, None).unwrap();
    assert_eq!(editor.buffer().to_text(), "aXXXcd");
}

#[test]
fn no_op_replace_backspace_finishes_an_empty_session() {
    let mut editor = editor("abc");
    let mut state = VisualState::default();
    state.replace_character(&mut editor, 'Z').unwrap();
    let before = editor.clone();
    state.column = 1;
    let mut input = state
        .begin_counted_insert(&mut editor, 'R', 1_000_000)
        .unwrap();
    input.apply_input(&InputAction::RestoreReplacement).unwrap();
    input.commit(&mut state).unwrap();
    assert_eq!(editor.buffer(), before.buffer());
    state.column = 1;
    state.repeat_change(&mut editor, None).unwrap();
    assert_eq!(editor.buffer().to_text(), "Zbc");
    assert_eq!(state.column, 0);
    state.undo(&mut editor).unwrap();
    assert_eq!(editor.buffer().to_text(), "Zbc");
}
