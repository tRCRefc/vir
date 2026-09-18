//! Scalar-based word traversal, independent of terminal input and editing.
use super::Motion;
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

pub(super) fn word_target(
    buffer: &TextBuffer,
    position: Position,
    motion: Motion,
    count: usize,
) -> Position {
    let Some(line) = buffer.line(position.row) else {
        return position;
    };
    let (column, byte) = line
        .char_indices()
        .enumerate()
        .take(position.column.saturating_add(1))
        .last()
        .map_or((0, 0), |(column, (byte, _))| (column, byte));
    let mut cursor = Cursor {
        buffer,
        big_word: matches!(
            motion,
            Motion::BigWordForward | Motion::BigWordBackward | Motion::BigWordEnd
        ),
        position: Position { column, ..position },
        byte,
    };
    for _ in 0..count {
        let before = cursor.position;
        match motion {
            Motion::WordForward | Motion::BigWordForward => cursor.forward_word(),
            Motion::WordBackward | Motion::BigWordBackward => cursor.backward_word(),
            Motion::WordEnd | Motion::BigWordEnd => cursor.end_word(),
            _ => unreachable!("word_target requires a word motion"),
        }
        if before == cursor.position {
            break;
        }
    }
    cursor.position
}
