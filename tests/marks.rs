use vir::{
    buffer::{Position, TextBuffer},
    editor::Editor,
    ex::{command::execute, parser::parse},
    visual::VisualState,
};

fn editor(text: &str) -> Editor {
    Editor::from_buffer(TextBuffer::from_text(text).unwrap())
}
fn p(row: usize, column: usize) -> Position {
    Position { row, column }
}

#[test]
fn marking_is_metadata_and_invalid_marks_leave_state_untouched() {
    let mut ed = editor("one\n\n");
    ed.set_mark('a', p(0, 1)).unwrap();
    ed.set_mark('z', p(1, 0)).unwrap();
    assert!(!ed.is_modified());
    assert_eq!(ed.current(), Some(1));
    assert!(ed.undo().is_err());
    assert_eq!(ed.mark('a').unwrap(), p(0, 1));
    let before = ed.clone();
    for (name, position) in [
        ('A', p(0, 0)),
        ('1', p(0, 0)),
        ('a', p(2, 0)),
        ('a', p(0, 9)),
    ] {
        assert!(ed.set_mark(name, position).is_err());
        assert_eq!(ed, before);
    }
    assert!(Editor::default().set_mark('a', p(0, 0)).is_err());
}

#[test]
fn marks_follow_scalar_insert_split_delete_and_undo() {
    let mut ed = editor("a中bc\ntail\n");
    ed.set_mark('a', p(0, 2)).unwrap();
    ed.set_mark('b', p(1, 2)).unwrap();
    let mut edit = ed.begin_text_edit(p(0, 1)).unwrap();
    edit.insert("XY\nZ").unwrap();
    edit.commit().unwrap();
    assert_eq!(ed.buffer().to_text(), "aXY\nZ中bc\ntail\n");
    assert_eq!(ed.mark('a').unwrap(), p(1, 2));
    assert_eq!(ed.mark('b').unwrap(), p(2, 2));
    ed.undo().unwrap();
    assert_eq!(ed.mark('a').unwrap(), p(0, 2));
    ed.undo().unwrap();
    assert_eq!(ed.mark('a').unwrap(), p(1, 2));
    let before = ed.clone();
    let mut edit = ed.begin_text_edit(p(0, 0)).unwrap();
    edit.delete_range(p(0, 1), p(2, 1)).unwrap();
    drop(edit);
    assert_eq!(ed, before);
    let mut edit = ed.begin_text_edit(p(0, 0)).unwrap();
    edit.delete_range(p(0, 1), p(2, 1)).unwrap();
    edit.commit().unwrap();
    assert_eq!(ed.buffer().to_text(), "aail\n");
    assert_eq!(ed.mark('a').unwrap(), p(0, 1));
    assert_eq!(ed.mark('b').unwrap(), p(0, 2));
    ed.undo().unwrap();
    assert_eq!(ed.mark('a').unwrap(), p(1, 2));
}

#[test]
fn line_deletion_invalidates_and_undo_restores_marks() {
    let mut ed = editor("aa\nbb\ncc\n");
    ed.set_mark('a', p(0, 1)).unwrap();
    ed.set_mark('b', p(1, 1)).unwrap();
    ed.set_mark('c', p(2, 1)).unwrap();
    execute(parse("2d").unwrap(), &mut ed).unwrap();
    assert!(ed.mark('b').is_err());
    assert_eq!(ed.mark('c').unwrap(), p(1, 1));
    ed.undo().unwrap();
    assert_eq!(ed.mark('b').unwrap(), p(1, 1));
    ed.insert_lines(0, ["new"]).unwrap();
    assert_eq!(ed.mark('a').unwrap(), p(1, 1));
    execute(parse("2,3c").unwrap(), &mut ed).unwrap();
    assert!(ed.mark('a').is_err());
    assert!(ed.mark('b').is_err());
    assert_eq!(ed.mark('c').unwrap(), p(1, 1));
}

#[test]
fn move_carries_marks_and_copy_keeps_original_anchors() {
    let mut ed = editor("aa\nbb\ncc\ndd\n");
    for (name, row) in [('a', 0), ('b', 1), ('c', 2), ('d', 3)] {
        ed.set_mark(name, p(row, 1)).unwrap();
    }
    execute(parse("2,3m0").unwrap(), &mut ed).unwrap();
    for (name, row) in [('a', 2), ('b', 0), ('c', 1), ('d', 3)] {
        assert_eq!(ed.mark(name).unwrap(), p(row, 1));
    }
    ed.undo().unwrap();
    execute(parse("1,2m$").unwrap(), &mut ed).unwrap();
    assert_eq!(ed.mark('a').unwrap(), p(2, 1));
    assert_eq!(ed.mark('c').unwrap(), p(0, 1));
    ed.undo().unwrap();
    execute(parse("2,3t2").unwrap(), &mut ed).unwrap();
    assert_eq!(ed.mark('b').unwrap(), p(1, 1));
    assert_eq!(ed.mark('c').unwrap(), p(4, 1));
    assert_eq!(ed.mark('d').unwrap(), p(5, 1));
}

#[test]
fn join_preserves_source_character_anchors_and_substitute_clamps_columns() {
    let mut ed = editor("one.\n  中two\nlast\n");
    ed.set_mark('a', p(0, 2)).unwrap();
    ed.set_mark('b', p(1, 3)).unwrap();
    ed.set_mark('c', p(2, 2)).unwrap();
    execute(parse("1,2j").unwrap(), &mut ed).unwrap();
    assert_eq!(ed.buffer().to_text(), "one.  中two\nlast\n");
    assert_eq!(ed.mark('a').unwrap(), p(0, 2));
    assert_eq!(ed.mark('b').unwrap(), p(0, 7));
    assert_eq!(ed.mark('c').unwrap(), p(1, 2));
    execute(parse("1s/.*/X/").unwrap(), &mut ed).unwrap();
    assert_eq!(ed.mark('b').unwrap(), p(0, 0));
    assert_eq!(ed.mark('c').unwrap(), p(1, 2));
    ed.undo().unwrap();
    assert_eq!(ed.mark('b').unwrap(), p(0, 7));
}

#[test]
fn marks_are_snapshot_metadata_and_successful_load_clears_them() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("new");
    std::fs::write(&path, "new\n").unwrap();
    let mut ed = editor("old\n");
    ed.set_mark('a', p(0, 1)).unwrap();
    ed.insert_lines(0, ["inserted"]).unwrap();
    ed.set_mark('b', p(0, 2)).unwrap();
    ed.undo().unwrap();
    assert_eq!(ed.mark('a').unwrap(), p(0, 1));
    assert!(ed.mark('b').is_err());
    ed.undo().unwrap();
    assert_eq!(ed.mark('b').unwrap(), p(0, 2));
    assert!(ed.load_with_force(&path, false).is_err());
    assert_eq!(ed.mark('a').unwrap(), p(1, 1));
    ed.load_with_force(&path, true).unwrap();
    assert!(ed.mark('a').is_err());
    assert!(ed.mark('b').is_err());
}

#[test]
fn line_restore_marks_remain_valid_and_undo_restores_the_previous_positions() {
    let mut ed = editor("abc\n");
    ed.set_mark('a', p(0, 2)).unwrap();
    let mut state = VisualState::default();
    state.checkpoint_line(&ed);
    let mut edit = ed.begin_text_edit(p(0, 0)).unwrap();
    edit.insert("XY").unwrap();
    edit.commit().unwrap();
    assert_eq!(ed.mark('a').unwrap(), p(0, 4));
    state.restore_line(&mut ed).unwrap();
    assert!(ed.mark('a').unwrap().column < 3);
    state.undo(&mut ed).unwrap();
    assert_eq!(ed.mark('a').unwrap(), p(0, 4));
}

#[test]
fn mark_jumps_are_navigation_and_line_jump_uses_first_nonblank() {
    let mut ed = editor("  中abc\nlast\n");
    ed.set_mark('a', p(0, 4)).unwrap();
    let before = ed.clone();
    let mut state = VisualState::default();
    state.row = 1;
    state.jump_mark(ed.buffer(), 'a', false).unwrap();
    assert_eq!((state.row, state.column), (0, 4));
    state.jump_mark(ed.buffer(), 'a', true).unwrap();
    assert_eq!((state.row, state.column), (0, 2));
    assert_eq!(ed, before);
    assert!(state.jump_mark(ed.buffer(), 'b', false).is_err());
    assert_eq!((state.row, state.column), (0, 2));
}

#[test]
fn ex_mark_addresses_work_in_ranges_offsets_and_transfer_destinations() {
    let mut ed = editor("one\ntwo\nthree\nfour\n");
    ed.set_mark('a', p(1, 1)).unwrap();
    ed.set_mark('b', p(3, 1)).unwrap();
    assert_eq!(
        execute(parse("'a,'b-1p").unwrap(), &mut ed).unwrap().output,
        "two\nthree\n"
    );
    execute(parse("1m'b").unwrap(), &mut ed).unwrap();
    assert_eq!(ed.buffer().to_text(), "two\nthree\nfour\none\n");
    assert_eq!(ed.mark('a').unwrap(), p(0, 1));
    execute(parse("'at'b").unwrap(), &mut ed).unwrap();
    assert_eq!(ed.buffer().to_text(), "two\nthree\nfour\ntwo\none\n");
    execute(parse("'b+1d").unwrap(), &mut ed).unwrap();
    assert_eq!(ed.buffer().to_text(), "two\nthree\nfour\none\n");
    for command in ["'zp", "1t'z", "'a-1d", "'b+99p"] {
        let before = ed.clone();
        assert!(
            execute(parse(command).unwrap(), &mut ed).is_err(),
            "{command}"
        );
        assert_eq!(ed, before);
    }
    for command in ["'", "'Ap", "'1p"] {
        assert!(parse(command).is_err());
    }
}

#[test]
fn mark_operators_use_linewise_or_exclusive_character_ranges() {
    use vir::{editor::Register, visual::Motion};
    let mut ed = editor("one\ntwo\nthree\n");
    ed.set_mark('a', p(1, 1)).unwrap();
    let mut state = VisualState::default();
    let motion = Motion::Mark {
        name: 'a',
        linewise: true,
    };
    state
        .yank_register(&mut ed, Some(motion), None, Some('b'))
        .unwrap();
    assert_eq!(
        ed.named_register('b'),
        Some(&Register::Lines(vec!["one".into(), "two".into()]))
    );
    state.delete_motion(&mut ed, motion, None).unwrap();
    assert_eq!(ed.buffer().to_text(), "three\n");
    assert!(ed.mark('a').is_err());
    state.undo(&mut ed).unwrap();
    assert_eq!(ed.mark('a').unwrap(), p(1, 1));
    state.row = 0;
    state.column = 1;
    let motion = Motion::Mark {
        name: 'a',
        linewise: false,
    };
    state.delete_motion(&mut ed, motion, None).unwrap();
    assert_eq!(ed.buffer().to_text(), "owo\nthree\n");
    assert_eq!(
        ed.numbered_register(1),
        Some(&Register::Characters("ne\nt".into()))
    );
}

#[test]
fn mark_change_and_dot_resolve_the_current_mark_and_preserve_numbered_history() {
    use vir::{
        editor::{InputAction, Register},
        visual::{InputStart, Motion},
    };
    let mut ed = editor("keep word\nnext\nkeep other\nlast\n");
    ed.set_mark('a', p(0, 5)).unwrap();
    let mut state = VisualState::default();
    state.row = 1;
    let motion = Motion::Mark {
        name: 'a',
        linewise: false,
    };
    let mut edit = state.begin_change(&mut ed, Some(motion), None).unwrap();
    edit.apply_input(&InputAction::Insert("X".into())).unwrap();
    state
        .commit_input(
            edit,
            InputStart::Change {
                motion: Some(motion),
                count: None,
            },
        )
        .unwrap();
    assert_eq!(
        ed.numbered_register(1),
        Some(&Register::Characters("word".into()))
    );
    ed.set_mark('a', p(2, 5)).unwrap();
    state.row = 3;
    state.column = 0;
    state.repeat_change(&mut ed, None).unwrap();
    assert_eq!(ed.buffer().to_text(), "keep X\nnext\nkeep X\nlast\n");
    assert_eq!(
        ed.numbered_register(1),
        Some(&Register::Characters("other".into()))
    );
    let before = ed.clone();
    assert!(state.delete_motion(&mut ed, motion, Some(2)).is_err());
    assert_eq!(ed, before);
}
