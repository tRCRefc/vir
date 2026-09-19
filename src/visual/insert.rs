use super::{Motion, VisualState, repeat::ChangeRecord};
use crate::{
    buffer::Position,
    editor::{Editor, Register, TextEdit},
};

impl VisualState {
    pub fn join_lines(&mut self, editor: &mut Editor, count: usize) -> Result<(), String> {
        let mut edit = editor.begin_text_edit(Position {
            row: self.row,
            column: self.column,
        })?;
        edit.join_lines(count)?;
        let position = edit.commit()?;
        self.column = position.column;
        self.preferred_column = self.column;
        self.last_change = Some(ChangeRecord::Join(count.max(2)));
        Ok(())
    }

    pub fn begin_insert<'a>(
        &self,
        editor: &'a mut Editor,
        mode: char,
    ) -> Result<TextEdit<'a>, String> {
        if !matches!(mode, 'i' | 'a' | 'o' | 'I' | 'A' | 'O' | 'C' | 'R') {
            return Err("unsupported insert mode".into());
        }
        let length = editor.buffer().line(self.row).unwrap_or("").chars().count();
        let column = if mode == 'a' || mode == 'A' {
            if mode == 'A' {
                length
            } else {
                self.column.saturating_add(1).min(length)
            }
        } else if mode == 'I' {
            editor
                .buffer()
                .line(self.row)
                .unwrap_or("")
                .chars()
                .position(|c| c != ' ' && c != '\t')
                .unwrap_or(length)
        } else {
            self.column
        };
        let register = if mode == 'C' && column < length {
            Some(Register::characters(
                editor.buffer(),
                Position {
                    row: self.row,
                    column,
                },
                Position {
                    row: self.row,
                    column: length,
                },
            )?)
        } else {
            None
        };
        let mut edit = editor.begin_text_edit(Position {
            row: self.row,
            column,
        })?;
        if mode == 'C' {
            edit.delete_to_line_end()?;
            if let Some(register) = register {
                edit.stage_register(register);
            }
        }
        if mode == 'o' {
            edit.open_line_below()?;
        } else if mode == 'O' {
            edit.open_line_above()?;
        }
        edit.record_input();
        Ok(edit)
    }

    pub fn finish_insert(&mut self, position: Position) {
        self.row = position.row;
        self.column = position.column.saturating_sub(1);
        self.preferred_column = self.column;
    }

    pub fn delete_character(&mut self, editor: &mut Editor) -> Result<(), String> {
        let length = editor.buffer().line(self.row).unwrap_or("").chars().count();
        if self.column >= length {
            return Ok(());
        }
        let register = Register::characters(
            editor.buffer(),
            Position {
                row: self.row,
                column: self.column,
            },
            Position {
                row: self.row,
                column: self.column + 1,
            },
        )?;
        let mut edit = editor.begin_text_edit(Position {
            row: self.row,
            column: self.column,
        })?;
        edit.delete()?;
        edit.stage_register(register);
        edit.commit()?;
        self.column = self.column.min(length.saturating_sub(2));
        self.preferred_column = self.column;
        self.last_change = Some(ChangeRecord::Delete {
            motion: Some(Motion::Right),
            count: Some(1),
        });
        Ok(())
    }

    pub fn replace_character(
        &mut self,
        editor: &mut Editor,
        replacement: char,
    ) -> Result<(), String> {
        self.replace_counted(editor, replacement, 1)
    }

    /// Replace a counted range on the current line in one undo transaction.
    pub fn replace_counted(
        &mut self,
        editor: &mut Editor,
        replacement: char,
        count: usize,
    ) -> Result<(), String> {
        if replacement == '\0' || replacement == '\r' {
            return Err("NUL and carriage return replacement are unsupported".into());
        }
        let count = count.max(1);
        let end = self.replacement_end(editor, count)?;
        let changed = editor
            .buffer()
            .line(self.row)
            .unwrap()
            .chars()
            .skip(self.column)
            .take(count)
            .any(|c| c != replacement);
        if !changed {
            return Ok(());
        }
        let mut edit = editor.begin_text_edit(Position {
            row: self.row,
            column: self.column,
        })?;
        edit.delete_range(
            Position {
                row: self.row,
                column: self.column,
            },
            Position {
                row: self.row,
                column: end,
            },
        )?;
        if replacement == '\n' {
            edit.insert("\n")?;
        } else {
            edit.insert(&replacement.to_string().repeat(count))?;
        }
        let position = edit.commit()?;
        self.row = position.row;
        self.column = position.column.saturating_sub(1);
        self.preferred_column = self.column;
        self.last_change = Some(ChangeRecord::Replace {
            character: replacement,
            count,
        });
        Ok(())
    }
    // Historical vrep checks the range before reading the replacement key.
    pub(super) fn replacement_end(&self, editor: &Editor, count: usize) -> Result<usize, String> {
        let length = editor.buffer().line(self.row).unwrap_or("").chars().count();
        self.column
            .checked_add(count.max(1))
            .filter(|end| *end <= length)
            .ok_or_else(|| "not enough characters to replace".into())
    }
}
