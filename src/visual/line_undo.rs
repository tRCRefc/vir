//! Frontend checkpoints for U, separate from Editor's command-level undo.
use super::VisualState;
use crate::{
    buffer::{Position, TextBuffer},
    editor::Editor,
};
use std::path::PathBuf;

#[derive(Debug, Default, Clone)]
pub(super) struct LineUndo {
    observed: Option<TextBuffer>,
    path: Option<PathBuf>,
    row: usize,
    original: Option<String>,
    consumed: bool,
    generation: u64,
}

impl LineUndo {
    fn observe(&mut self, editor: &Editor, row: usize) {
        let buffer = editor.buffer();
        let same_line = self.observed.as_ref().is_some_and(|before| {
            self.row == row
                && self.path.as_deref() == editor.path()
                && before.line_count() == buffer.line_count()
                && before.has_trailing_newline() == buffer.has_trailing_newline()
                && (0..buffer.line_count())
                    .all(|index| index == row || before.line(index) == buffer.line(index))
        });
        if !same_line {
            self.original = buffer.line(row).map(str::to_owned);
            self.consumed = false;
        } else if self.generation != editor.edit_generation() {
            self.consumed = false;
        }
        if self.observed.as_ref() != Some(buffer) {
            self.observed = Some(buffer.clone());
        }
        self.path = editor.path().map(PathBuf::from);
        self.row = row;
        self.generation = editor.edit_generation();
    }
}

impl VisualState {
    /// Frontends call at normal-command boundaries, before accepting input.
    /// Embedded callers must checkpoint before the first edit and after movement.
    pub fn checkpoint_line(&mut self, editor: &Editor) {
        self.line_undo.observe(editor, self.row);
    }

    /// Invalidate after a buffer reload or structural change outside visual APIs.
    /// The terminal frontend handles these boundaries automatically.
    pub fn forget_line(&mut self) {
        self.line_undo = LineUndo::default();
    }

    /// Historical vsave converts VCAPU before entering ex/search, including a
    /// cancelled prompt. The visit snapshot and ordinary undo remain available.
    pub(super) fn sync_line_undo(&mut self) {
        self.line_undo.consumed = false;
    }

    /// Restore this visit's initial line as one ordinary reversible transaction.
    /// U does not replace the last logical edit used by dot.
    pub fn restore_line(&mut self, editor: &mut Editor) -> Result<(), String> {
        self.checkpoint_line(editor);
        let original = self
            .line_undo
            .original
            .as_ref()
            .ok_or("no line to restore")?;
        let current = editor.buffer().line(self.row).ok_or("no line to restore")?;
        if self.line_undo.consumed {
            return Err("no line changes to restore".into());
        }
        let end = Position {
            row: self.row,
            column: current.chars().count(),
        };
        let mut edit = editor.begin_text_edit(Position {
            row: self.row,
            column: 0,
        })?;
        edit.delete_range(
            Position {
                row: self.row,
                column: 0,
            },
            end,
        )?;
        edit.insert(original)?;
        edit.commit_even_if_unchanged()?;
        self.column = 0;
        self.preferred_column = 0;
        self.left = 0;
        self.checkpoint_line(editor);
        self.line_undo.consumed = true;
        Ok(())
    }
}
