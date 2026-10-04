use vir::{
    buffer::TextBuffer,
    editor::{Editor, Register},
    visual::{InputStart, Motion, VisualState},
};

fn editor(text: &str) -> Editor {
    Editor::from_buffer(TextBuffer::from_text(text).unwrap())
}

#[test]
fn nested_pairs_match_both_directions_across_unicode_and_empty_lines() {
    let editor = editor("中 {\n\n  { [()] }\n} tail\n");
    let mut state = VisualState::default();
    state.move_counted(Motion::MatchBracket, Some(usize::MAX), editor.buffer(), 4);
    assert_eq!((state.row, state.column), (3, 0));
    state.match_bracket(editor.buffer()).unwrap();
    assert_eq!((state.row, state.column), (0, 2));
    state.row = 2;
    for (start, end) in [(2, 9), (4, 7), (5, 6), (6, 5), (7, 4), (9, 2)] {
        state.column = start;
        state.match_bracket(editor.buffer()).unwrap();
        assert_eq!((state.row, state.column), (2, end));
    }
}

#[test]
fn backward_operator_includes_the_cursor_and_yank_preserves_undo_dot() {
    let mut editor = editor("a (中\n\n文) z\n");
    let mut state = VisualState::default();
    state.replace_character(&mut editor, 'A').unwrap();
    state.row = 2;
    state.column = 1;
    let text = editor.buffer().clone();
    state
        .yank_register(&mut editor, Some(Motion::MatchBracket), Some(99), Some('a'))
        .unwrap();
    assert_eq!(editor.buffer(), &text);
    assert_eq!(
        editor.named_register('a'),
        Some(&Register::Characters("(中\n\n文)".into()))
    );
    assert_eq!(editor.numbered_register(1), None);
    state.undo(&mut editor).unwrap();
    assert_eq!(editor.buffer().line(0), Some("a (中"));
    state.row = 2;
    state.column = 1;
    state.repeat_change(&mut editor, None).unwrap();
    assert_eq!(editor.buffer().line(2), Some("文A z"));
}

#[test]
fn delete_normalizes_complete_lines_and_dot_rematches_the_target() {
    let mut editor = editor("  {\nx\n}  \n(a) tail\n[bb] rest\n");
    let mut state = VisualState::default();
    state
        .delete_motion(&mut editor, Motion::MatchBracket, Some(99))
        .unwrap();
    assert_eq!(editor.buffer().to_text(), "(a) tail\n[bb] rest\n");
    assert_eq!(
        editor.numbered_register(1),
        Some(&Register::Lines(vec![
            "  {".into(),
            "x".into(),
            "}  ".into()
        ]))
    );
    state.repeat_change(&mut editor, Some(usize::MAX)).unwrap();
    assert_eq!(editor.buffer().to_text(), " tail\n[bb] rest\n");
    state.row = 1;
    state.column = 3;
    state.repeat_change(&mut editor, None).unwrap();
    assert_eq!(editor.buffer().to_text(), " tail\n rest\n");
    state.undo(&mut editor).unwrap();
    assert_eq!(editor.buffer().to_text(), " tail\n[bb] rest\n");
}

#[test]
fn change_is_one_transaction_and_dot_recomputes_range_without_repeating_input() {
    let mut editor = editor("(old) tail\n[new] rest\n");
    let mut state = VisualState::default();
    let start = InputStart::ChangeNamed {
        motion: Some(Motion::MatchBracket),
        count: Some(99),
        register: 'a',
    };
    let mut edit = state
        .begin_change_register(&mut editor, Some(Motion::MatchBracket), Some(99), Some('a'))
        .unwrap();
    edit.insert("X").unwrap();
    state.commit_input(edit, start).unwrap();
    assert_eq!(editor.buffer().to_text(), "X tail\n[new] rest\n");
    assert_eq!(
        editor.named_register('a'),
        Some(&Register::Characters("(old)".into()))
    );
    assert_eq!(editor.numbered_register(1), editor.named_register('a'));
    state.row = 1;
    state.column = 0;
    state.repeat_change(&mut editor, Some(2)).unwrap();
    assert_eq!(editor.buffer().to_text(), "X tail\nX rest\n");
    state.undo(&mut editor).unwrap();
    assert_eq!(editor.buffer().to_text(), "X tail\n[new] rest\n");
    state.undo(&mut editor).unwrap();
    assert_eq!(editor.buffer().to_text(), "X tail\nX rest\n");
}

#[test]
fn unmatched_or_missing_pairs_preserve_registers_undo_dot_and_cursor() {
    for text in ["plain\n", "(no end\n", ") no start\n", "x\n(a)\n"] {
        let mut editor = editor(text);
        let mut state = VisualState::default();
        state.replace_character(&mut editor, 'X').unwrap();
        // Restore text with undo while retaining the replacement recipe.
        state.undo(&mut editor).unwrap();
        let before = editor.clone();
        assert!(state.match_bracket(editor.buffer()).is_err());
        assert_eq!((state.row, state.column), (0, 0));
        assert!(
            state
                .delete_motion_register(&mut editor, Motion::MatchBracket, None, Some('a'))
                .is_err()
        );
        assert!(
            state
                .yank(&mut editor, Some(Motion::MatchBracket), None)
                .is_err()
        );
        assert!(
            state
                .begin_change(&mut editor, Some(Motion::MatchBracket), None)
                .is_err()
        );
        assert_eq!(editor, before);
        state.repeat_change(&mut editor, None).unwrap();
        assert!(editor.buffer().line(0).unwrap().starts_with('X'));
    }
}
