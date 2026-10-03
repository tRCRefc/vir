//! Local marks travel with buffer snapshots, including ordinary and macro undo.
use super::{Position, TextBuffer};

impl TextBuffer {
    pub(crate) fn set_mark(&mut self, name: char, position: Position) -> Result<(), &'static str> {
        let slot = mark_slot(name)?;
        self.validate_position(position)?;
        let line = self
            .line(position.row)
            .ok_or("cannot mark an empty buffer")?;
        self.marks[slot] = Some(Position {
            column: position.column.min(line.chars().count().saturating_sub(1)),
            ..position
        });
        Ok(())
    }

    pub(crate) fn mark(&self, name: char) -> Result<Position, &'static str> {
        self.marks[mark_slot(name)?].ok_or("mark is not set")
    }

    pub(crate) fn mark_positions(&self) -> [Option<Position>; 26] {
        self.marks
    }

    pub(super) fn adjust_text_marks(&mut self, start: Position, end: Position, inserted: Position) {
        for position in self.marks.iter_mut().flatten() {
            if *position < start {
                continue;
            }
            if *position < end {
                // Replaced text has no surviving character anchor.
                *position = start;
            } else if position.row == end.row {
                position.row = inserted.row;
                position.column = inserted.column + position.column - end.column;
            } else {
                position.row = inserted.row + position.row - end.row;
            }
        }
        self.clamp_marks();
    }

    pub(super) fn adjust_line_marks(&mut self, start: usize, removed: usize, added: usize) {
        for mark in &mut self.marks {
            let Some(position) = mark else { continue };
            if position.row < start {
                continue;
            }
            if position.row < start + removed {
                if position.row - start >= added {
                    *mark = None;
                }
            } else {
                position.row = position.row - removed + added;
            }
        }
        self.clamp_marks();
    }

    pub(super) fn transfer_marks(&mut self, start: usize, end: usize, at: usize, copy: bool) {
        let count = end - start + 1;
        let destination = if !copy && at > end { at - count } else { at };
        for position in self.marks.iter_mut().flatten() {
            if !copy && (start..=end).contains(&position.row) {
                position.row = destination + position.row - start;
                continue;
            }
            if !copy && position.row > end {
                position.row -= count;
            }
            if position.row >= destination {
                position.row += count;
            }
        }
    }

    /// Joining retains characters; preserve each source line's column after
    /// whitespace trimming and separator insertion instead of treating it as
    /// replacement of the entire joined span.
    pub(crate) fn restore_join_marks(
        &mut self,
        original: [Option<Position>; 26],
        row: usize,
        offsets: &[(usize, usize)],
    ) {
        for (slot, mark) in original.into_iter().enumerate() {
            if let Some(position) = mark
                && position.row >= row
                && let Some(&(offset, trimmed)) = offsets.get(position.row - row)
            {
                self.marks[slot] = Some(Position {
                    row,
                    column: offset + position.column.saturating_sub(trimmed),
                });
            }
        }
        self.clamp_marks();
    }

    fn clamp_marks(&mut self) {
        for mark in &mut self.marks {
            if let Some(position) = mark {
                if let Some(line) = self.lines.get(position.row) {
                    position.column = position.column.min(line.chars().count().saturating_sub(1));
                } else {
                    *mark = None;
                }
            }
        }
    }
}

fn mark_slot(name: char) -> Result<usize, &'static str> {
    if name.is_ascii_lowercase() {
        Ok((name as u8 - b'a') as usize)
    } else {
        Err("marks must be lowercase a-z")
    }
}
