use super::VisualState;
use crate::{
    buffer::Position,
    editor::{Editor, TextEdit},
};

impl VisualState {
    pub fn begin_insert<'a>(
        &self,
        editor: &'a mut Editor,
        mode: char,
    ) -> Result<TextEdit<'a>, String> {
        if !matches!(mode, 'i' | 'a' | 'o' | 'I' | 'A' | 'O') {
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
            0
        } else {
            self.column
        };
        let mut edit = editor.begin_text_edit(Position {
            row: self.row,
            column,
        })?;
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
}
