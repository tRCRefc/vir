//! Input key decoding and one-character quoting, independent of terminal I/O.
use super::input::InputTarget;
use crate::editor::InputAction;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

#[derive(Default)]
pub(super) struct InputKeys {
    quoted: bool,
}

impl InputKeys {
    pub(super) fn is_quoted(&self) -> bool {
        self.quoted
    }
    pub(super) fn cancel_quote(&mut self) -> bool {
        std::mem::take(&mut self.quoted)
    }

    pub(super) fn key(
        &mut self,
        edit: &mut impl InputTarget,
        replace: bool,
        key: KeyEvent,
    ) -> Result<bool, String> {
        if key.kind == KeyEventKind::Release {
            return Ok(false);
        }
        if self.quoted {
            let c = quoted_character(key)?;
            // Consume the quoted key even if the transaction rejects its cost.
            self.quoted = false;
            let action = if replace {
                InputAction::ReplaceText(c.to_string())
            } else {
                InputAction::Insert(c.to_string())
            };
            edit.apply_input(&action)?;
            return Ok(false);
        }
        if is_quote(key) {
            self.quoted = true;
            return Ok(false);
        }
        edit_key(edit, replace, key)
    }
}

/// Pending normal-mode r, separate from the input-session quoting lifecycle.
pub(super) struct ReplaceKeys {
    pub(super) count: usize,
    quoted: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum ReplaceKey {
    Pending,
    Cancel,
    Character(char),
}

impl ReplaceKeys {
    pub(super) fn new(count: usize) -> Self {
        Self {
            count,
            quoted: false,
        }
    }
    pub(super) fn is_quoted(&self) -> bool {
        self.quoted
    }
    pub(super) fn key(&mut self, key: KeyEvent) -> Result<ReplaceKey, String> {
        if key.kind == KeyEventKind::Release {
            return Ok(ReplaceKey::Pending);
        }
        if self.quoted {
            return quoted_character(key).map(ReplaceKey::Character);
        }
        if is_quote(key) {
            self.quoted = true;
            return Ok(ReplaceKey::Pending);
        }
        if key.modifiers == KeyModifiers::CONTROL {
            match key.code {
                KeyCode::Char('j' | 'm') => return Ok(ReplaceKey::Character('\n')),
                KeyCode::Char('i') => return Ok(ReplaceKey::Character('\t')),
                _ => {}
            }
        }
        if key.code == KeyCode::Esc
            || key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
        {
            return Ok(ReplaceKey::Cancel);
        }
        match key.code {
            KeyCode::Enter => Ok(ReplaceKey::Character('\n')),
            KeyCode::Tab => Ok(ReplaceKey::Character('\t')),
            KeyCode::Char(c) if !c.is_control() => Ok(ReplaceKey::Character(c)),
            _ => Err("replacement requires one character".into()),
        }
    }
}

fn is_quote(key: KeyEvent) -> bool {
    matches!(key.code, KeyCode::Char('v' | 'q'))
        && key.modifiers.contains(KeyModifiers::CONTROL)
        && !key.modifiers.intersects(
            KeyModifiers::ALT | KeyModifiers::SUPER | KeyModifiers::HYPER | KeyModifiers::META,
        )
}

fn quoted_character(key: KeyEvent) -> Result<char, String> {
    if key.modifiers.intersects(
        KeyModifiers::ALT | KeyModifiers::SUPER | KeyModifiers::HYPER | KeyModifiers::META,
    ) {
        return Err("quote requires one character; still waiting".into());
    }
    let c = match key.code {
        KeyCode::Esc => '\x1b',
        KeyCode::Enter => '\n',
        KeyCode::Tab => '\t',
        // Crossterm's legacy Unix decoder maps DEL to Backspace, ^H to Ctrl-h.
        KeyCode::Backspace => '\x7f',
        KeyCode::Null => '\0',
        KeyCode::Char(c) if key.modifiers.contains(KeyModifiers::CONTROL) => match c {
            ' ' | '@' | '2' => '\0',
            '3' => '\x1b',
            '4'..='7' => (c as u8 - b'4' + 0x1c) as char,
            '8' => '\x7f',
            'a'..='z' => (c as u8 - b'a' + 1) as char,
            'A'..='_' => (c as u8 & 0x1f) as char,
            '?' => '\x7f',
            _ if c.is_ascii_control() => c,
            _ => return Err("quote requires one character; still waiting".into()),
        },
        KeyCode::Char(c) => c,
        _ => return Err("quote requires one character; still waiting".into()),
    };
    match c {
        '\0' => Err("quoted NUL is unsupported; still waiting".into()),
        '\r' => Ok('\n'), // The text model and terminal Enter use LF.
        _ => Ok(c),
    }
}

fn edit_key(edit: &mut impl InputTarget, replace: bool, key: KeyEvent) -> Result<bool, String> {
    if key.code == KeyCode::Esc
        || matches!(key.code, KeyCode::Char('c')) && key.modifiers.contains(KeyModifiers::CONTROL)
    {
        return Ok(true);
    }
    if let Some(action) = action_for_key(replace, key) {
        edit.apply_input(&action)?;
    }
    Ok(false)
}

fn action_for_key(replace: bool, key: KeyEvent) -> Option<InputAction> {
    match key.code {
        KeyCode::Backspace => Some(if replace {
            InputAction::RestoreReplacement
        } else {
            InputAction::Backspace
        }),
        KeyCode::Delete if !replace => Some(InputAction::Delete),
        KeyCode::Enter => Some(if replace {
            InputAction::Replace('\n')
        } else {
            InputAction::Insert("\n".into())
        }),
        KeyCode::Tab => Some(if replace {
            InputAction::Replace('\t')
        } else {
            InputAction::Insert("\t".into())
        }),
        KeyCode::Char(c)
            if !key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                && !c.is_control() =>
        {
            Some(if replace {
                InputAction::Replace(c)
            } else {
                InputAction::Insert(c.to_string())
            })
        }
        _ => None,
    }
}

#[cfg(test)]
fn insert_key(edit: &mut impl InputTarget, key: KeyEvent) -> Result<bool, String> {
    edit_key(edit, false, key)
}

#[cfg(test)]
fn replace_key(edit: &mut impl InputTarget, key: KeyEvent) -> Result<bool, String> {
    edit_key(edit, true, key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{buffer::TextBuffer, editor::Editor, visual::VisualState};
    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }
    fn ctrl(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }
    fn editor() -> Editor {
        Editor::from_buffer(TextBuffer::from_text("abcdef").unwrap())
    }

    #[test]
    fn replacement_quote_has_distinct_cancel_and_character_states() {
        for event in [key(KeyCode::Esc), ctrl('c')] {
            assert_eq!(ReplaceKeys::new(3).key(event).unwrap(), ReplaceKey::Cancel);
            let mut pending = ReplaceKeys::new(3);
            assert_eq!(pending.key(ctrl('v')).unwrap(), ReplaceKey::Pending);
            assert_eq!(
                pending.key(event).unwrap(),
                ReplaceKey::Character(if event.code == KeyCode::Esc {
                    '\x1b'
                } else {
                    '\x03'
                })
            );
            assert_eq!(pending.count, 3);
        }
        let mut pending = ReplaceKeys::new(2);
        pending.key(ctrl('q')).unwrap();
        assert!(pending.key(ctrl(' ')).is_err());
        assert!(pending.is_quoted());
        assert!(pending.key(key(KeyCode::Left)).is_err());
        assert_eq!(
            pending.key(key(KeyCode::Tab)).unwrap(),
            ReplaceKey::Character('\t')
        );
        assert_eq!(
            ReplaceKeys::new(2).key(key(KeyCode::Enter)).unwrap(),
            ReplaceKey::Character('\n')
        );
        assert!(ReplaceKeys::new(2).key(key(KeyCode::Delete)).is_err());
    }

    #[test]
    fn quote_maps_control_bytes_and_rejects_nul_and_noncharacter_keys() {
        for (event, expected) in [
            (ctrl('c'), '\x03'),
            (ctrl('h'), '\x08'),
            (ctrl('v'), '\x16'),
            (ctrl('q'), '\x11'),
            (ctrl('4'), '\x1c'),
            (ctrl('5'), '\x1d'),
            (ctrl('6'), '\x1e'),
            (ctrl('7'), '\x1f'),
            (key(KeyCode::Esc), '\x1b'),
            (key(KeyCode::Backspace), '\x7f'),
            (key(KeyCode::Tab), '\t'),
            (key(KeyCode::Enter), '\n'),
            (ctrl('j'), '\n'),
            (ctrl('m'), '\n'),
            (key(KeyCode::Char('字')), '字'),
        ] {
            assert_eq!(quoted_character(event).unwrap(), expected);
        }
        for event in [
            ctrl(' '),
            key(KeyCode::Null),
            key(KeyCode::Left),
            key(KeyCode::Delete),
            KeyEvent::new(KeyCode::Char('x'), KeyModifiers::ALT),
        ] {
            assert!(quoted_character(event).is_err());
        }
    }

    #[test]
    fn quoted_escape_and_interrupt_are_text_with_one_undo_and_literal_dot() {
        for mode in ['i', 'R'] {
            let mut editor = editor();
            let mut state = VisualState::default();
            let mut input = state.begin_counted_insert(&mut editor, mode, 3).unwrap();
            let mut keys = InputKeys::default();
            for event in [ctrl('v'), key(KeyCode::Esc), ctrl('q'), ctrl('c')] {
                assert!(!keys.key(&mut input, mode == 'R', event).unwrap());
            }
            assert!(
                keys.key(&mut input, mode == 'R', key(KeyCode::Esc))
                    .unwrap()
            );
            input.commit(&mut state).unwrap();
            let suffix = if mode == 'R' { "cdef" } else { "abcdef" };
            let expected = format!("{}{suffix}", "\x1b\x03".repeat(3));
            assert_eq!(editor.buffer().to_text(), expected);
            editor.undo().unwrap();
            assert_eq!(editor.buffer().to_text(), "abcdef");
            state.column = 0;
            state.repeat_change(&mut editor, None).unwrap();
            assert_eq!(editor.buffer().to_text(), expected);
        }
    }

    #[test]
    fn pending_quote_survives_release_and_invalid_keys_but_can_cancel_for_paste() {
        let mut editor = editor();
        let state = VisualState::default();
        let mut input = state.begin_counted_insert(&mut editor, 'i', 1).unwrap();
        let mut keys = InputKeys::default();
        keys.key(&mut input, false, ctrl('v')).unwrap();
        let mut release = key(KeyCode::Esc);
        release.kind = KeyEventKind::Release;
        keys.key(&mut input, false, release).unwrap();
        assert!(keys.is_quoted());
        for event in [ctrl(' '), key(KeyCode::Left), key(KeyCode::Delete)] {
            assert!(keys.key(&mut input, false, event).is_err());
            assert!(keys.is_quoted());
            assert_eq!(input.buffer().to_text(), "abcdef");
        }
        assert!(keys.cancel_quote());
        assert!(!keys.cancel_quote());
        assert!(keys.key(&mut input, false, key(KeyCode::Esc)).unwrap());
    }

    #[test]
    fn quote_newline_consumes_count_and_replace_backspace_restores_control() {
        let mut editor = editor();
        let mut state = VisualState::default();
        let mut input = state.begin_counted_insert(&mut editor, 'R', 3).unwrap();
        let mut keys = InputKeys::default();
        for event in [
            ctrl('v'),
            ctrl('c'),
            key(KeyCode::Backspace),
            key(KeyCode::Char('X')),
            ctrl('v'),
            ctrl('j'),
            key(KeyCode::Char('Y')),
        ] {
            keys.key(&mut input, true, event).unwrap();
        }
        input.commit(&mut state).unwrap();
        assert_eq!(editor.buffer().to_text(), "XXX\nYcdef");
    }

    #[test]
    fn rejected_quoted_input_consumes_prefix_without_losing_accepted_text() {
        let mut editor = editor();
        let mut state = VisualState::default();
        let mut input = state
            .begin_counted_insert(&mut editor, 'i', 500_000)
            .unwrap();
        let mut keys = InputKeys::default();
        keys.key(&mut input, false, key(KeyCode::Char('X')))
            .unwrap();
        keys.key(&mut input, false, ctrl('v')).unwrap();
        assert!(keys.key(&mut input, false, ctrl('c')).is_err());
        assert!(!keys.is_quoted());
        assert_eq!(input.buffer().to_text(), "Xabcdef");
        keys.key(&mut input, false, key(KeyCode::Backspace))
            .unwrap();
        keys.key(&mut input, false, key(KeyCode::Esc)).unwrap();
        input.commit(&mut state).unwrap();
        assert_eq!(editor.buffer().to_text(), "abcdef");
    }
    #[test]
    fn replace_keys_commit_cancel_and_ignore_navigation() {
        for exit in [
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
        ] {
            let mut editor = Editor::default();
            let mut edit = editor
                .begin_text_edit(crate::buffer::Position::default())
                .unwrap();
            for code in [
                KeyCode::Char('中'),
                KeyCode::Tab,
                KeyCode::Enter,
                KeyCode::Char('x'),
                KeyCode::Backspace,
                KeyCode::Backspace,
            ] {
                assert!(!replace_key(&mut edit, KeyEvent::new(code, KeyModifiers::NONE)).unwrap());
            }
            let before = edit.buffer().clone();
            for code in [KeyCode::Left, KeyCode::Delete] {
                assert!(!replace_key(&mut edit, KeyEvent::new(code, KeyModifiers::NONE)).unwrap());
            }
            assert_eq!(edit.buffer(), &before);
            assert!(replace_key(&mut edit, exit).unwrap());
            edit.commit().unwrap();
            assert_eq!(editor.buffer().to_text(), "中\t\n");
            editor.undo().unwrap();
            assert!(editor.buffer().is_empty());
        }
    }

    #[test]
    fn insert_keys_treat_commands_as_text_and_commit_on_escape_or_control_c() {
        for exit in [
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
        ] {
            let mut editor = Editor::default();
            let mut edit = editor
                .begin_text_edit(crate::buffer::Position::default())
                .unwrap();
            for c in "q:中".chars() {
                assert!(
                    !insert_key(
                        &mut edit,
                        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
                    )
                    .unwrap()
                );
            }
            insert_key(&mut edit, KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)).unwrap();
            insert_key(&mut edit, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)).unwrap();
            insert_key(
                &mut edit,
                KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE),
            )
            .unwrap();
            insert_key(&mut edit, KeyEvent::new(KeyCode::Left, KeyModifiers::NONE)).unwrap();
            insert_key(
                &mut edit,
                KeyEvent::new(KeyCode::Char('q'), KeyModifiers::CONTROL),
            )
            .unwrap();
            assert!(insert_key(&mut edit, exit).unwrap());
            edit.commit().unwrap();
            assert_eq!(editor.buffer().to_text(), "q:中\t\n");
            editor.undo().unwrap();
            assert!(editor.buffer().is_empty());
        }
    }
}
