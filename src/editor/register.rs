use crate::buffer::{Position, TextBuffer};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Register {
    Characters(String),
    Lines(Vec<String>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RegisterBank {
    named: Vec<Option<Register>>,
    numbered: Vec<Option<Register>>,
}

impl Default for RegisterBank {
    fn default() -> Self {
        Self {
            named: vec![None; 26],
            numbered: vec![None; 9],
        }
    }
}

impl RegisterBank {
    pub(crate) fn named(&self, name: char) -> Option<&Register> {
        name.to_ascii_lowercase()
            .is_ascii_lowercase()
            .then(|| self.named[(name.to_ascii_lowercase() as u8 - b'a') as usize].as_ref())
            .flatten()
    }

    pub(crate) fn set_named(
        &mut self,
        name: char,
        register: Register,
        append: bool,
    ) -> Result<(), String> {
        let lower = name.to_ascii_lowercase();
        if !lower.is_ascii_lowercase() {
            return Err("invalid named register".into());
        }
        let slot = &mut self.named[(lower as u8 - b'a') as usize];
        *slot = if append {
            Some(append_register(slot.take(), register)?)
        } else {
            Some(register)
        };
        Ok(())
    }

    pub(crate) fn numbered(&self, number: usize) -> Option<&Register> {
        (1..=9)
            .contains(&number)
            .then(|| self.numbered[number - 1].as_ref())
            .flatten()
    }

    pub(crate) fn push_numbered(&mut self, register: Register) {
        self.numbered.rotate_right(1);
        self.numbered[0] = Some(register);
    }
}

fn append_register(previous: Option<Register>, next: Register) -> Result<Register, String> {
    match (previous, next) {
        (None, next) => Ok(next),
        (Some(Register::Characters(mut left)), Register::Characters(right)) => {
            left.push_str(&right);
            Ok(Register::Characters(left))
        }
        (Some(Register::Lines(mut left)), Register::Lines(right)) => {
            left.extend(right);
            Ok(Register::Lines(left))
        }
        _ => Err("cannot append registers of different kinds".into()),
    }
}

impl Register {
    pub(crate) fn characters(
        buffer: &TextBuffer,
        start: Position,
        end: Position,
    ) -> Result<Self, String> {
        buffer.validate_position(start).map_err(str::to_owned)?;
        buffer.validate_position(end).map_err(str::to_owned)?;
        if start > end {
            return Err("reversed register range".into());
        }
        let mut text = String::new();
        for row in start.row..=end.row {
            if row > start.row {
                text.push('\n');
            }
            let line = buffer.line(row).unwrap_or("");
            let from = if row == start.row { start.column } else { 0 };
            let to = if row == end.row {
                end.column
            } else {
                line.chars().count()
            };
            text.extend(line.chars().skip(from).take(to - from));
        }
        Ok(Self::Characters(text))
    }

    pub(crate) fn lines(buffer: &TextBuffer, start: usize, end: usize) -> Result<Self, String> {
        if start > end || end >= buffer.line_count() {
            return Err("register range out of bounds".into());
        }
        Ok(Self::Lines(
            (start..=end)
                .map(|row| buffer.line(row).unwrap().to_owned())
                .collect(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_registers_overwrite_and_append() {
        let mut bank = RegisterBank::default();
        bank.set_named('a', Register::Characters("one".into()), false)
            .unwrap();
        bank.set_named('A', Register::Characters(" two".into()), true)
            .unwrap();
        assert_eq!(
            bank.named('a'),
            Some(&Register::Characters("one two".into()))
        );
        bank.set_named('a', Register::Lines(vec!["line".into()]), false)
            .unwrap();
        assert_eq!(bank.named('a'), Some(&Register::Lines(vec!["line".into()])));
    }

    #[test]
    fn numbered_registers_shift_newest_to_one() {
        let mut bank = RegisterBank::default();
        for text in ["one", "two"] {
            bank.push_numbered(Register::Characters(text.into()));
        }
        assert_eq!(bank.numbered(1), Some(&Register::Characters("two".into())));
        assert_eq!(bank.numbered(2), Some(&Register::Characters("one".into())));
        assert_eq!(bank.numbered(0), None);
    }
}
