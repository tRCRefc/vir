//! Register keystrokes shared by all visual input modes.
use crate::editor::{Editor, Register};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};

const MAX_MACRO_KEYS: usize = 65_536;
const MAX_EXPANSIONS: usize = 100;

#[derive(Default)]
pub(super) struct MacroInput {
    events: Vec<Event>,
    expansions: usize,
}

impl MacroInput {
    pub(super) fn is_active(&self) -> bool {
        self.expansions != 0
    }

    pub(super) fn pop(&mut self) -> Option<Event> {
        let event = self.events.pop();
        if event.is_none() {
            // A new physical keystroke starts a fresh invocation budget. Do
            // not reset when popping a nested macro's last argument key.
            self.expansions = 0;
        }
        event
    }

    pub(super) fn clear(&mut self) {
        self.events.clear();
        self.expansions = 0;
    }

    pub(super) fn expand(&mut self, editor: &mut Editor, name: char) -> Result<(), String> {
        if self.expansions >= MAX_EXPANSIONS {
            return Err("macro expansion limit".into());
        }
        let register = match name {
            'a'..='z' | 'A'..='Z' => editor.named_register(name),
            '1'..='9' => editor.numbered_register((name as u8 - b'0') as usize),
            _ => None,
        }
        .ok_or("macro register is empty")?;
        let bytes = match register {
            Register::Characters(text) => text.len(),
            Register::Lines(lines) => lines
                .iter()
                .try_fold(0usize, |size, line| {
                    size.checked_add(line.len())?.checked_add(1)
                })
                .ok_or("macro too long")?,
        };
        if bytes == 0 {
            return Err("macro register is empty".into());
        }
        // Byte count is a conservative bound on scalar events, checked before
        // allocating a copy of the register or expanding nested invocations.
        if bytes > MAX_MACRO_KEYS.saturating_sub(self.events.len()) {
            return Err("macro too long".into());
        }
        let text = match register {
            Register::Characters(text) => text.clone(),
            Register::Lines(lines) => format!("{}\n", lines.join("\n")),
        };
        self.events
            .extend(text.chars().rev().map(|c| Event::Key(decode(c))));
        self.expansions += 1;
        editor.begin_macro_undo();
        Ok(())
    }
}

fn decode(c: char) -> KeyEvent {
    let (code, modifiers) = match c {
        '\x1b' => (KeyCode::Esc, KeyModifiers::NONE),
        '\n' | '\r' => (KeyCode::Enter, KeyModifiers::NONE),
        '\t' => (KeyCode::Tab, KeyModifiers::NONE),
        '\x7f' => (KeyCode::Backspace, KeyModifiers::NONE),
        '\0' => (KeyCode::Null, KeyModifiers::NONE),
        '\x01'..='\x1f' => (
            KeyCode::Char((c as u8 + b'@').to_ascii_lowercase() as char),
            KeyModifiers::CONTROL,
        ),
        _ => (KeyCode::Char(c), KeyModifiers::NONE),
    };
    KeyEvent::new(code, modifiers)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_streams_precede_parent_suffix_and_controls_use_terminal_key_events() {
        let mut editor = Editor::default();
        editor
            .set_named_register(
                'a',
                Register::Characters("i中\x1b\x16\x03\t\x7f".into()),
                false,
            )
            .unwrap();
        editor
            .set_named_register('b', Register::Lines(vec![":s/a/b/".into()]), false)
            .unwrap();
        let mut input = MacroInput::default();
        input.expand(&mut editor, 'a').unwrap();
        assert_eq!(input.pop(), Some(Event::Key(decode('i'))));
        input.expand(&mut editor, 'b').unwrap();
        for c in ":s/a/b/\n中\x1b\x16\x03\t\x7f".chars() {
            assert_eq!(input.pop(), Some(Event::Key(decode(c))));
        }
        assert_eq!(input.pop(), None);
        assert_eq!(
            decode('\x1b'),
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)
        );
        assert_eq!(
            decode('\x16'),
            KeyEvent::new(KeyCode::Char('v'), KeyModifiers::CONTROL)
        );
        assert_eq!(
            decode('\x03'),
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)
        );
        assert_eq!(
            decode('\t'),
            KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)
        );
        assert_eq!(
            decode('\x7f'),
            KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE)
        );
    }

    #[test]
    fn expansion_budget_resets_for_physical_input_but_limits_tail_recursion() {
        let mut editor = Editor::default();
        editor
            .set_named_register('a', Register::Characters("@a".into()), false)
            .unwrap();
        let mut input = MacroInput::default();
        for _ in 0..MAX_EXPANSIONS + 5 {
            input.expand(&mut editor, 'a').unwrap();
            while input.pop().is_some() {}
        }
        for _ in 0..MAX_EXPANSIONS {
            input.expand(&mut editor, 'a').unwrap();
            assert!(input.pop().is_some());
            assert!(input.pop().is_some());
        }
        assert!(input.expand(&mut editor, 'a').is_err());
        input.clear();
        assert!(input.expand(&mut editor, 'a').is_ok());
    }

    #[test]
    fn oversized_and_empty_registers_preserve_the_pending_stream() {
        let mut editor = Editor::default();
        editor
            .set_named_register('a', Register::Characters("x".into()), false)
            .unwrap();
        editor
            .set_named_register('b', Register::Characters("x".repeat(MAX_MACRO_KEYS)), false)
            .unwrap();
        editor
            .set_named_register('c', Register::Characters(String::new()), false)
            .unwrap();
        let mut input = MacroInput::default();
        input.expand(&mut editor, 'a').unwrap();
        for name in ['b', 'c', 'd'] {
            assert!(input.expand(&mut editor, name).is_err());
        }
        assert_eq!(input.pop(), Some(Event::Key(decode('x'))));
        assert_eq!(input.pop(), None);
    }
}
