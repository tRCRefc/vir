use vir::{
    buffer::TextBuffer,
    editor::{Editor, Register},
    visual::{Motion, VisualState},
};

fn editor(text: &str) -> Editor {
    Editor::from_buffer(TextBuffer::from_text(text).unwrap())
}

#[test]
fn all_operators_compose_with_both_word_sizes_and_three_directions() {
    let original = "one two...中文 last";
    for (motion, column, captured, deleted) in [
        (Motion::WordForward, 4, "two", "one ...中文 last"),
        (Motion::BigWordForward, 4, "two...中文 ", "one last"),
        (Motion::WordEnd, 4, "two", "one ...中文 last"),
        (Motion::BigWordEnd, 4, "two...中文", "one  last"),
        (Motion::WordBackward, 10, "...", "one two中文 last"),
        (Motion::BigWordBackward, 10, "two...", "one 中文 last"),
    ] {
        for op in ['d', 'c', 'y'] {
            let mut editor = editor(original);
            let mut state = VisualState::default();
            state.column = column;
            let before = editor.clone();
            match op {
                'd' => state.delete_motion(&mut editor, motion, None).unwrap(),
                'c' => {
                    let edit = state.begin_change(&mut editor, Some(motion), None).unwrap();
                    state.finish_insert(edit.commit().unwrap());
                }
                _ => state.yank(&mut editor, Some(motion), None).unwrap(),
            }
            let (captured, deleted) = if op == 'c' && motion == Motion::BigWordForward {
                ("two...中文", "one  last")
            } else {
                (captured, deleted)
            };
            assert_eq!(
                editor.unnamed_register(),
                Some(&Register::Characters(captured.into())),
                "{op} {motion:?}"
            );
            if op == 'y' {
                assert_eq!(editor.buffer(), before.buffer());
                assert_eq!(editor.is_modified(), before.is_modified());
                assert_eq!(editor.current(), before.current());
                assert_eq!(state.column, column);
            } else {
                assert_eq!(editor.buffer().to_text(), deleted, "{op} {motion:?}");
                editor.undo().unwrap();
                assert_eq!(editor.buffer().to_text(), original);
                editor.undo().unwrap();
                assert_eq!(editor.buffer().to_text(), deleted);
            }
        }
    }
}

#[test]
fn change_forward_preserves_trailing_space_and_is_one_input_transaction() {
    for (text, column, count, expected, removed) in [
        ("one two", 0, 1, "X two", "one"),
        ("one two", 2, 1, "onX two", "e"),
        ("one two three", 0, 2, "X three", "one two"),
        ("a\n\nnext tail", 0, 2, "X tail", "a\n\nnext"),
        ("  one two", 0, 1, "Xone two", "  "),
        ("  one two", 0, 2, "Xtwo", "  one "),
        ("one\n  two tail", 0, 2, "X tail", "one\n  two"),
        ("one  ", 0, 1, "X  ", "one"),
        ("one  ", 0, 2, "X", "one  "),
    ] {
        for motion in [Motion::WordForward, Motion::BigWordForward] {
            let mut editor = editor(text);
            let mut state = VisualState::default();
            state.column = column;
            state.yank(&mut editor, None, None).unwrap();
            let before = editor.clone();
            drop(
                state
                    .begin_change(&mut editor, Some(motion), Some(count))
                    .unwrap(),
            );
            assert_eq!(editor, before);
            let mut edit = state
                .begin_change(&mut editor, Some(motion), Some(count))
                .unwrap();
            edit.insert("X").unwrap();
            state.finish_insert(edit.commit().unwrap());
            assert_eq!(editor.buffer().to_text(), expected);
            assert_eq!(
                editor.unnamed_register(),
                Some(&Register::Characters(removed.into()))
            );
            editor.undo().unwrap();
            assert_eq!(editor.buffer().to_text(), text);
            editor.undo().unwrap();
            assert_eq!(editor.buffer().to_text(), expected);
        }
    }
}

#[test]
fn forward_last_segment_stops_at_line_end_and_includes_eof_character() {
    for motion in [Motion::WordForward, Motion::BigWordForward] {
        for (text, column, expected, removed) in [
            ("one  \n  two", 0, "\n  two", "one  "),
            ("one\n  two", 0, "\n  two", "one"),
            ("one  \n  two", 3, "one\n  two", "  "),
            ("中🙂", 1, "中", "🙂"),
            ("a", 0, "", "a"),
            ("  ", 0, "", "  "),
        ] {
            let mut editor = editor(text);
            let mut state = VisualState::default();
            state.column = column;
            state.delete_motion(&mut editor, motion, None).unwrap();
            assert_eq!(editor.buffer().to_text(), expected);
            assert_eq!(
                editor.unnamed_register(),
                Some(&Register::Characters(removed.into()))
            );
        }
    }
    for motion in [Motion::WordEnd, Motion::BigWordEnd] {
        let mut editor = editor("中🙂");
        let mut state = VisualState::default();
        state.column = 1;
        state.delete_motion(&mut editor, motion, None).unwrap();
        assert_eq!(editor.buffer().to_text(), "中");
    }
}

#[test]
fn cross_line_ranges_normalize_exclusive_ends_and_linewise_deletes() {
    for motion in [Motion::WordBackward, Motion::BigWordBackward] {
        for (text, column, expected, register) in [
            ("one\ntwo", 0, "two", Register::Lines(vec!["one".into()])),
            (
                "xx one\ntwo",
                0,
                "xx \ntwo",
                Register::Characters("one".into()),
            ),
            (
                "one\n  two",
                2,
                "two",
                Register::Characters("one\n  ".into()),
            ),
        ] {
            let mut editor = editor(text);
            let mut state = VisualState::default();
            state.row = 1;
            state.column = column;
            state.delete_motion(&mut editor, motion, None).unwrap();
            assert_eq!(editor.buffer().to_text(), expected);
            assert_eq!(editor.unnamed_register(), Some(&register));
            editor.undo().unwrap();
            assert_eq!(editor.buffer().to_text(), text);
        }
    }
    for motion in [Motion::WordForward, Motion::BigWordForward] {
        let mut state = VisualState::default();
        let mut editor = editor("one\ntwo");
        state.yank(&mut editor, Some(motion), Some(2)).unwrap();
        assert_eq!(
            editor.unnamed_register(),
            Some(&Register::Characters("one\ntwo".into()))
        );
        state.delete_motion(&mut editor, motion, Some(2)).unwrap();
        assert!(editor.buffer().is_empty());
        assert_eq!(
            editor.unnamed_register(),
            Some(&Register::Lines(vec!["one".into(), "two".into()]))
        );
    }
}

#[test]
fn empty_lines_counts_and_failed_ranges_keep_state_consistent() {
    for motion in [Motion::WordForward, Motion::BigWordForward] {
        let mut editor = editor("\nnext");
        let mut state = VisualState::default();
        state.yank(&mut editor, Some(motion), None).unwrap();
        assert_eq!(
            editor.unnamed_register(),
            Some(&Register::Lines(vec!["".into()]))
        );
        let mut edit = state.begin_change(&mut editor, Some(motion), None).unwrap();
        edit.insert("X").unwrap();
        edit.commit().unwrap();
        assert_eq!(editor.buffer().to_text(), "X\nnext");
        editor.undo().unwrap();
        state.delete_motion(&mut editor, motion, None).unwrap();
        assert_eq!(editor.buffer().to_text(), "next");
    }
    for motion in [
        Motion::WordForward,
        Motion::BigWordForward,
        Motion::WordEnd,
        Motion::BigWordEnd,
    ] {
        let mut editor = editor("one two\nthree four");
        let mut state = VisualState::default();
        state
            .delete_motion(&mut editor, motion, Some(usize::MAX))
            .unwrap();
        assert!(editor.buffer().is_empty());
        editor.undo().unwrap();
        let mut edit = state
            .begin_change(&mut editor, Some(motion), Some(usize::MAX))
            .unwrap();
        edit.insert("X").unwrap();
        edit.commit().unwrap();
        assert_eq!(editor.buffer().to_text(), "X");
    }
    for text in ["", "abc", "\n"] {
        let mut editor = editor(text);
        let mut state = VisualState::default();
        let before = editor.clone();
        let visual = format!("{state:?}");
        assert!(
            state
                .delete_motion(&mut editor, Motion::WordBackward, None)
                .is_err()
        );
        assert!(
            state
                .yank(&mut editor, Some(Motion::BigWordBackward), None)
                .is_err()
        );
        assert!(
            state
                .begin_change(&mut editor, Some(Motion::WordBackward), None)
                .is_err()
        );
        assert_eq!(editor, before);
        assert_eq!(format!("{state:?}"), visual);
    }
}

#[test]
fn empty_final_ranges_preserve_register_and_counted_backward_clamps() {
    for motion in [
        Motion::WordForward,
        Motion::BigWordForward,
        Motion::WordEnd,
        Motion::BigWordEnd,
    ] {
        for count in [1, 3, usize::MAX] {
            let mut editor = editor("keep\n\n");
            let mut state = VisualState::default();
            state.yank(&mut editor, None, None).unwrap();
            state.row = 1;
            let before = editor.clone();
            assert!(
                state
                    .delete_motion(&mut editor, motion, Some(count))
                    .is_err()
            );
            assert!(state.yank(&mut editor, Some(motion), Some(count)).is_err());
            assert!(
                state
                    .begin_change(&mut editor, Some(motion), Some(count))
                    .is_err()
            );
            assert_eq!(editor, before);
        }
    }
    for motion in [Motion::WordBackward, Motion::BigWordBackward] {
        let mut editor = editor("\nnext");
        let mut state = VisualState::default();
        state.row = 1;
        state
            .delete_motion(&mut editor, motion, Some(usize::MAX))
            .unwrap();
        assert_eq!(editor.buffer().to_text(), "next");
        assert_eq!(
            editor.unnamed_register(),
            Some(&Register::Lines(vec![String::new()]))
        );
    }
    for count in [2, 3, usize::MAX] {
        let mut editor = editor("one\n\n\n");
        let mut state = VisualState::default();
        state
            .delete_motion(&mut editor, Motion::WordForward, Some(count))
            .unwrap();
        assert_eq!(editor.buffer().to_text(), "\n");
        assert_eq!(
            editor.unnamed_register(),
            Some(&Register::Lines(vec!["one".into(), String::new()]))
        );
    }
}
