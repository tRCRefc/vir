//! A staged character-edit session. Dropping it cancels; commit creates one undo.
use super::Editor;
use crate::buffer::{Position, TextBuffer};

pub struct TextEdit<'a> {
    editor: &'a mut Editor,
    draft: TextBuffer,
    cursor: Position,
    pending_register: Option<super::Register>,
    replacements: Vec<(Position, Position, String, bool)>,
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
            replacements: Vec::new(),
            pending_register: None,
        })
    }
}

impl TextEdit<'_> {
    /// Type one scalar in Replace mode; LF splits rather than overwrites.
    pub fn replace_input(&mut self, character: char) -> Result<(), String> {
        if character.is_control() && character != '\n' && character != '\t' {
            return Err("unsupported replacement character".into());
        }
        let start = self.cursor;
        let old = if character == '\n' {
            None
        } else {
            self.draft
                .line(start.row)
                .and_then(|line| line.chars().nth(start.column))
        };
        let end = Position {
            column: start.column + usize::from(old.is_some()),
            ..start
        };
        let was_empty = self.draft.is_empty();
        self.cursor = self
            .draft
            .replace_text(start, end, &character.to_string())
            .map_err(str::to_owned)?;
        self.replacements.push((
            start,
            self.cursor,
            old.map_or_else(String::new, |c| c.to_string()),
            was_empty,
        ));
        Ok(())
    }

    /// Reverse the most recent Replace keystroke, stopping at session entry.
    pub fn replace_backspace(&mut self) -> Result<(), String> {
        if let Some((start, end, old, was_empty)) = self.replacements.last() {
            self.draft
                .replace_text(*start, *end, old)
                .map_err(str::to_owned)?;
            if *was_empty {
                self.draft = TextBuffer::default();
            }
            self.cursor = *start;
            self.replacements.pop();
        }
        Ok(())
    }

    pub(crate) fn stage_register(&mut self, register: super::Register) {
        self.pending_register = Some(register);
    }

    pub fn buffer(&self) -> &TextBuffer {
        &self.draft
    }

    pub fn delete_range(&mut self, start: Position, end: Position) -> Result<(), String> {
        self.cursor = self
            .draft
            .replace_text(start, end, "")
            .map_err(str::to_owned)?;
        self.replacements.clear();
        Ok(())
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

    pub fn open_line_above(&mut self) -> Result<(), String> {
        let row = if self.draft.is_empty() {
            0
        } else {
            self.cursor.row
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

    /// Delete from the cursor through the end of the current line.
    pub fn delete_to_line_end(&mut self) -> Result<(), String> {
        let length = self
            .draft
            .line(self.cursor.row)
            .unwrap_or("")
            .chars()
            .count();
        if self.cursor.column >= length {
            return Ok(());
        }
        let end = Position {
            row: self.cursor.row,
            column: length,
        };
        self.draft
            .replace_text(self.cursor, end, "")
            .map_err(str::to_owned)?;
        Ok(())
    }

    /// Join complete lines using historical ex whitespace rules.
    pub fn join_lines(&mut self, count: usize) -> Result<(), String> {
        let row = self.cursor.row;
        if row >= self.draft.line_count().saturating_sub(1) {
            return Err("no following line to join".into());
        }
        let end = row
            .saturating_add(count.max(2) - 1)
            .min(self.draft.line_count() - 1);
        let mut joined = self.draft.line(row).unwrap().to_owned();
        let column = joined.chars().count();
        for index in row + 1..=end {
            let next = self
                .draft
                .line(index)
                .unwrap()
                .trim_start_matches([' ', '\t']);
            if !next.is_empty()
                && !joined.is_empty()
                && !joined.ends_with([' ', '\t'])
                && !next.starts_with(')')
            {
                if joined.ends_with('.') {
                    joined.push(' ');
                }
                joined.push(' ');
            }
            joined.push_str(next);
        }
        let last_column = self.draft.line(end).unwrap().chars().count();
        self.draft
            .replace_text(
                Position { row, column: 0 },
                Position {
                    row: end,
                    column: last_column,
                },
                &joined,
            )
            .map_err(str::to_owned)?;
        self.cursor = Position {
            row,
            column: column.min(joined.chars().count().saturating_sub(1)),
        };
        Ok(())
    }

    pub fn commit(self) -> Result<Position, String> {
        let pending_register = self.pending_register;
        if self.draft != self.editor.buffer {
            let row = self.cursor.row;
            self.editor.edit(move |buffer| {
                *buffer = self.draft;
                Ok(if buffer.is_empty() { None } else { Some(row) })
            })?;
        }
        if let Some(register) = pending_register {
            self.editor.unnamed = Some(register);
        }
        Ok(self.cursor)
    }
}
