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
fn character_find_repeat_and_failure_preserve_the_last_recipe() {
    let editor = make_editor("abcabc");
    let mut state = VisualState::default();

    state.find_character(&editor, true, false, 'c', 1).unwrap();
    state.repeat_find(&editor, false, 1).unwrap();
    assert_eq!(state.column, 5);
    state.repeat_find(&editor, true, 1).unwrap();
    assert_eq!(state.column, 2);
    let before = (state.row, state.column);
    assert!(state.find_character(&editor, true, false, 'z', 1).is_err());
    assert_eq!((state.row, state.column), before);
    state.repeat_find(&editor, false, 1).unwrap();
    assert_eq!(state.column, 5);
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
    assert_eq!(editor.buffer().line(0), Some("xa"));
}
