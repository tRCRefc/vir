use vir::{
    buffer::TextBuffer,
    editor::Editor,
    visual::{Motion, VisualState},
};

fn make_editor(text: &str) -> Editor {
    Editor::from_buffer(TextBuffer::from_text(text).unwrap())
}

#[test]
fn character_find_moves_with_direction_till_and_count() {
    let editor = make_editor("abcabc");
    let mut state = VisualState::default();

    state.find_character(&editor, true, false, 'c', 1).unwrap();
    assert_eq!((state.row, state.column), (0, 2));
    state.find_character(&editor, true, false, 'c', 1).unwrap();
    assert_eq!(state.column, 5);

    state.find_character(&editor, false, false, 'c', 1).unwrap();
    assert_eq!(state.column, 2);
    state.column = 5;
    state.find_character(&editor, false, true, 'c', 1).unwrap();
    assert_eq!(state.column, 3);

    state.column = 0;
    state.find_character(&editor, true, true, 'c', 2).unwrap();
    assert_eq!(state.column, 4);
    state.column = 0;
    state.find_character(&editor, true, false, 'c', 2).unwrap();
    assert_eq!(state.column, 5);
}

#[test]
fn comma_preserves_direction_but_failed_explicit_find_remembers_its_argument() {
    let editor = make_editor("abcabc\nzaz\n");
    let mut state = VisualState::default();

    state.find_character(&editor, true, false, 'c', 1).unwrap();
    state.repeat_find(&editor, false, 1).unwrap();
    assert_eq!(state.column, 5);
    state.repeat_find(&editor, true, 1).unwrap();
    assert_eq!(state.column, 2);
    let before = (state.row, state.column);
    assert!(state.find_character(&editor, true, false, 'z', 1).is_err());
    assert_eq!((state.row, state.column), before);
    assert!(state.repeat_find(&editor, false, 1).is_err());
    state.row = 1;
    state.column = 0;
    state.repeat_find(&editor, false, 1).unwrap();
    assert_eq!(state.column, 2);
}

#[test]
fn character_find_motion_is_usable_by_delete_operators() {
    let mut editor = make_editor("abcabc");
    let mut state = VisualState::default();
    state
        .delete_motion(
            &mut editor,
            Motion::Find {
                forward: true,
                till: false,
                character: 'c',
            },
            None,
        )
        .unwrap();
    assert_eq!(editor.buffer().line(0), Some("abc"));

    let mut editor = make_editor("xabcx");
    let mut state = VisualState::default();
    state.column = 4;
    state
        .delete_motion(
            &mut editor,
            Motion::Find {
                forward: false,
                till: true,
                character: 'a',
            },
            None,
        )
        .unwrap();
    assert_eq!(editor.buffer().line(0), Some("xax"));
}

#[test]
fn all_find_operator_endpoints_match_historical_half_open_ranges() {
    use vir::editor::Register;
    for (column, forward, till, selected, remaining) in [
        (0, true, false, "ab中", "cd中ef"),
        (0, true, true, "ab", "中cd中ef"),
        (7, false, false, "中e", "ab中cdf"),
        (7, false, true, "e", "ab中cd中f"),
    ] {
        let motion = Motion::Find {
            forward,
            till,
            character: '中',
        };
        for operator in ['d', 'c', 'y'] {
            let mut editor = make_editor("ab中cd中ef");
            let mut state = VisualState::default();
            state.column = column;
            match operator {
                'd' => state.delete_motion(&mut editor, motion, None).unwrap(),
                'c' => {
                    let edit = state.begin_change(&mut editor, Some(motion), None).unwrap();
                    state
                        .commit_input(
                            edit,
                            vir::visual::InputStart::Change {
                                motion: Some(motion),
                                count: None,
                            },
                        )
                        .unwrap();
                }
                'y' => state.yank(&mut editor, Some(motion), None).unwrap(),
                _ => unreachable!(),
            }
            assert_eq!(
                editor.unnamed_register(),
                Some(&Register::Characters(selected.into())),
                "{operator} {motion:?}"
            );
            assert_eq!(
                editor.buffer().to_text(),
                if operator == 'y' {
                    "ab中cd中ef"
                } else {
                    remaining
                }
            );
            if operator != 'y' {
                state.undo(&mut editor).unwrap();
                assert_eq!(editor.buffer().to_text(), "ab中cd中ef");
            }
        }
    }
}

#[test]
fn adjacent_till_huge_counts_empty_lines_and_dot_keep_correct_boundaries() {
    let mut editor = make_editor("a中b中c中\n\nx\n");
    let mut state = VisualState::default();
    let before = editor.clone();
    state.find_character(&editor, true, true, '中', 1).unwrap();
    assert_eq!(state.column, 0);
    // Historical ; repeats from the cursor and may find the same adjacent target.
    state.repeat_find(&editor, false, 1).unwrap();
    assert_eq!(state.column, 0);
    state.repeat_find(&editor, false, 2).unwrap();
    assert_eq!(state.column, 2);
    assert!(
        state
            .find_character(&editor, true, false, '中', usize::MAX)
            .is_err()
    );
    assert_eq!(state.column, 2);
    assert_eq!(editor, before);
    state.column = 0;
    let till = Motion::Find {
        forward: true,
        till: true,
        character: '中',
    };
    state.delete_motion(&mut editor, till, None).unwrap();
    assert_eq!(editor.buffer().line(0), Some("中b中c中"));
    state.repeat_change(&mut editor, Some(2)).unwrap();
    assert_eq!(editor.buffer().line(0), Some("中"));
    let before = editor.clone();
    assert!(state.repeat_change(&mut editor, None).is_err());
    assert_eq!(editor, before);
    state.row = 1;
    assert!(state.find_character(&editor, true, false, 'x', 1).is_err());
    state.row = 0;
    state.column = 0;
    let mut empty = Editor::default();
    assert!(state.find_character(&empty, true, false, 'x', 1).is_err());
    assert!(state.delete_motion(&mut empty, till, None).is_err());
}

#[test]
fn dot_replays_semicolon_against_current_find_history_but_explicit_find_keeps_its_character() {
    let mut editor = make_editor("abxcydz");
    let mut state = VisualState::default();
    state.find_character(&editor, true, false, 'x', 1).unwrap();
    state.column = 0;
    state
        .delete_motion(&mut editor, Motion::RepeatFind { reverse: false }, None)
        .unwrap();
    assert_eq!(editor.buffer().to_text(), "cydz");
    state.find_character(&editor, true, false, 'y', 1).unwrap();
    state.column = 0;
    state.repeat_change(&mut editor, None).unwrap();
    assert_eq!(editor.buffer().to_text(), "dz");

    let mut editor = make_editor("axbycx");
    let mut state = VisualState::default();
    state
        .delete_motion(
            &mut editor,
            Motion::Find {
                forward: true,
                till: false,
                character: 'x',
            },
            None,
        )
        .unwrap();
    state.find_character(&editor, true, false, 'y', 1).unwrap();
    state.column = 0;
    state.repeat_change(&mut editor, None).unwrap();
    assert_eq!(editor.buffer().to_text(), "");
}
