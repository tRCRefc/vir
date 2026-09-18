//! Delete motion ranges are resolved before any editor state is changed.
use super::{Motion, VisualState};
use crate::{buffer::Position, editor::Editor};

impl VisualState {
    pub fn delete_motion(
        &mut self,
        editor: &mut Editor,
        motion: Motion,
        count: Option<usize>,
    ) -> Result<(), String> {
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
            editor.delete_lines(self.row.min(target), self.row.max(target))?;
            self.row = editor.current().unwrap_or(0);
            self.column = 0;
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
            let mut edit = editor.begin_text_edit(position)?;
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
        self.preferred_column = self.column;
        self.top = self.top.min(self.row);
        self.left = 0;
        Ok(())
    }
}
