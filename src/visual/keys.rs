//! Normal-mode counts and command prefixes; no terminal I/O or text mutation.
use super::{InputStart, Motion, input::MAX_INPUT_WORK, operator::Operator};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Default)]
pub(super) struct NormalPrefix {
    digits: usize,
    operator: Option<(Operator, usize)>,
    register: Option<char>,
    register_pending: bool,
    selected_register: Option<char>,
    macro_pending: bool,
    find_pending: Option<(bool, bool)>,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum PrefixAction {
    Pass,
    Pending,
    Join(usize),
    Replace(usize),
    Insert(char, usize),
    Move(Motion, usize),
    Repeat(Option<usize>),
    Operate(Operator, Option<Motion>, Option<usize>),
    Put(bool, usize),
    Macro(char),
    Find {
        forward: bool,
        till: bool,
        character: char,
        count: usize,
        operator: Option<(Operator, Option<usize>)>,
    },
    RepeatFind {
        reverse: bool,
        count: usize,
        operator: Option<(Operator, Option<usize>)>,
    },
}

impl NormalPrefix {
    pub(super) fn is_pending(&self) -> bool {
        self.digits != 0
            || self.operator.is_some()
            || self.register_pending
            || self.register.is_some()
            || self.macro_pending
            || self.find_pending.is_some()
    }

    pub(super) fn take_register(&mut self) -> Option<char> {
        self.selected_register.take()
    }

    fn complete(&mut self) {
        let selected = self.register.take();
        *self = Self {
            selected_register: selected,
            ..Self::default()
        };
    }

    pub(super) fn key(&mut self, key: KeyEvent) -> Result<PrefixAction, &'static str> {
        let active = self.is_pending();
        if self.register_pending {
            if key.code == KeyCode::Esc
                || key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
            {
                *self = Self::default();
                return Ok(PrefixAction::Pending);
            }
            let KeyCode::Char(name @ ('a'..='z' | 'A'..='Z' | '1'..='9')) = key.code else {
                *self = Self::default();
                return Err("invalid register");
            };
            self.register_pending = false;
            self.register = Some(name);
            return Ok(PrefixAction::Pending);
        }
        if self.macro_pending {
            if key.code == KeyCode::Esc
                || key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
            {
                *self = Self::default();
                return Ok(PrefixAction::Pending);
            }
            let KeyCode::Char(name @ ('a'..='z' | 'A'..='Z' | '1'..='9' | '@')) = key.code else {
                *self = Self::default();
                return Err("invalid macro register");
            };
            self.macro_pending = false;
            self.complete();
            self.selected_register = None;
            return Ok(PrefixAction::Macro(name));
        }
        if let Some((forward, till)) = self.find_pending {
            if key.code == KeyCode::Esc
                || key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
            {
                *self = Self::default();
                return Ok(PrefixAction::Pending);
            }
            let KeyCode::Char(character) = key.code else {
                *self = Self::default();
                return Err("invalid character search");
            };
            let count = self.digits.max(1);
            let operator = self.operator.map(|(operator, before)| {
                let count = before.max(1).checked_mul(self.digits.max(1));
                (
                    operator,
                    (before != 0 || self.digits != 0).then_some(count.unwrap_or(0)),
                )
            });
            if operator.is_some_and(|(_, count)| count == Some(0)) {
                *self = Self::default();
                return Err("count too large");
            }
            self.find_pending = None;
            self.complete();
            return Ok(PrefixAction::Find {
                forward,
                till,
                character,
                count,
                operator,
            });
        }
        if self.digits != 0
            && self.operator.is_none()
            && !matches!(key.code, KeyCode::Char('0'..='9'))
            && let Some(motion) = normal_motion(key)
        {
            let count = self.digits;
            self.complete();
            return Ok(PrefixAction::Move(motion, count));
        }

        if key.code == KeyCode::Esc
            || key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
        {
            *self = Self::default();
            return Ok(if active {
                PrefixAction::Pending
            } else {
                PrefixAction::Pass
            });
        }
        if let Some(result) = self.digit_key(key) {
            return result;
        }
        if key.code == KeyCode::Char('"') {
            self.register_pending = true;
            return Ok(PrefixAction::Pending);
        }
        if key.code == KeyCode::Char('@') {
            self.macro_pending = true;
            return Ok(PrefixAction::Pending);
        }
        if let KeyCode::Char(command @ ('f' | 'F' | 't' | 'T')) = key.code {
            self.find_pending = Some((matches!(command, 'f' | 't'), matches!(command, 't' | 'T')));
            return Ok(PrefixAction::Pending);
        }
        if key.code == KeyCode::Char(';') || key.code == KeyCode::Char(',') {
            let count = self.digits.max(1);
            let operator = self.operator.map(|(operator, before)| {
                let count = before.max(1).checked_mul(self.digits.max(1));
                (
                    operator,
                    (before != 0 || self.digits != 0).then_some(count.unwrap_or(0)),
                )
            });
            if operator.is_some_and(|(_, count)| count == Some(0)) {
                *self = Self::default();
                return Err("count too large");
            }
            self.complete();
            return Ok(PrefixAction::RepeatFind {
                reverse: key.code == KeyCode::Char(','),
                count,
                operator,
            });
        }
        if let Some(result) = self.operator_key(key) {
            return result;
        }
        if let Some((operator, before)) = self.operator {
            return self.finish_operator(key, operator, before);
        }
        self.command_key(key, active)
    }

    fn digit_key(&mut self, key: KeyEvent) -> Option<Result<PrefixAction, &'static str>> {
        let KeyCode::Char(c @ '0'..='9') = key.code else {
            return None;
        };
        if c == '0' && self.digits == 0 {
            return None;
        }
        let next = self
            .digits
            .checked_mul(10)
            .and_then(|n| n.checked_add((c as u8 - b'0') as usize));
        Some(match next {
            Some(next) => {
                self.digits = next;
                Ok(PrefixAction::Pending)
            }
            None => {
                *self = Self::default();
                Err("count too large")
            }
        })
    }

    fn operator_key(&mut self, key: KeyEvent) -> Option<Result<PrefixAction, &'static str>> {
        let KeyCode::Char(c) = key.code else {
            return None;
        };
        let requested = Operator::from_key(c)?;
        if let Some((operator, before)) = self.operator {
            let count = before.max(1).checked_mul(self.digits.max(1));
            self.complete();
            return Some(if requested != operator {
                Err("mixed operators; command cancelled")
            } else {
                count
                    .map(|n| PrefixAction::Operate(operator, None, Some(n)))
                    .ok_or("count too large")
            });
        }
        self.operator = Some((requested, self.digits));
        self.digits = 0;
        Some(Ok(PrefixAction::Pending))
    }

    fn finish_operator(
        &mut self,
        key: KeyEvent,
        operator: Operator,
        before: usize,
    ) -> Result<PrefixAction, &'static str> {
        let count = before.max(1).checked_mul(self.digits.max(1));
        let explicit = before != 0 || self.digits != 0;
        self.complete();
        let count = count.ok_or("count too large")?;
        normal_motion(key)
            .filter(|motion| motion.supports_operator())
            .map(|motion| PrefixAction::Operate(operator, Some(motion), explicit.then_some(count)))
            .ok_or("unsupported operator motion; command cancelled")
    }

    fn command_key(&mut self, key: KeyEvent, active: bool) -> Result<PrefixAction, &'static str> {
        let digits = self.digits;
        let count = digits.max(1);
        self.complete();
        match key.code {
            KeyCode::Char(mode @ ('i' | 'a' | 'I' | 'A' | 'o' | 'O' | 'R')) => {
                if InputStart::Insert(mode).input_count(count) > MAX_INPUT_WORK {
                    return Err("input repeat too large");
                }
                Ok(PrefixAction::Insert(mode, count))
            }
            KeyCode::Char('r') => Ok(PrefixAction::Replace(count)),
            KeyCode::Char('.') => Ok(PrefixAction::Repeat((digits != 0).then_some(digits))),
            KeyCode::Char(c @ ('p' | 'P')) => Ok(PrefixAction::Put(c == 'P', count)),
            KeyCode::Char('J') => Ok(PrefixAction::Join(count.max(2))),
            _ if active => Err("unsupported count or operator; command cancelled"),
            _ => Ok(PrefixAction::Pass),
        }
    }
}

pub(super) fn normal_motion(key: KeyEvent) -> Option<Motion> {
    if key.modifiers.contains(KeyModifiers::ALT) {
        return None;
    }
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        return match key.code {
            KeyCode::Char('f') => Some(Motion::PageDown),
            KeyCode::Char('b') => Some(Motion::PageUp),
            _ => None,
        };
    }
    match key.code {
        KeyCode::Char('W') => Some(Motion::BigWordForward),
        KeyCode::Char('B') => Some(Motion::BigWordBackward),
        KeyCode::Char('E') => Some(Motion::BigWordEnd),
        KeyCode::Char('w') => Some(Motion::WordForward),
        KeyCode::Char('b') => Some(Motion::WordBackward),
        KeyCode::Char('e') => Some(Motion::WordEnd),
        KeyCode::Char('h') | KeyCode::Left => Some(Motion::Left),
        KeyCode::Char('j') | KeyCode::Down => Some(Motion::Down),
        KeyCode::Char('k') | KeyCode::Up => Some(Motion::Up),
        KeyCode::Char('l') | KeyCode::Right => Some(Motion::Right),
        KeyCode::Char('0') | KeyCode::Home => Some(Motion::Home),
        KeyCode::Char('$') | KeyCode::End => Some(Motion::End),
        KeyCode::Char('g') => Some(Motion::First),
        KeyCode::Char('G') => Some(Motion::Last),
        KeyCode::PageUp => Some(Motion::PageUp),
        KeyCode::PageDown => Some(Motion::PageDown),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{editor::Editor, visual::VisualState};
    #[test]
    fn open_screen_counts_accept_large_values_but_overflow_cancels() {
        let key = |c| KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE);
        let mut prefix = NormalPrefix::default();
        for mode in ['o', 'O'] {
            for c in usize::MAX.to_string().chars() {
                prefix.key(key(c)).unwrap();
            }
            assert_eq!(
                prefix.key(key(mode)),
                Ok(PrefixAction::Insert(mode, usize::MAX))
            );
            for c in usize::MAX.to_string().chars() {
                prefix.key(key(c)).unwrap();
            }
            assert!(prefix.key(key('0')).is_err());
            assert_eq!(prefix.key(key(mode)), Ok(PrefixAction::Insert(mode, 1)));
            prefix.key(key('d')).unwrap();
            assert!(prefix.key(key(mode)).is_err());
            assert!(!prefix.is_pending());
        }
    }

    #[test]
    fn counted_input_prefix_is_bounded_and_cancellation_clears_state() {
        let key = |c| KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE);
        let mut prefix = NormalPrefix::default();
        for mode in ['i', 'a', 'I', 'A', 'o', 'O', 'R'] {
            prefix.key(key('3')).unwrap();
            assert!(prefix.is_pending());
            assert_eq!(prefix.key(key(mode)), Ok(PrefixAction::Insert(mode, 3)));
            assert!(!prefix.is_pending());
            assert_eq!(prefix.key(key(mode)), Ok(PrefixAction::Insert(mode, 1)));
        }
        prefix.key(key('2')).unwrap();
        assert!(prefix.key(key('C')).is_err());
        assert_eq!(prefix.key(key('i')), Ok(PrefixAction::Insert('i', 1)));
        for c in "1000001".chars() {
            prefix.key(key(c)).unwrap();
        }
        assert!(prefix.key(key('i')).is_err());
        assert!(!prefix.is_pending());
        for cancel in [
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
        ] {
            prefix.key(key('3')).unwrap();
            prefix.key(cancel).unwrap();
            assert_eq!(prefix.key(key('i')), Ok(PrefixAction::Insert('i', 1)));
        }
    }

    #[test]
    fn replace_prefix_preserves_count_and_rejects_operator_combinations() {
        let key = |c| KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE);
        let mut prefix = NormalPrefix::default();
        for (sequence, count) in [("r", 1), ("1r", 1), ("3r", 3), ("10r", 10)] {
            for c in sequence[..sequence.len() - 1].chars() {
                assert_eq!(prefix.key(key(c)), Ok(PrefixAction::Pending));
            }
            assert_eq!(prefix.key(key('r')), Ok(PrefixAction::Replace(count)));
            assert_eq!(prefix.key(key('0')), Ok(PrefixAction::Pass));
        }
        for operator in ['d', 'c', 'y'] {
            prefix.key(key('2')).unwrap();
            prefix.key(key(operator)).unwrap();
            assert!(prefix.key(key('r')).is_err());
            assert_eq!(prefix.key(key('r')), Ok(PrefixAction::Replace(1)));
        }
        for c in usize::MAX.to_string().chars() {
            prefix.key(key(c)).unwrap();
        }
        assert!(prefix.key(key('0')).is_err());
        assert_eq!(prefix.key(key('r')), Ok(PrefixAction::Replace(1)));
        let mut editor = Editor::default();
        let state = VisualState::default();
        assert!(state.replacement_end(&editor, 1).is_err());
        editor.insert_lines(0, ["中ab"]).unwrap();
        assert_eq!(state.replacement_end(&editor, 3).unwrap(), 3);
        assert!(state.replacement_end(&editor, 4).is_err());
    }

    #[test]
    fn word_keys_compose_with_operators_and_cancel_counts() {
        let key = |c| KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE);
        let mut prefix = NormalPrefix::default();
        for c in ['w', 'b', 'e', 'W', 'B', 'E'] {
            assert_eq!(prefix.key(key(c)), Ok(PrefixAction::Pass));
            for cancel in [
                KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
                KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
            ] {
                prefix.key(key('9')).unwrap();
                assert_eq!(prefix.key(cancel), Ok(PrefixAction::Pending));
                assert_eq!(prefix.key(key(c)), Ok(PrefixAction::Pass));
            }
            for operator in ['d', 'c', 'y'] {
                prefix.key(key('2')).unwrap();
                prefix.key(key(operator)).unwrap();
                prefix.key(key('3')).unwrap();
                assert_eq!(
                    prefix.key(key(c)),
                    Ok(PrefixAction::Operate(
                        Operator::from_key(operator).unwrap(),
                        normal_motion(key(c)),
                        Some(6)
                    ))
                );
                assert_eq!(prefix.key(key(c)), Ok(PrefixAction::Pass));
            }
            for operator in ['d', 'c', 'y'] {
                for digit in usize::MAX.to_string().chars() {
                    prefix.key(key(digit)).unwrap();
                }
                prefix.key(key(operator)).unwrap();
                prefix.key(key('2')).unwrap();
                assert!(prefix.key(key(c)).is_err());
                assert_eq!(prefix.key(key(c)), Ok(PrefixAction::Pass));
                prefix.key(key(operator)).unwrap();
                prefix.key(key('3')).unwrap();
                assert_eq!(
                    prefix.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
                    Ok(PrefixAction::Pending)
                );
                assert_eq!(prefix.key(key(c)), Ok(PrefixAction::Pass));
            }
            assert_eq!(
                normal_motion(KeyEvent::new(KeyCode::Char(c), KeyModifiers::ALT)),
                None
            );
        }
    }

    #[test]
    fn repeat_prefix_counts_cancel_and_do_not_complete_an_operator() {
        let mut prefix = NormalPrefix::default();
        let key = |c| KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE);
        assert_eq!(prefix.key(key('.')), Ok(PrefixAction::Repeat(None)));
        for c in "12".chars() {
            prefix.key(key(c)).unwrap();
        }
        assert_eq!(prefix.key(key('.')), Ok(PrefixAction::Repeat(Some(12))));
        assert_eq!(prefix.key(key('.')), Ok(PrefixAction::Repeat(None)));
        for cancel in [
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
        ] {
            prefix.key(key('9')).unwrap();
            prefix.key(cancel).unwrap();
            assert_eq!(prefix.key(key('.')), Ok(PrefixAction::Repeat(None)));
        }
        for op in ['d', 'c', 'y'] {
            prefix.key(key(op)).unwrap();
            assert!(prefix.key(key('.')).is_err());
            assert_eq!(prefix.key(key('.')), Ok(PrefixAction::Repeat(None)));
        }
        for c in usize::MAX.to_string().chars() {
            prefix.key(key(c)).unwrap();
        }
        assert!(prefix.key(key('0')).is_err());
        assert_eq!(prefix.key(key('.')), Ok(PrefixAction::Repeat(None)));
    }

    #[test]
    fn normal_prefix_counts_cancel_and_reject_overflow_without_leaking() {
        let mut prefix = NormalPrefix::default();
        let key = |c| KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE);
        for (sequence, motion, count) in [
            ("10j", Motion::Down, 10),
            ("10w", Motion::WordForward, 10),
            ("2b", Motion::WordBackward, 2),
            ("3e", Motion::WordEnd, 3),
            ("10W", Motion::BigWordForward, 10),
            ("2B", Motion::BigWordBackward, 2),
            ("3E", Motion::BigWordEnd, 3),
            ("2$", Motion::End, 2),
            ("5G", Motion::Last, 5),
        ] {
            for c in sequence[..sequence.len() - 1].chars() {
                assert_eq!(prefix.key(key(c)), Ok(PrefixAction::Pending));
            }
            assert_eq!(
                prefix.key(key(sequence.chars().last().unwrap())),
                Ok(PrefixAction::Move(motion, count))
            );
            assert_eq!(prefix.key(key('0')), Ok(PrefixAction::Pass));
        }
        for (code, modifiers, motion) in [
            (KeyCode::Right, KeyModifiers::NONE, Motion::Right),
            (KeyCode::Char('f'), KeyModifiers::CONTROL, Motion::PageDown),
        ] {
            prefix.key(key('2')).unwrap();
            assert_eq!(
                prefix.key(KeyEvent::new(code, modifiers)),
                Ok(PrefixAction::Move(motion, 2))
            );
        }
        for (sequence, motion, count) in [
            ("d0", Motion::Home, None),
            ("dG", Motion::Last, None),
            ("1dG", Motion::Last, Some(1)),
            ("2d3l", Motion::Right, Some(6)),
            ("d10j", Motion::Down, Some(10)),
        ] {
            for c in sequence[..sequence.len() - 1].chars() {
                assert_eq!(prefix.key(key(c)), Ok(PrefixAction::Pending));
            }
            assert_eq!(
                prefix.key(key(sequence.chars().last().unwrap())),
                Ok(PrefixAction::Operate(Operator::Delete, Some(motion), count))
            );
            assert_eq!(prefix.key(key('0')), Ok(PrefixAction::Pass));
        }
        for (sequence, motion, count) in [
            ("cc", None, Some(1)),
            ("2c3c", None, Some(6)),
            ("c0", Some(Motion::Home), None),
            ("cG", Some(Motion::Last), None),
            ("c1G", Some(Motion::Last), Some(1)),
            ("2c3l", Some(Motion::Right), Some(6)),
        ] {
            for c in sequence[..sequence.len() - 1].chars() {
                assert_eq!(prefix.key(key(c)), Ok(PrefixAction::Pending));
            }
            assert_eq!(
                prefix.key(key(sequence.chars().last().unwrap())),
                Ok(PrefixAction::Operate(Operator::Change, motion, count))
            );
        }
        for end in [
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
        ] {
            prefix.key(key('c')).unwrap();
            assert_eq!(prefix.key(end), Ok(PrefixAction::Pending));
            assert_eq!(prefix.key(key('q')), Ok(PrefixAction::Pass));
        }
        for (sequence, motion, count) in [
            ("yy", None, Some(1)),
            ("2y3y", None, Some(6)),
            ("yG", Some(Motion::Last), None),
            ("y1G", Some(Motion::Last), Some(1)),
            ("y0", Some(Motion::Home), None),
            ("y2l", Some(Motion::Right), Some(2)),
        ] {
            for c in sequence[..sequence.len() - 1].chars() {
                assert_eq!(prefix.key(key(c)), Ok(PrefixAction::Pending));
            }
            assert_eq!(
                prefix.key(key(sequence.chars().last().unwrap())),
                Ok(PrefixAction::Operate(Operator::Yank, motion, count))
            );
        }
        prefix.key(key('2')).unwrap();
        assert_eq!(prefix.key(key('p')), Ok(PrefixAction::Put(false, 2)));
        assert_eq!(prefix.key(key('P')), Ok(PrefixAction::Put(true, 1)));
        prefix.key(key('y')).unwrap();
        assert_eq!(
            prefix.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
            Ok(PrefixAction::Pending)
        );
        assert_eq!(prefix.key(key('p')), Ok(PrefixAction::Put(false, 1)));
        for (sequence, count) in [("dd", 1), ("10dd", 10), ("d20d", 20), ("2d3d", 6)] {
            for c in sequence[..sequence.len() - 1].chars() {
                assert_eq!(prefix.key(key(c)), Ok(PrefixAction::Pending));
            }
            assert_eq!(
                prefix.key(key('d')),
                Ok(PrefixAction::Operate(Operator::Delete, None, Some(count)))
            );
        }
        for (sequence, count) in [("J", 2), ("1J", 2), ("3J", 3)] {
            for c in sequence[..sequence.len() - 1].chars() {
                assert_eq!(prefix.key(key(c)), Ok(PrefixAction::Pending));
            }
            assert_eq!(prefix.key(key('J')), Ok(PrefixAction::Join(count)));
            assert_eq!(prefix.key(key('0')), Ok(PrefixAction::Pass));
        }
        for sequence in [
            "2U", "3C", "dq", "dg", "9x", "dJ", "2C", "3D", "dr", "cd", "dc", "cq", "yd", "cy",
            "yp",
        ] {
            for c in sequence[..sequence.len() - 1].chars() {
                prefix.key(key(c)).unwrap();
            }
            assert!(prefix.key(key(sequence.chars().last().unwrap())).is_err());
            assert_eq!(prefix.key(key('0')), Ok(PrefixAction::Pass));
        }
        for cancel in [
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
        ] {
            prefix.key(key('4')).unwrap();
            prefix.key(key('d')).unwrap();
            assert_eq!(prefix.key(cancel), Ok(PrefixAction::Pending));
            assert_eq!(prefix.key(key('q')), Ok(PrefixAction::Pass));
        }
        for c in usize::MAX.to_string().chars() {
            prefix.key(key(c)).unwrap();
        }
        assert!(prefix.key(key('0')).is_err());
        assert_eq!(prefix.key(key('i')), Ok(PrefixAction::Insert('i', 1)));
        for c in usize::MAX.to_string().chars() {
            prefix.key(key(c)).unwrap();
        }
        prefix.key(key('d')).unwrap();
        prefix.key(key('2')).unwrap();
        assert!(prefix.key(key('d')).is_err());
        assert_eq!(prefix.key(key('q')), Ok(PrefixAction::Pass));
    }

    #[test]
    fn register_prefix_survives_operator_and_cancels_cleanly() {
        let key = |c| KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE);
        let mut prefix = NormalPrefix::default();
        assert_eq!(prefix.key(key('"')), Ok(PrefixAction::Pending));
        assert_eq!(prefix.key(key('a')), Ok(PrefixAction::Pending));
        assert_eq!(prefix.key(key('d')), Ok(PrefixAction::Pending));
        assert_eq!(
            prefix.key(key('w')),
            Ok(PrefixAction::Operate(
                Operator::Delete,
                Some(Motion::WordForward),
                None
            ))
        );
        assert_eq!(prefix.take_register(), Some('a'));
        assert!(!prefix.is_pending());

        assert_eq!(prefix.key(key('"')), Ok(PrefixAction::Pending));
        assert!(prefix.key(key('0')).is_err());
        assert!(!prefix.is_pending());
        assert_eq!(prefix.key(key('p')), Ok(PrefixAction::Put(false, 1)));
    }

    #[test]
    fn macro_prefix_accepts_named_numbered_and_previous_registers() {
        let key = |c| KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE);
        let mut prefix = NormalPrefix::default();
        assert_eq!(prefix.key(key('@')), Ok(PrefixAction::Pending));
        assert_eq!(prefix.key(key('a')), Ok(PrefixAction::Macro('a')));
        assert_eq!(prefix.key(key('@')), Ok(PrefixAction::Pending));
        assert_eq!(prefix.key(key('@')), Ok(PrefixAction::Macro('@')));
        assert_eq!(prefix.key(key('@')), Ok(PrefixAction::Pending));
        assert_eq!(prefix.key(key('1')), Ok(PrefixAction::Macro('1')));
    }

    #[test]
    fn character_find_prefixes_support_counts_repeats_and_operators() {
        let key = |c| KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE);
        let mut prefix = NormalPrefix::default();
        prefix.key(key('2')).unwrap();
        assert_eq!(prefix.key(key('f')), Ok(PrefixAction::Pending));
        assert_eq!(
            prefix.key(key('x')),
            Ok(PrefixAction::Find {
                forward: true,
                till: false,
                character: 'x',
                count: 2,
                operator: None,
            })
        );
        assert_eq!(
            prefix.key(key(';')),
            Ok(PrefixAction::RepeatFind {
                reverse: false,
                count: 1,
                operator: None,
            })
        );
        prefix.key(key('2')).unwrap();
        prefix.key(key('d')).unwrap();
        prefix.key(key('f')).unwrap();
        assert_eq!(
            prefix.key(key('x')),
            Ok(PrefixAction::Find {
                forward: true,
                till: false,
                character: 'x',
                count: 1,
                operator: Some((Operator::Delete, Some(2))),
            })
        );
        prefix.key(key('d')).unwrap();
        assert_eq!(
            prefix.key(key(';')),
            Ok(PrefixAction::RepeatFind {
                reverse: false,
                count: 1,
                operator: Some((Operator::Delete, None)),
            })
        );
        assert!(!prefix.is_pending());
    }
}
