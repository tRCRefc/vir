use super::VisualState;
use crate::{
    buffer::Position,
    editor::{Editor, TextEdit},
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
        let mut edit = editor.begin_text_edit(Position {
            row: self.row,
            column,
        })?;
        if mode == 'C' {
            edit.delete_to_line_end()?;
        }
        if mode == 'o' {
            edit.open_line_below()?;
        } else if mode == 'O' {
            edit.open_line_above()?;
        }
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
        let mut edit = editor.begin_text_edit(Position {
            row: self.row,
            column: self.column,
        })?;
        edit.delete()?;
        edit.commit()?;
        self.column = self.column.min(length.saturating_sub(2));
        self.preferred_column = self.column;
        Ok(())
    }

    pub fn replace_character(
        &mut self,
        editor: &mut Editor,
        replacement: char,
    ) -> Result<(), String> {
        if replacement == '\n' || replacement == '\r' || replacement.is_control() {
            return Err("replacement must be a printable character".into());
        }
        let length = editor.buffer().line(self.row).unwrap_or("").chars().count();
        if self.column >= length {
            return Err("cursor is at end of line".into());
        }
        let mut edit = editor.begin_text_edit(Position {
            row: self.row,
            column: self.column,
        })?;
        edit.delete()?;
        edit.insert(&replacement.to_string())?;
        edit.commit()?;
        Ok(())
    }
}
