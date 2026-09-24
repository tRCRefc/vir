//! A staged character-edit session. Dropping it cancels; commit creates one undo.
use super::Editor;
use crate::buffer::{Position, TextBuffer};

/// Logical text input, independent of terminal key encoding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputAction {
    Insert(String),
    Backspace,
    Delete,
    Replace(char),
    ReplaceText(String),
    RestoreReplacement,
}

pub struct TextEdit<'a> {
    editor: &'a mut Editor,
    draft: TextBuffer,
    cursor: Position,
    undo_anchor: Position,
    crossed_lines: bool,
    opened_at: Option<usize>,
    input_row: Option<usize>,
    replaced_lines: Option<(usize, usize)>,
    initial_undo_column: Option<usize>,
    pending_register: Option<super::Register>,
    replacements: Vec<(Position, Position, String, bool)>,
    input_actions: Option<Vec<InputAction>>,
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
            undo_anchor: cursor,
            crossed_lines: false,
            opened_at: None,
            input_row: None,
            replaced_lines: None,
            initial_undo_column: None,
            replacements: Vec::new(),
            pending_register: None,
            input_actions: None,
        })
    }
}

impl TextEdit<'_> {
    pub(crate) fn original(&self) -> &Editor {
        self.editor
    }

    pub fn has_changes(&self) -> bool {
        self.draft != self.editor.buffer
    }

    pub(crate) fn record_input(&mut self) {
        // Only single-original-line input has the VCHNG -> VMANYINS rule.
        self.input_row =
            (!self.crossed_lines && !self.editor.buffer.is_empty()).then_some(self.undo_anchor.row);
        self.input_actions = Some(Vec::new());
    }

    pub(crate) fn take_input_actions(&mut self) -> Vec<InputAction> {
        self.input_actions.take().unwrap_or_default()
    }

    fn record(&mut self, action: InputAction) {
        if let Some(actions) = &mut self.input_actions {
            if let (Some(InputAction::Insert(previous)), InputAction::Insert(text)) =
                (actions.last_mut(), &action)
            {
                previous.push_str(text);
            } else {
                actions.push(action);
            }
        }
    }

    pub fn apply_input(&mut self, action: &InputAction) -> Result<(), String> {
        match action {
            InputAction::Insert(text) => self.insert(text),
            InputAction::Backspace => self.backspace(),
            InputAction::Delete => self.delete(),
            InputAction::Replace(c) => self.replace_input(*c),
            InputAction::ReplaceText(text) => self.replace_input_text(text),
            InputAction::RestoreReplacement => self.replace_backspace(),
        }
    }

    /// Type one scalar in Replace mode; LF splits rather than overwrites.
    pub fn replace_input(&mut self, character: char) -> Result<(), String> {
        if character.is_control() && character != '\n' && character != '\t' {
            return Err("unsupported replacement character".into());
        }
        self.apply_replace_text(&character.to_string(), true)?;
        self.record(InputAction::Replace(character));
        Ok(())
    }

    /// Replace literal text in one buffer operation, retaining per-scalar R
    /// backspace history. LF splits the current line; it never consumes a file LF.
    pub fn replace_input_text(&mut self, text: &str) -> Result<(), String> {
        self.write_replace_text(text, true)
    }

    /// Insert generated R copies while retaining reversible per-scalar history.
    pub(crate) fn insert_replace_copies(&mut self, text: &str) -> Result<(), String> {
        self.write_replace_text(text, false)
    }

    fn write_replace_text(&mut self, text: &str, overwrite: bool) -> Result<(), String> {
        self.apply_replace_text(text, overwrite)?;
        if !text.is_empty() && self.input_actions.is_some() {
            self.record(InputAction::ReplaceText(text.to_owned()));
        }
        Ok(())
    }

    fn apply_replace_text(&mut self, text: &str, overwrite: bool) -> Result<(), String> {
        if text.contains('\r') {
            return Err("carriage returns are not supported".into());
        }
        if text.is_empty() {
            return Ok(());
        }
        let start = self.cursor;
        let mut cursor = start;
        let mut old = self
            .draft
            .line(start.row)
            .unwrap_or("")
            .chars()
            .skip(start.column);
        let mut overwritten = 0;
        let mut history = Vec::new();
        for (index, character) in text.chars().enumerate() {
            let previous = if character == '\n' || !overwrite {
                None
            } else {
                old.next()
            };
            overwritten += usize::from(previous.is_some());
            let next = if character == '\n' {
                Position {
                    row: cursor.row + 1,
                    column: 0,
                }
            } else {
                Position {
                    column: cursor.column + 1,
                    ..cursor
                }
            };
            history.push((
                cursor,
                next,
                previous.map_or_else(String::new, |c| c.to_string()),
                self.draft.is_empty() && index == 0,
            ));
            cursor = next;
        }
        self.cursor = self
            .draft
            .replace_text(
                start,
                Position {
                    column: start.column + overwritten,
                    ..start
                },
                text,
            )
            .map_err(str::to_owned)?;
        self.crossed_lines |= text.contains('\n');
        self.replacements.extend(history);
        Ok(())
    }

    pub(crate) fn can_restore_replacement(&self) -> bool {
        !self.replacements.is_empty()
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
            self.record(InputAction::RestoreReplacement);
        }
        Ok(())
    }

    pub(crate) fn stage_register(&mut self, register: super::Register) {
        self.pending_register = Some(register);
    }

    /// The original line range replaced by a command, before any draft changes.
    pub(crate) fn record_line_replacement(&mut self, start: usize, end: usize) {
        if start <= end && end < self.editor.buffer.line_count() && self.draft == self.editor.buffer
        {
            self.replaced_lines = Some((start, end));
            self.initial_undo_column = None;
        }
    }

    pub(crate) fn record_character_replacement(&mut self, start: Position, end: Position) {
        if start.row < end.row && self.draft == self.editor.buffer {
            self.record_line_replacement(start.row, end.row);
            self.initial_undo_column = Some(start.column);
        }
    }

    pub fn buffer(&self) -> &TextBuffer {
        &self.draft
    }

    pub fn delete_range(&mut self, start: Position, end: Position) -> Result<(), String> {
        self.cursor = self
            .draft
            .replace_text(start, end, "")
            .map_err(str::to_owned)?;
        self.undo_anchor = self.undo_anchor.min(start);
        self.crossed_lines |= start.row != end.row;
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
        let pristine = self.draft == self.editor.buffer;
        self.draft.insert_lines(row, [""]).map_err(str::to_owned)?;
        if pristine {
            self.opened_at = Some(row);
        }
        self.crossed_lines = true;
        self.cursor = Position { row, column: 0 };
        Ok(())
    }

    pub fn open_line_above(&mut self) -> Result<(), String> {
        let row = if self.draft.is_empty() {
            0
        } else {
            self.cursor.row
        };
        let pristine = self.draft == self.editor.buffer;
        self.draft.insert_lines(row, [""]).map_err(str::to_owned)?;
        if pristine {
            self.opened_at = Some(row);
        }
        self.crossed_lines = true;
        self.cursor = Position { row, column: 0 };
        Ok(())
    }

    pub fn insert(&mut self, text: &str) -> Result<(), String> {
        self.cursor = self
            .draft
            .replace_text(self.cursor, self.cursor, text)
            .map_err(str::to_owned)?;
        self.crossed_lines |= text.contains('\n');
        if !text.is_empty() && self.input_actions.is_some() {
            self.record(InputAction::Insert(text.to_owned()));
        }
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
        self.crossed_lines |= start.row != self.undo_anchor.row;
        self.undo_anchor = self.undo_anchor.min(start);
        self.record(InputAction::Backspace);
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
        self.crossed_lines |= self.cursor.row != end.row;
        self.record(InputAction::Delete);
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
        self.record_line_replacement(row, end);
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
        self.crossed_lines = true;
        self.cursor = Position {
            row,
            column: column.min(joined.chars().count().saturating_sub(1)),
        };
        Ok(())
    }

    /// Determine visual undo placement without publishing the draft.
    fn undo_location(&self) -> Option<super::UndoLocation> {
        let original = &self.editor.buffer;
        let single_line = !self.crossed_lines
            && self.draft.line_count() == original.line_count()
            && self.draft.has_trailing_newline() == original.has_trailing_newline()
            && (0..original.line_count())
                .all(|r| r == self.undo_anchor.row || self.draft.line(r) == original.line(r));
        let anchor = single_line.then_some(super::UndoLocation::Character(self.undo_anchor));
        // Open-line input may include cross-boundary erase/delete extensions.
        // Only publish insertion semantics when both original sides survive.
        let opened = self.opened_at.and_then(|at| {
            let added = self.draft.line_count().checked_sub(original.line_count())?;
            (added > 0
                && at <= original.line_count()
                && (0..at).all(|r| original.line(r) == self.draft.line(r))
                && (at..original.line_count())
                    .all(|r| original.line(r) == self.draft.line(r + added)))
            .then_some(super::UndoLocation::Lines {
                next: at.saturating_sub(1),
                opposite: at,
            })
        });
        // The first LF saves the original line. Both undo directions
        // restore a nonempty block, so both target its first line.
        let expanded = self.input_row.and_then(|at| {
            let added = self.draft.line_count().checked_sub(original.line_count())?;
            (added > 0
                && at < original.line_count()
                && self.draft.has_trailing_newline() == original.has_trailing_newline()
                && (0..at).all(|r| original.line(r) == self.draft.line(r))
                && (at + 1..original.line_count())
                    .all(|r| original.line(r) == self.draft.line(r + added)))
            .then_some(super::UndoLocation::Lines {
                next: at,
                opposite: at,
            })
        });
        let replaced = self.replaced_lines.and_then(|(start, end)| {
            let suffix = original.line_count().checked_sub(end + 1)?;
            let after = self.draft.line_count().checked_sub(suffix)?;
            (after > start
                && self.draft.has_trailing_newline() == original.has_trailing_newline()
                && (0..start).all(|r| original.line(r) == self.draft.line(r))
                && (0..suffix).all(|r| original.line(end + 1 + r) == self.draft.line(after + r)))
            .then_some(match self.initial_undo_column {
                Some(column) => {
                    super::UndoLocation::InitialCharacter(Position { row: start, column })
                }
                None => super::UndoLocation::Lines {
                    next: start,
                    opposite: start,
                },
            })
        });
        replaced.or(opened).or(expanded).or(anchor)
    }

    pub fn commit(self) -> Result<Position, String> {
        self.commit_inner(false)
    }

    /// Publish a successful visual command whose semantics replace unchanged undo.
    pub(crate) fn commit_even_if_unchanged(self) -> Result<Position, String> {
        self.commit_inner(true)
    }

    fn commit_inner(self, publish_unchanged: bool) -> Result<Position, String> {
        if publish_unchanged || self.draft != self.editor.buffer {
            let row = self.cursor.row;
            let location = self.undo_location();
            self.editor.edit(move |buffer| {
                *buffer = self.draft;
                Ok(if buffer.is_empty() { None } else { Some(row) })
            })?;
            self.editor.undo_location = location;
        }
        if let Some(register) = self.pending_register {
            self.editor.unnamed = Some(register);
        }
        Ok(self.cursor)
    }
}
