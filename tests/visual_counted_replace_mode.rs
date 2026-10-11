use vir::{
    buffer::TextBuffer,
    editor::{Editor, InputAction},
    visual::{Motion, VisualState},
};
fn editor(text: &str) -> Editor {
    Editor::from_buffer(TextBuffer::from_text(text).unwrap())
}
#[test]
fn count_overwrites_once_then_inserts_net_unicode_copies() {
    for original in ["abcdef", "abcdef\n", "a", ""] {
        let mut editor = editor(original);
        let mut state = VisualState::default();
        if !original.is_empty() {
            state.yank(&mut editor, Some(Motion::End), None).unwrap();
        }
        let register = editor.unnamed_register().cloned();
        let mut input = state.begin_counted_insert(&mut editor, 'R', 3).unwrap();
        for action in [
            InputAction::Replace('字'),
            InputAction::Replace('🙂'),
            InputAction::RestoreReplacement,
            InputAction::Replace('X'),
        ] {
            input.apply_input(&action).unwrap();
        }
        input.commit(&mut state).unwrap();
        let suffix = if original.starts_with("abcdef") {
            &original[2..]
        } else {
            ""
        };
        assert_eq!(
            editor.buffer().to_text(),
            format!(
                "字X字X字X{suffix}{}",
                if original.is_empty() { "\n" } else { "" }
            )
        );
        assert_eq!(state.column, 5);
        assert_eq!(editor.unnamed_register(), register.as_ref());
        editor.undo().unwrap();
        assert_eq!(editor.buffer().to_text(), original);
    }
}
#[test]
fn newline_consumes_count_and_generated_copies_can_be_backspaced() {
    let mut editor = editor("abcdef");
    let mut state = VisualState::default();
    let mut input = state.begin_counted_insert(&mut editor, 'R', 3).unwrap();
    input.paste("X\r\nY").unwrap();
    assert_eq!(input.buffer().to_text(), "XXX\nYcdef");
    for expected in ["XXX\nbcdef", "XXXbcdef", "XXbcdef", "Xbcdef", "abcdef"] {
        input.apply_input(&InputAction::RestoreReplacement).unwrap();
        assert_eq!(input.buffer().to_text(), expected);
    }
    // The first LF already consumed the count, even after reversing the split.
    input.apply_input(&InputAction::Replace('Z')).unwrap();
    input.commit(&mut state).unwrap();
    assert_eq!(editor.buffer().to_text(), "Zbcdef");
    editor.undo().unwrap();
    state.column = 0;
    state.repeat_change(&mut editor, None).unwrap();
    assert_eq!(editor.buffer().to_text(), "Zbcdef");
}
#[test]
fn dot_uses_historical_count_and_override_without_compounding() {
    let mut editor = editor("abcdef");
    let mut state = VisualState::default();
    let mut input = state.begin_counted_insert(&mut editor, 'R', 3).unwrap();
    input.paste("XY").unwrap();
    input.commit(&mut state).unwrap();
    assert_eq!(editor.buffer().to_text(), "XYXYXYcdef");
    state.column = 6;
    state.repeat_change(&mut editor, Some(2)).unwrap();
    assert_eq!(editor.buffer().to_text(), "XYXYXYXYXYef");
    state.column = 10;
    state.repeat_change(&mut editor, None).unwrap();
    assert_eq!(editor.buffer().to_text(), "XYXYXYXYXYXYXY");
    editor.undo().unwrap();
    assert_eq!(editor.buffer().to_text(), "XYXYXYXYXYef");
}
#[test]
fn rejected_input_is_atomic_and_backspace_frees_utf8_budget() {
    let mut editor = editor("abc");
    let mut state = VisualState::default();
    assert!(
        state
            .begin_counted_insert(&mut editor, 'R', 1_000_001)
            .is_err()
    );
    let mut input = state
        .begin_counted_insert(&mut editor, 'R', 250_000)
        .unwrap();
    input.apply_input(&InputAction::Replace('字')).unwrap();
    assert!(input.apply_input(&InputAction::Replace('X')).is_err());
    assert!(
        input
            .apply_input(&InputAction::ReplaceText("\n\r".into()))
            .is_err()
    );
    assert_eq!(input.buffer().to_text(), "字bc");
    input.apply_input(&InputAction::RestoreReplacement).unwrap();
    input.apply_input(&InputAction::Replace('Y')).unwrap();
    input.commit(&mut state).unwrap();
    assert_eq!(
        editor.buffer().to_text(),
        format!("{}bc", "Y".repeat(250_000))
    );
    let before = editor.clone();
    assert!(state.repeat_change(&mut editor, Some(usize::MAX)).is_err());
    assert_eq!(editor, before);
}
#[test]
fn dropped_replace_preserves_history_but_completed_erasure_replaces_it() {
    let mut editor = editor("abc");
    let mut state = VisualState::default();
    state.replace_character(&mut editor, 'Z').unwrap();
    let before = editor.clone();
    {
        let mut input = state.begin_counted_insert(&mut editor, 'R', 3).unwrap();
        input.paste("cancel").unwrap();
    }
    assert_eq!(editor, before);
    let mut input = state.begin_counted_insert(&mut editor, 'R', 3).unwrap();
    input.apply_input(&InputAction::Replace('X')).unwrap();
    input.apply_input(&InputAction::RestoreReplacement).unwrap();
    input.commit(&mut state).unwrap();
    assert_eq!(editor.buffer(), before.buffer());
    state.undo(&mut editor).unwrap();
    assert_eq!(editor.buffer(), before.buffer());
    state.column = 1;
    state.repeat_change(&mut editor, None).unwrap();
    assert_eq!(editor.buffer().to_text(), "Zbc");
}

#[test]
fn autoindent_replace_newlines_at_end_empty_and_short_lines() {
    for original in ["a\n", "\n", "", "a"] {
        for count in [1, 2, 3] {
            let mut editor = editor(original);
            editor.set_autoindent(true);
            let mut state = VisualState::default();
            let mut input = state.begin_counted_insert(&mut editor, 'R', count).unwrap();
            input.apply_input(&InputAction::Replace('X')).unwrap();
            input.apply_input(&InputAction::Replace('\n')).unwrap();
            let expected = format!(
                "{}{}",
                "X".repeat(count),
                if original == "a" { "\n" } else { "\n\n" }
            );
            assert_eq!(input.buffer().to_text(), expected);
            input.apply_input(&InputAction::Replace('\n')).unwrap();
            input.apply_input(&InputAction::Replace('Y')).unwrap();
            input.commit(&mut state).unwrap();
            let result = format!("{}\n\nY\n", "X".repeat(count));
            // A non-LF-terminated original keeps its tail convention.
            let result = if original == "a" {
                result.trim_end_matches('\n').to_owned()
            } else {
                result
            };
            assert_eq!(editor.buffer().to_text(), result);
            editor.undo().unwrap();
            assert_eq!(editor.buffer().to_text(), original);
            state.row = 0;
            state.column = 0;
            state.repeat_change(&mut editor, None).unwrap();
            assert_eq!(editor.buffer().to_text(), result);
        }
    }
}

#[test]
fn counted_replace_newline_indent_failure_rolls_back_head_and_copies() {
    let original = format!("{}ab\n", "\t".repeat(1_000_001));
    let mut editor = editor(&original);
    editor.set_autoindent(true);
    let mut state = VisualState::default();
    state.column = 1_000_001;
    let mut input = state.begin_counted_insert(&mut editor, 'R', 2).unwrap();
    let cursor = input.cursor();
    assert_eq!(
        input.apply_input(&InputAction::ReplaceTyped("X\n".into())),
        Err("input repeat too large".into())
    );
    assert_eq!(input.buffer().to_text(), original);
    assert_eq!(input.cursor(), cursor);
    input.apply_input(&InputAction::Replace('Y')).unwrap();
    input.commit(&mut state).unwrap();
    assert_eq!(
        editor.buffer().to_text(),
        format!("{}YYb\n", "\t".repeat(1_000_001))
    );
    editor.undo().unwrap();
    assert_eq!(editor.buffer().to_text(), original);
    state.column = 1_000_001;
    state.repeat_change(&mut editor, None).unwrap();
    assert_eq!(
        editor.buffer().to_text(),
        format!("{}YYb\n", "\t".repeat(1_000_001))
    );
}
