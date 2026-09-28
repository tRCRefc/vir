//! Single-character motions used by normal mode and operators.
use super::{Motion, VisualState};
use crate::{
    buffer::{Position, TextBuffer},
    editor::Editor,
};

/// Resolve one `f/F/t/T` motion without changing editor or visual state.
pub(super) fn target(
    buffer: &TextBuffer,
    origin: Position,
    forward: bool,
    till: bool,
    character: char,
    count: usize,
) -> Option<Position> {
    let line = buffer.line(origin.row)?;
    let length = line.chars().count();
    if origin.column >= length {
        return None;
    }
    let mut column = origin.column;
    for _ in 0..count.max(1) {
        let found = if forward {
            (column.saturating_add(1)..length)
                .find(|&candidate| line.chars().nth(candidate) == Some(character))
        } else {
            (0..column)
                .rev()
                .find(|&candidate| line.chars().nth(candidate) == Some(character))
        }?;
        column = found;
    }
    if till {
        // `t` stops before the target, while `T` stops after it. The
        // adjustment happens after all counted searches, as in vi.
        column = if forward {
            column.saturating_sub(1)
        } else {
            column.saturating_add(1).min(length.saturating_sub(1))
        };
    }
    Some(Position {
        row: origin.row,
        column,
    })
}

impl VisualState {
    /// Move to a character on the current line. Failed searches preserve the
    /// cursor and the previous repeat recipe.
    pub fn find_character(
        &mut self,
        editor: &Editor,
        forward: bool,
        till: bool,
        character: char,
        count: usize,
    ) -> Result<(), String> {
        let origin = Position {
            row: self.row,
            column: self.column,
        };
        let target = target(editor.buffer(), origin, forward, till, character, count)
            .ok_or("character not found")?;
        self.row = target.row;
        self.column = target.column;
        self.preferred_column = self.column;
        self.last_find = Some((forward, till, character));
        Ok(())
    }

    /// Repeat the last character search; `,` reverses its direction while
    /// retaining whether the original command was `f`/`t` or `F`/`T`.
    pub fn repeat_find(
        &mut self,
        editor: &Editor,
        reverse: bool,
        count: usize,
    ) -> Result<(), String> {
        let (forward, till, character) = self.last_find.ok_or("no previous character search")?;
        let origin = Position {
            row: self.row,
            column: self.column,
        };
        let target = target(
            editor.buffer(),
            origin,
            forward ^ reverse,
            till,
            character,
            count,
        )
        .ok_or("character not found")?;
        self.row = target.row;
        self.column = target.column;
        self.preferred_column = self.column;
        Ok(())
    }

    /// Resolve a character motion for an operator without moving the cursor.
    pub(super) fn prepare_find_motion(
        &mut self,
        editor: &Editor,
        forward: bool,
        till: bool,
        character: char,
        count: usize,
    ) -> Result<Motion, String> {
        let origin = Position {
            row: self.row,
            column: self.column,
        };
        target(editor.buffer(), origin, forward, till, character, count)
            .ok_or("character not found")?;
        self.last_find = Some((forward, till, character));
        Ok(Motion::Find {
            forward,
            till,
            character,
        })
    }

    pub(super) fn prepare_repeat_find_motion(
        &mut self,
        editor: &Editor,
        reverse: bool,
        count: usize,
    ) -> Result<Motion, String> {
        let (forward, till, character) = self.last_find.ok_or("no previous character search")?;
        let effective_forward = forward ^ reverse;
        target(
            editor.buffer(),
            Position {
                row: self.row,
                column: self.column,
            },
            effective_forward,
            till,
            character,
            count,
        )
        .ok_or("character not found")?;
        Ok(Motion::Find {
            forward: effective_forward,
            till,
            character,
        })
    }
}
