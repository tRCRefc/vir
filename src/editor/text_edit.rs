//! A staged character-edit session. Dropping it cancels; commit creates one undo.
use super::Editor;
use crate::buffer::{Position, TextBuffer};

pub struct TextEdit<'a> {
    editor: &'a mut Editor,
    draft: TextBuffer,
    cursor: Position,
}

impl Editor {
    pub fn begin_text_edit(&mut self, cursor: Position) -> Result<TextEdit<'_>, String> {
        self.buffer
            .validate_position(cursor)
            .map_err(str::to_owned)?;
        Ok(TextEdit {
            draft: self.buffer.clone(),
            editor: self,
            cursor,
        })
    }
}

impl TextEdit<'_> {
    pub fn buffer(&self) -> &TextBuffer {
        &self.draft
    }
    pub fn cursor(&self) -> Position {
        self.cursor
    }

    pub fn open_line_below(&mut self) -> Result<(), String> {
        let row = if self.draft.is_empty() {
            0
        } else {
            self.cursor.row + 1
        };
        self.draft.insert_lines(row, [""]).map_err(str::to_owned)?;
        self.cursor = Position { row, column: 0 };
        Ok(())
    }

    pub fn insert(&mut self, text: &str) -> Result<(), String> {
        self.cursor = self
            .draft
            .replace_text(self.cursor, self.cursor, text)
            .map_err(str::to_owned)?;
        Ok(())
    }

    pub fn backspace(&mut self) -> Result<(), String> {
        let start = if self.cursor.column > 0 {
            Position {
                column: self.cursor.column - 1,
                ..self.cursor
            }
        } else if self.cursor.row > 0 {
            Position {
                row: self.cursor.row - 1,
                column: self
                    .draft
                    .line(self.cursor.row - 1)
                    .unwrap()
                    .chars()
                    .count(),
            }
        } else {
            return Ok(());
        };
        self.cursor = self
            .draft
            .replace_text(start, self.cursor, "")
            .map_err(str::to_owned)?;
        Ok(())
    }

    pub fn delete(&mut self) -> Result<(), String> {
        let length = self
            .draft
            .line(self.cursor.row)
            .unwrap_or("")
            .chars()
            .count();
        let end = if self.cursor.column < length {
            Position {
                column: self.cursor.column + 1,
                ..self.cursor
            }
        } else if self.cursor.row + 1 < self.draft.line_count() {
            Position {
                row: self.cursor.row + 1,
                column: 0,
            }
        } else {
            return Ok(());
        };
        self.draft
            .replace_text(self.cursor, end, "")
            .map_err(str::to_owned)?;
        Ok(())
    }

    pub fn commit(self) -> Result<Position, String> {
        if self.draft != self.editor.buffer {
            let row = self.cursor.row;
            self.editor.edit(move |buffer| {
                *buffer = self.draft;
                Ok(if buffer.is_empty() { None } else { Some(row) })
            })?;
        }
        Ok(self.cursor)
    }
}
