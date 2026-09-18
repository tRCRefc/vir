//! Shared character/line ranges for visual operators.
use super::{Motion, VisualState};
use crate::{
    buffer::Position,
    editor::{Editor, TextEdit},
};

enum EditRange {
    Lines(usize, usize),
    Characters(Position, Position),
}

impl VisualState {
    fn motion_range(
        &self,
        editor: &Editor,
        motion: Motion,
        count: Option<usize>,
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
        let last = buffer.line_count() - 1;
        let length = buffer.line(self.row).unwrap().chars().count();
        if matches!(motion, Motion::Up | Motion::Down | Motion::Last) {
            let target = match motion {
                Motion::Up => self.row.checked_sub(steps),
                Motion::Down => self.row.checked_add(steps).filter(|row| *row <= last),
                _ => Some(count.map_or(last, |_| steps - 1)).filter(|row| *row <= last),
            }
            .ok_or("delete motion out of bounds")?;
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
                        .ok_or("delete motion out of bounds")?;
                    (
                        position,
                        Position {
                            row,
                            column: buffer.line(row).unwrap().chars().count(),
                        },
                    )
                }
                _ => return Err("unsupported delete motion".into()),
            };
            if start == end {
                return Err("empty delete motion".into());
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
        match self.motion_range(editor, motion, count)? {
            EditRange::Lines(start, end) => {
                editor.delete_lines(start, end)?;
                self.row = editor.current().unwrap_or(0);
                self.column = 0;
            }
            EditRange::Characters(start, end) => {
                let mut edit = editor.begin_text_edit(start)?;
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
        self.preferred_column = self.column;
        self.top = self.top.min(self.row);
        self.left = 0;
        Ok(())
    }

    /// None selects cc; a motion selects the same range rules as d.
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
            self.motion_range(editor, motion, count)?
        } else {
            editor
                .buffer()
                .validate_position(Position {
                    row: self.row,
                    column: self.column,
                })
                .map_err(str::to_owned)?;
            if editor.buffer().is_empty() {
                return editor.begin_text_edit(Position::default());
            }
            EditRange::Lines(
                self.row,
                self.row
                    .saturating_add(count.unwrap_or(1).max(1) - 1)
                    .min(editor.buffer().line_count() - 1),
            )
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
        edit.delete_range(start, end)?;
        Ok(edit)
    }
}
