//! ASCII bracket matching, deliberately independent of language syntax.
use super::VisualState;
use crate::buffer::{Position, TextBuffer};

pub(super) fn target(buffer: &TextBuffer, position: Position) -> Option<Position> {
    let (column, source) = buffer
        .line(position.row)?
        .chars()
        .enumerate()
        .skip(position.column)
        .find(|(_, c)| matches!(c, '(' | ')' | '[' | ']' | '{' | '}'))?;
    let (paired, forward) = match source {
        '(' => (')', true),
        '[' => (']', true),
        '{' => ('}', true),
        ')' => ('(', false),
        ']' => ('[', false),
        '}' => ('{', false),
        _ => unreachable!(),
    };
    let mut depth = 1usize;
    let mut closes = |c| {
        if c == source {
            depth += 1;
        }
        if c == paired {
            depth -= 1;
        }
        depth == 0
    };
    if forward {
        for row in position.row..buffer.line_count() {
            let skip = if row == position.row { column + 1 } else { 0 };
            for (column, c) in buffer.line(row)?.chars().enumerate().skip(skip) {
                if closes(c) {
                    return Some(Position { row, column });
                }
            }
        }
    } else {
        for row in (0..=position.row).rev() {
            let line = buffer.line(row)?;
            let length = line.chars().count();
            for (reverse, c) in line.chars().rev().enumerate() {
                let candidate = length - reverse - 1;
                if row == position.row && candidate >= column {
                    continue;
                }
                if closes(c) {
                    return Some(Position {
                        row,
                        column: candidate,
                    });
                }
            }
        }
    }
    None
}

impl VisualState {
    pub fn match_bracket(&mut self, buffer: &TextBuffer) -> Result<(), String> {
        let position = target(
            buffer,
            Position {
                row: self.row,
                column: self.column,
            },
        )
        .ok_or("no matching bracket")?;
        self.row = position.row;
        self.column = position.column;
        self.preferred_column = position.column;
        self.left = 0;
        self.top = self.top.min(self.row);
        Ok(())
    }
}
