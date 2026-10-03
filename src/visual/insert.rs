use super::{VisualState, repeat::ChangeRecord};
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
        self.begin_insert_register(editor, mode, None)
    }

    pub fn begin_insert_register<'a>(
        &self,
        editor: &'a mut Editor,
        mode: char,
        selected: Option<char>,
    ) -> Result<TextEdit<'a>, String> {
        if !matches!(mode, 'i' | 'a' | 'o' | 'I' | 'A' | 'O' | 'C' | 'R') {
            return Err("unsupported insert mode".into());
        }
        if matches!(selected, Some(name) if !name.is_ascii_alphabetic()) {
            return Err("invalid register".into());
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
                if selected.is_some() {
                    edit.stage_numbered_register(register.clone());
                }
                match selected {
                    None => edit.stage_register(register),
                    Some(name @ 'a'..='z') | Some(name @ 'A'..='Z') => {
                        edit.stage_named_register(name, register, name.is_ascii_uppercase())
                    }
                    Some(_) => return Err("invalid register".into()),
                }
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

    /// X deletes only the characters before this cursor, never a preceding LF.
    pub fn delete_before(
        &mut self,
        editor: &mut Editor,
        count: usize,
        register: Option<char>,
    ) -> Result<(), String> {
        if self.column == 0 {
            return Err("no character before cursor".into());
        }
        let mut next = self.clone();
        let deleted = count.max(1).min(self.column);
        next.column -= deleted;
        next.delete_characters(editor, Some(deleted), register)?;
        next.last_change = Some(ChangeRecord::DeleteBefore {
            count: count.max(1),
            register,
        });
        *self = next;
        Ok(())
    }

    pub fn delete_character(&mut self, editor: &mut Editor) -> Result<(), String> {
        self.delete_characters(editor, Some(1), None)
    }

    /// x consumes a character count; D always stops at this line's end.
    pub fn delete_characters(
        &mut self,
        editor: &mut Editor,
        count: Option<usize>,
        selected: Option<char>,
    ) -> Result<(), String> {
        let length = editor.buffer().line(self.row).unwrap_or("").chars().count();
        if self.column >= length {
            return Err("no character to delete".into());
        }
        let end = self
            .column
            .saturating_add(count.unwrap_or(usize::MAX).max(1))
            .min(length);
        let register = Register::characters(
            editor.buffer(),
            Position {
                row: self.row,
                column: self.column,
            },
            Position {
                row: self.row,
                column: end,
            },
        )?;
        super::operator::validate_register_target(editor, selected, &register)?;
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
        if let Some(name) = selected {
            edit.stage_named_register(name, register, name.is_ascii_uppercase());
        } else {
            edit.stage_register(register);
        }
        edit.commit()?;
        self.column = self
            .column
            .min((length - (end - self.column)).saturating_sub(1));
        self.preferred_column = self.column;
        self.last_change = Some(ChangeRecord::DeleteCharacters {
            count,
            register: selected,
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
        let mut edit = editor.begin_text_edit(Position {
            row: self.row,
            column: self.column,
        })?;
        if replacement == '\n' {
            edit.record_line_replacement(self.row, self.row);
        }
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
            edit.insert_input("\n")?;
        } else {
            edit.insert(&replacement.to_string().repeat(count))?;
        }
        let position = edit.commit_even_if_unchanged()?;
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
