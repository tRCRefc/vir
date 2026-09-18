//! Scalar-based word traversal, independent of terminal input and editing.
use super::{Motion, operator::Operator};
use crate::buffer::{Position, TextBuffer};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Class {
    Empty,
    Space,
    Word,
    Punctuation,
}

#[derive(Clone, Copy)]
struct Cursor<'a> {
    buffer: &'a TextBuffer,
    position: Position,
    byte: usize,
    big_word: bool,
}

impl Cursor<'_> {
    fn line(&self) -> &str {
        self.buffer.line(self.position.row).unwrap()
    }

    fn class(&self) -> Class {
        match self.line()[self.byte..].chars().next() {
            None => Class::Empty,
            Some(c) if c.is_whitespace() => Class::Space,
            Some(c) if self.big_word || c.is_alphanumeric() || c == '_' => Class::Word,
            Some(_) => Class::Punctuation,
        }
    }

    fn step(&mut self, forward: bool) -> bool {
        if forward {
            let next = self.byte
                + self.line()[self.byte..]
                    .chars()
                    .next()
                    .map_or(0, char::len_utf8);
            if next < self.line().len() {
                self.byte = next;
                self.position.column += 1;
            } else if self.position.row + 1 < self.buffer.line_count() {
                self.position.row += 1;
                self.position.column = 0;
                self.byte = 0;
            } else {
                return false;
            }
        } else if self.byte > 0 {
            self.byte -= self.line()[..self.byte]
                .chars()
                .next_back()
                .unwrap()
                .len_utf8();
            self.position.column -= 1;
        } else if self.position.row > 0 {
            self.position.row -= 1;
            self.byte = self.line().char_indices().next_back().map_or(0, |(i, _)| i);
            self.position.column = self.line().chars().count().saturating_sub(1);
        } else {
            return false;
        }
        true
    }

    fn forward_word(&mut self) {
        let class = self.class();
        let row = self.position.row;
        loop {
            if !self.step(true) {
                return;
            }
            if self.position.row != row || self.class() != class || class == Class::Empty {
                break;
            }
        }
        while self.class() == Class::Space {
            if !self.step(true) {
                break;
            }
        }
    }

    fn backward_word(&mut self) {
        if !self.step(false) {
            return;
        }
        while self.class() == Class::Space {
            if !self.step(false) {
                return;
            }
        }
        self.finish_run(false);
    }

    fn end_word(&mut self) {
        if !self.step(true) {
            return;
        }
        while matches!(self.class(), Class::Space | Class::Empty) {
            if !self.step(true) {
                return;
            }
        }
        self.finish_run(true);
    }

    fn finish_run(&mut self, forward: bool) {
        let class = self.class();
        loop {
            let mut next = *self;
            if !next.step(forward)
                || next.position.row != self.position.row
                || next.class() != class
            {
                break;
            }
            *self = next;
        }
    }
}

#[derive(Clone, Copy)]
enum WordDirection {
    Forward,
    Backward,
    End,
}

#[derive(Clone, Copy)]
struct WordMotion {
    direction: WordDirection,
    big_word: bool,
}

impl WordMotion {
    fn from_motion(motion: Motion) -> Option<Self> {
        let (direction, big_word) = match motion {
            Motion::WordForward => (WordDirection::Forward, false),
            Motion::WordBackward => (WordDirection::Backward, false),
            Motion::WordEnd => (WordDirection::End, false),
            Motion::BigWordForward => (WordDirection::Forward, true),
            Motion::BigWordBackward => (WordDirection::Backward, true),
            Motion::BigWordEnd => (WordDirection::End, true),
            _ => return None,
        };
        Some(Self {
            direction,
            big_word,
        })
    }

    fn advance(self, cursor: &mut Cursor<'_>) {
        match self.direction {
            WordDirection::Forward => cursor.forward_word(),
            WordDirection::Backward => cursor.backward_word(),
            WordDirection::End => cursor.end_word(),
        }
    }
}

impl<'a> Cursor<'a> {
    fn new(buffer: &'a TextBuffer, position: Position, big_word: bool) -> Option<Self> {
        let line = buffer.line(position.row)?;
        let (column, byte) = line
            .char_indices()
            .enumerate()
            .take(position.column.saturating_add(1))
            .last()
            .map_or((0, 0), |(column, (byte, _))| (column, byte));
        Some(Self {
            buffer,
            big_word,
            position: Position { column, ..position },
            byte,
        })
    }

    // The final forward segment of an operator stops at this line's end,
    // unlike ordinary navigation. Earlier counted segments can cross lines.
    fn forward_operator_end(&mut self) -> Endpoint {
        if self.class() == Class::Empty {
            self.forward_word();
            return Endpoint::Exclusive(self.position);
        }
        let class = self.class();
        loop {
            let next = self.byte + self.line()[self.byte..].chars().next().unwrap().len_utf8();
            self.position.column += 1;
            self.byte = next;
            if next == self.line().len() {
                return Endpoint::Exclusive(self.position);
            }
            if self.class() != class {
                break;
            }
        }
        while self.class() == Class::Space {
            self.byte += self.line()[self.byte..].chars().next().unwrap().len_utf8();
            self.position.column += 1;
        }
        Endpoint::Exclusive(self.position)
    }
}

/// The endpoint retains inclusion semantics until the operator normalizes it.
pub(super) enum Endpoint {
    Exclusive(Position),
    Inclusive(Position),
}

pub(super) fn word_target(
    buffer: &TextBuffer,
    position: Position,
    motion: Motion,
    count: usize,
) -> Position {
    let word = WordMotion::from_motion(motion).expect("word_target requires a word motion");
    let Some(mut cursor) = Cursor::new(buffer, position, word.big_word) else {
        return position;
    };
    for _ in 0..count {
        let before = cursor.position;
        word.advance(&mut cursor);
        if before == cursor.position {
            break;
        }
    }
    cursor.position
}

pub(super) fn word_endpoint(
    buffer: &TextBuffer,
    position: Position,
    motion: Motion,
    count: usize,
    operator: Operator,
) -> Option<Endpoint> {
    let word = WordMotion::from_motion(motion)?;
    let mut cursor = Cursor::new(buffer, position, word.big_word)?;
    // Change-forward from text consumes the current word, including a lone
    // final character, then word ends. From whitespace it retains forward
    // motion semantics. This is one semantic policy, independent of key names.
    let change_word = operator == Operator::Change
        && matches!(word.direction, WordDirection::Forward)
        && matches!(cursor.class(), Class::Word | Class::Punctuation);
    for index in 0..count {
        let before = cursor.position;
        if change_word {
            if index == 0 {
                cursor.finish_run(true);
            } else {
                cursor.end_word();
            }
        } else if matches!(word.direction, WordDirection::Forward) && index == count - 1 {
            return Some(cursor.forward_operator_end());
        } else {
            word.advance(&mut cursor);
        }
        if before == cursor.position && !(change_word && index == 0) {
            break;
        }
    }
    Some(
        if change_word
            || matches!(word.direction, WordDirection::End)
            || (matches!(word.direction, WordDirection::Forward) && cursor.class() != Class::Empty)
        {
            Endpoint::Inclusive(cursor.position)
        } else {
            Endpoint::Exclusive(cursor.position)
        },
    )
}
