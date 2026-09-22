//! Shared character/line ranges for visual operators.
use super::{
    Motion, VisualState,
    motion::{Endpoint, word_endpoint},
    repeat::ChangeRecord,
};
use crate::{
    buffer::{Position, TextBuffer},
    editor::{Editor, Register, TextEdit},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Operator {
    Delete,
    Change,
    Yank,
}

impl Operator {
    pub(super) fn from_key(key: char) -> Option<Self> {
        match key {
            'd' => Some(Self::Delete),
            'c' => Some(Self::Change),
            'y' => Some(Self::Yank),
            _ => None,
        }
    }
}

impl Motion {
    pub(super) fn supports_operator(self) -> bool {
        !matches!(self, Self::First | Self::PageUp | Self::PageDown)
    }
}

enum EditRange {
    Lines(usize, usize),
    Characters(Position, Position),
}

impl EditRange {
    fn from_word_endpoint(
        buffer: &TextBuffer,
        origin: Position,
        endpoint: Endpoint,
        operator: Operator,
    ) -> Result<Self, String> {
        let (target, inclusive) = match endpoint {
            Endpoint::Exclusive(target) => (target, false),
            Endpoint::Inclusive(target) => (target, true),
        };
        let start = origin.min(target);
        let mut end = origin.max(target);
        let starts_in_indent = buffer
            .line(start.row)
            .unwrap()
            .chars()
            .take(start.column)
            .all(char::is_whitespace);
        // Exclusive motions ending at column zero do not consume that line.
        if !inclusive && start.row < end.row && end.column == 0 {
            if starts_in_indent {
                return Ok(Self::Lines(start.row, end.row - 1));
            }
            end.row -= 1;
            end.column = buffer.line(end.row).unwrap().chars().count();
        } else if inclusive {
            end.column = end
                .column
                .saturating_add(1)
                .min(buffer.line(end.row).unwrap().chars().count());
        }
        // A multi-line delete covering all nonblank text removes whole lines.
        // Yank/change keep their character range, unless the exclusive rule
        // above already made the motion linewise.
        if operator == Operator::Delete
            && start.row < end.row
            && starts_in_indent
            && buffer
                .line(end.row)
                .unwrap()
                .chars()
                .skip(end.column)
                .all(char::is_whitespace)
        {
            return Ok(Self::Lines(start.row, end.row));
        }
        if start == end {
            return Err("empty operator motion".into());
        }
        Ok(Self::Characters(start, end))
    }

    fn capture(&self, editor: &Editor) -> Result<Register, String> {
        match *self {
            Self::Lines(start, end) => Register::lines(editor.buffer(), start, end),
            Self::Characters(start, end) => Register::characters(editor.buffer(), start, end),
        }
    }
}

impl VisualState {
    pub fn delete_lines(&mut self, editor: &mut Editor, count: usize) -> Result<(), String> {
        let (start, end) = self.counted_lines(editor.buffer(), count)?;
        let register = Register::lines(editor.buffer(), start, end)?;
        editor.delete_lines(self.row, end)?;
        editor.set_line_undo(self.row, self.row.saturating_sub(1));
        editor.unnamed = Some(register);
        self.row = editor.current().unwrap_or(0);
        self.column = 0;
        self.preferred_column = 0;
        self.left = 0;
        self.top = self.top.min(self.row);
        self.last_change = Some(ChangeRecord::Delete {
            motion: None,
            count: Some(count.max(1)),
        });
        Ok(())
    }
}

impl VisualState {
    /// Inclusive range for dd/cc/yy; oversized counts stop at EOF.
    fn counted_lines(&self, buffer: &TextBuffer, count: usize) -> Result<(usize, usize), String> {
        if buffer.is_empty() {
            return Err("EmptyBuffer".into());
        }
        let end = self
            .row
            .saturating_add(count.max(1) - 1)
            .min(buffer.line_count() - 1);
        Ok((self.row, end))
    }

    fn motion_range(
        &self,
        editor: &Editor,
        motion: Motion,
        count: Option<usize>,
        operator: Operator,
    ) -> Result<EditRange, String> {
        let buffer = editor.buffer();
        let position = Position {
            row: self.row,
            column: self.column,
        };
        buffer.validate_position(position).map_err(str::to_owned)?;
        if buffer.is_empty() {
            return Err("EmptyBuffer".into());
        }
        let steps = count.unwrap_or(1).max(1);
        if let Some(endpoint) = word_endpoint(buffer, position, motion, steps, operator) {
            return EditRange::from_word_endpoint(buffer, position, endpoint, operator);
        }
        let last = buffer.line_count() - 1;
        let length = buffer.line(self.row).unwrap().chars().count();
        if matches!(motion, Motion::Up | Motion::Down | Motion::Last) {
            let target = match motion {
                Motion::Up => self.row.checked_sub(steps),
                Motion::Down => self.row.checked_add(steps).filter(|row| *row <= last),
                _ => Some(count.map_or(last, |_| steps - 1)).filter(|row| *row <= last),
            }
            .ok_or("operator motion out of bounds")?;
            Ok(EditRange::Lines(self.row.min(target), self.row.max(target)))
        } else {
            let (start, end) = match motion {
                Motion::Left => (
                    Position {
                        column: self.column.saturating_sub(steps),
                        ..position
                    },
                    position,
                ),
                Motion::Right => (
                    position,
                    Position {
                        column: self.column.saturating_add(steps).min(length),
                        ..position
                    },
                ),
                Motion::Home => (
                    Position {
                        column: 0,
                        ..position
                    },
                    position,
                ),
                Motion::End => {
                    let row = self
                        .row
                        .checked_add(steps - 1)
                        .filter(|row| *row <= last)
                        .ok_or("operator motion out of bounds")?;
                    (
                        position,
                        Position {
                            row,
                            column: buffer.line(row).unwrap().chars().count(),
                        },
                    )
                }
                _ => return Err("unsupported operator motion".into()),
            };
            if start == end {
                return Err("empty operator motion".into());
            }
            Ok(EditRange::Characters(start, end))
        }
    }

    pub fn delete_motion(
        &mut self,
        editor: &mut Editor,
        motion: Motion,
        count: Option<usize>,
    ) -> Result<(), String> {
        let range = self.motion_range(editor, motion, count, Operator::Delete)?;
        let register = range.capture(editor)?;
        match range {
            EditRange::Lines(start, end) => {
                editor.delete_lines(start, end)?;
                editor.set_line_undo(start, start.saturating_sub(1));
                self.row = editor.current().unwrap_or(0);
                self.column = 0;
            }
            EditRange::Characters(start, end) => {
                let mut edit = editor.begin_text_edit(start)?;
                edit.record_character_replacement(start, end);
                edit.delete_range(start, end)?;
                edit.commit()?;
                self.row = start.row;
                self.column = start.column.min(
                    editor
                        .buffer()
                        .line(self.row)
                        .unwrap_or("")
                        .chars()
                        .count()
                        .saturating_sub(1),
                );
            }
        }
        editor.unnamed = Some(register);
        self.preferred_column = self.column;
        self.top = self.top.min(self.row);
        self.left = 0;
        self.last_change = Some(ChangeRecord::Delete {
            motion: Some(motion),
            count,
        });
        Ok(())
    }

    pub fn put(&mut self, editor: &mut Editor, before: bool, count: usize) -> Result<(), String> {
        let register = editor
            .unnamed_register()
            .ok_or("register is empty")?
            .clone();
        let position = Position {
            row: self.row,
            column: self.column,
        };
        editor
            .buffer()
            .validate_position(position)
            .map_err(str::to_owned)?;
        let count = count.max(1);
        match register {
            Register::Characters(text) => {
                if text.is_empty() {
                    return Err("register is empty".into());
                }
                let capacity = text.len().checked_mul(count).ok_or("paste too large")?;
                let mut repeated = String::new();
                repeated
                    .try_reserve_exact(capacity)
                    .map_err(|_| "paste too large")?;
                for _ in 0..count {
                    repeated.push_str(&text);
                }
                let length = editor.buffer().line(self.row).unwrap_or("").chars().count();
                let column = if before {
                    self.column
                } else {
                    self.column.saturating_add(1).min(length)
                };
                let mut edit = editor.begin_text_edit(Position {
                    row: self.row,
                    column,
                })?;
                if repeated.contains('\n') {
                    edit.record_line_replacement(self.row, self.row);
                }
                edit.insert(&repeated)?;
                self.finish_insert(edit.commit()?);
            }
            Register::Lines(lines) => {
                let capacity = lines.len().checked_mul(count).ok_or("paste too large")?;
                let mut repeated = Vec::new();
                repeated
                    .try_reserve_exact(capacity)
                    .map_err(|_| "paste too large")?;
                for _ in 0..count {
                    repeated.extend(lines.iter().cloned());
                }
                let row = if editor.buffer().is_empty() {
                    0
                } else {
                    self.row + usize::from(!before)
                };
                editor.insert_lines(row, repeated)?;
                editor.set_line_undo(row.saturating_sub(1), row);
                // insert_lines places dot after the inserted block; visual put starts at its first line.
                editor.set_current(row).map_err(|e| format!("{e:?}"))?;
                self.row = row;
                self.column = editor
                    .buffer()
                    .line(row)
                    .unwrap()
                    .chars()
                    .position(|c| c != ' ' && c != '\t')
                    .unwrap_or(0);
            }
        }
        self.preferred_column = self.column;
        self.top = self.top.min(self.row);
        self.left = 0;
        self.last_change = Some(ChangeRecord::Put { before, count });
        Ok(())
    }

    pub fn yank(
        &self,
        editor: &mut Editor,
        motion: Option<Motion>,
        count: Option<usize>,
    ) -> Result<(), String> {
        let range = if let Some(motion) = motion {
            self.motion_range(editor, motion, count, Operator::Yank)?
        } else {
            let (start, end) = self.counted_lines(editor.buffer(), count.unwrap_or(1))?;
            EditRange::Lines(start, end)
        };
        editor.unnamed = Some(range.capture(editor)?);
        Ok(())
    }

    /// None selects cc; motions share range resolution with operator-specific policies.
    pub fn begin_change<'a>(
        &self,
        editor: &'a mut Editor,
        motion: Option<Motion>,
        count: Option<usize>,
    ) -> Result<TextEdit<'a>, String> {
        if matches!(motion, Some(Motion::End)) && count.unwrap_or(1) <= 1 {
            return self.begin_insert(editor, 'C');
        }
        let range = if let Some(motion) = motion {
            self.motion_range(editor, motion, count, Operator::Change)?
        } else {
            editor
                .buffer()
                .validate_position(Position {
                    row: self.row,
                    column: self.column,
                })
                .map_err(str::to_owned)?;
            if editor.buffer().is_empty() {
                let mut edit = editor.begin_text_edit(Position::default())?;
                edit.record_input();
                return Ok(edit);
            }
            let (start, end) = self.counted_lines(editor.buffer(), count.unwrap_or(1))?;
            EditRange::Lines(start, end)
        };
        let register = range.capture(editor)?;
        let replaced_lines = match range {
            EditRange::Lines(start, end) => Some((start, end)),
            EditRange::Characters(..) => None,
        };
        let (start, end) = match range {
            EditRange::Characters(start, end) => (start, end),
            EditRange::Lines(start, end) => (
                Position {
                    row: start,
                    column: 0,
                },
                Position {
                    row: end,
                    column: editor.buffer().line(end).unwrap().chars().count(),
                },
            ),
        };
        let mut edit = editor.begin_text_edit(start)?;
        if let Some((first, last)) = replaced_lines {
            if first < last {
                edit.record_line_replacement(first, last);
            }
        } else {
            edit.record_character_replacement(start, end);
        }
        edit.delete_range(start, end)?;
        edit.stage_register(register);
        edit.record_input();
        Ok(edit)
    }
}
