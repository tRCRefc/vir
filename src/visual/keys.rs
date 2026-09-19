//! Normal-mode counts and command prefixes; no terminal I/O or text mutation.
use super::{InputStart, Motion, input::MAX_INPUT_WORK, operator::Operator};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Default)]
pub(super) struct NormalPrefix {
    digits: usize,
    operator_count: Option<usize>,
    operator: Option<Operator>,
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
}

impl NormalPrefix {
    pub(super) fn is_pending(&self) -> bool {
        self.digits != 0 || self.operator_count.is_some()
    }

    pub(super) fn key(&mut self, key: KeyEvent) -> Result<PrefixAction, &'static str> {
        let active = self.digits != 0 || self.operator_count.is_some();
        if self.digits != 0
            && self.operator_count.is_none()
            && !matches!(key.code, KeyCode::Char('0'..='9'))
            && let Some(motion) = normal_motion(key)
        {
            let count = self.digits;
            *self = Self::default();
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
        if let KeyCode::Char(c @ '0'..='9') = key.code
            && (c != '0' || self.digits != 0)
        {
            let next = self
                .digits
                .checked_mul(10)
                .and_then(|n| n.checked_add((c as u8 - b'0') as usize));
            if let Some(next) = next {
                self.digits = next;
                return Ok(PrefixAction::Pending);
            }
            *self = Self::default();
            return Err("count too large");
        }
        if let KeyCode::Char(c) = key.code
            && let Some(requested) = Operator::from_key(c)
        {
            if let Some(before) = self.operator_count {
                let operator = self.operator.unwrap();
                let matches_operator = requested == operator;
                let count = before.max(1).checked_mul(self.digits.max(1));
                *self = Self::default();
                if !matches_operator {
                    return Err("mixed operators; command cancelled");
                }
                return count
                    .map(|n| PrefixAction::Operate(operator, None, Some(n)))
                    .ok_or("count too large");
            }
            self.operator = Some(requested);
            self.operator_count = Some(self.digits);
            self.digits = 0;
            return Ok(PrefixAction::Pending);
        }
        if let Some(before) = self.operator_count {
            let count = before.max(1).checked_mul(self.digits.max(1));
            let explicit = before != 0 || self.digits != 0;
            let operator = self.operator.unwrap();
            *self = Self::default();
            let count = count.ok_or("count too large")?;
            if let Some(motion) = normal_motion(key).filter(|motion| motion.supports_operator()) {
                return Ok(PrefixAction::Operate(
                    operator,
                    Some(motion),
                    explicit.then_some(count),
                ));
            }
            return Err("unsupported operator motion; command cancelled");
        }
        if let KeyCode::Char(mode @ ('i' | 'a' | 'I' | 'A' | 'o' | 'O' | 'R')) = key.code {
            let count = self.digits.max(1);
            *self = Self::default();
            if InputStart::Insert(mode).input_count(count) > MAX_INPUT_WORK {
                return Err("input repeat too large");
            }
            return Ok(PrefixAction::Insert(mode, count));
        }
        if key.code == KeyCode::Char('r') {
            let count = self.digits.max(1);
            *self = Self::default();
            return Ok(PrefixAction::Replace(count));
        }
        if key.code == KeyCode::Char('.') {
            let count = (self.digits != 0).then_some(self.digits);
            *self = Self::default();
            return Ok(PrefixAction::Repeat(count));
        }
        if matches!(key.code, KeyCode::Char('p' | 'P')) {
            let count = self.digits.max(1);
            *self = Self::default();
            return Ok(PrefixAction::Put(key.code == KeyCode::Char('P'), count));
        }
        if key.code == KeyCode::Char('J') && self.operator_count.is_none() {
            let count = self.digits.max(2);
            *self = Self::default();
            return Ok(PrefixAction::Join(count));
        }
        *self = Self::default();
        if active {
            Err("unsupported count or operator; command cancelled")
        } else {
            Ok(PrefixAction::Pass)
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
            "3C", "dq", "dg", "9x", "dJ", "2C", "3D", "dr", "cd", "dc", "cq", "yd", "cy", "yp",
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
}
