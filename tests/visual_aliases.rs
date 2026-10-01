use vir::{
    buffer::TextBuffer,
    editor::{Editor, Register},
    visual::VisualState,
};

fn editor(text: &str) -> Editor {
    Editor::from_buffer(TextBuffer::from_text(text).unwrap())
}

#[test]
fn counted_x_clamps_on_scalars_and_replays_the_requested_count() {
    let mut editor = editor("a中🙂z\n123456\n");
    let mut state = VisualState::default();
    state.column = 2;
    state.delete_characters(&mut editor, Some(3), None).unwrap();
    assert_eq!(editor.buffer().to_text(), "a中\n123456\n");
    assert_eq!(state.column, 1);
    assert_eq!(
        editor.unnamed_register(),
        Some(&Register::Characters("🙂z".into()))
    );
    assert_eq!(editor.numbered_register(1), None);
    state.undo(&mut editor).unwrap();
    assert_eq!(editor.buffer().line(0), Some("a中🙂z"));
    state.row = 1;
    state.column = 0;
    state.repeat_change(&mut editor, None).unwrap();
    assert_eq!(editor.buffer().line(1), Some("456"));
    state.repeat_change(&mut editor, Some(1)).unwrap();
    state.repeat_change(&mut editor, None).unwrap();
    assert_eq!(editor.buffer().line(1), Some("6"));
}

#[test]
fn x_and_d_use_named_registers_without_rolling_the_numbered_chain() {
    let mut editor = editor("abcd\nlast\n");
    let mut state = VisualState::default();
    state.row = 1;
    state.delete_lines(&mut editor, 1).unwrap();
    let numbered = editor.numbered_register(1).cloned();
    state.row = 0;
    state
        .delete_characters(&mut editor, Some(2), Some('a'))
        .unwrap();
    state
        .delete_characters(&mut editor, None, Some('A'))
        .unwrap();
    assert_eq!(
        editor.named_register('a'),
        Some(&Register::Characters("ab\ncd".into()))
    );
    assert_eq!(editor.numbered_register(1), numbered.as_ref());
    state.undo(&mut editor).unwrap();
    assert_eq!(editor.buffer().to_text(), "cd\n");
    assert_eq!(
        editor.named_register('a'),
        Some(&Register::Characters("ab\ncd".into()))
    );
}

#[test]
fn d_dot_ignores_new_counts_and_stays_on_one_line() {
    let mut editor = editor("abc\ndef\nghi\n");
    let mut state = VisualState::default();
    state.column = 1;
    state.delete_characters(&mut editor, None, None).unwrap();
    state.row = 1;
    state.column = 1;
    state.repeat_change(&mut editor, Some(3)).unwrap();
    assert_eq!(editor.buffer().to_text(), "a\nd\nghi\n");
    state.row = 2;
    state.column = 1;
    state.repeat_change(&mut editor, None).unwrap();
    assert_eq!(editor.buffer().to_text(), "a\nd\ng\n");
}

#[test]
fn invalid_alias_edits_preserve_text_registers_undo_and_dot() {
    let mut editor = editor("abc\n\n");
    let mut state = VisualState::default();
    state.replace_character(&mut editor, 'Z').unwrap();
    for (row, count, register) in [(1, Some(1), None), (1, None, None), (0, Some(2), Some('1'))] {
        state.row = row;
        let before = editor.clone();
        assert!(
            state
                .delete_characters(&mut editor, count, register)
                .is_err()
        );
        assert_eq!(editor, before);
    }
    state.row = 0;
    state.column = 1;
    state.repeat_change(&mut editor, None).unwrap();
    assert_eq!(editor.buffer().line(0), Some("ZZc"));
    let before = editor.clone();
    assert!(state.begin_counted_insert(&mut editor, 'C', 3).is_err());
    assert_eq!(editor, before);
}

#[test]
fn counted_c_selects_lines_once_and_dot_reevaluates_an_empty_line() {
    let mut editor = editor("one\ntwo\n\nfour\n");
    let mut state = VisualState::default();
    state.column = 1;
    let mut input = state
        .begin_counted_insert_register(&mut editor, 'C', 2, Some('a'))
        .unwrap();
    input.paste("X").unwrap();
    input.commit(&mut state).unwrap();
    assert_eq!(editor.buffer().to_text(), "oX\n\nfour\n");
    assert_eq!(
        editor.named_register('a'),
        Some(&Register::Characters("ne\ntwo".into()))
    );
    state.row = 1;
    state.column = 0;
    state.repeat_change(&mut editor, None).unwrap();
    assert_eq!(editor.buffer().to_text(), "oX\nXX\nfour\n");
    state.undo(&mut editor).unwrap();
    assert_eq!(editor.buffer().to_text(), "oX\n\nfour\n");
}

#[test]
fn counted_c_on_empty_line_repeats_input_but_dot_on_text_selects_lines() {
    let mut editor = editor("\nsecond\nthird\nfourth\n");
    let mut state = VisualState::default();
    let mut input = state.begin_counted_insert(&mut editor, 'C', 3).unwrap();
    input.paste("X").unwrap();
    input.commit(&mut state).unwrap();
    assert_eq!(editor.buffer().to_text(), "XXX\nsecond\nthird\nfourth\n");
    state.row = 1;
    state.column = 0;
    state.repeat_change(&mut editor, Some(2)).unwrap();
    assert_eq!(editor.buffer().to_text(), "XXX\nX\nfourth\n");
    state.undo(&mut editor).unwrap();
    assert_eq!(editor.buffer().to_text(), "XXX\nsecond\nthird\nfourth\n");
}

#[test]
fn c_input_budget_and_first_newline_follow_actual_repetitions() {
    let mut editor = editor("\nnext\n");
    let mut state = VisualState::default();
    let before = editor.clone();
    assert!(
        state
            .begin_counted_insert(&mut editor, 'C', usize::MAX)
            .is_err()
    );
    assert_eq!(editor, before);
    let mut input = state.begin_counted_insert(&mut editor, 'C', 3).unwrap();
    input.paste("X\nY").unwrap();
    input.commit(&mut state).unwrap();
    assert_eq!(editor.buffer().to_text(), "XXX\nY\nnext\n");
    state.undo(&mut editor).unwrap();
    assert_eq!(editor.buffer(), before.buffer());
}
