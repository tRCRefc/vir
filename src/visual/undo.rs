//! Visual placement after shared undo; ex retains its own line-address semantics.
use super::VisualState;
use crate::editor::{Editor, UndoLocation};

impl VisualState {
    pub fn undo(&mut self, editor: &mut Editor) -> Result<(), String> {
        let anchor = editor.undo_location();
        editor.undo()?;
        let row = match anchor {
            Some(UndoLocation::Character(p) | UndoLocation::InitialCharacter(p)) => p.row,
            Some(UndoLocation::Lines { next, .. }) => next,
            None => editor.current().unwrap_or(0),
        };
        self.row = row.min(editor.buffer().line_count().saturating_sub(1));
        let line = editor.buffer().line(self.row).unwrap_or("");
        let length = line.chars().count();
        self.column = match anchor {
            Some(UndoLocation::Character(p) | UndoLocation::InitialCharacter(p)) => p.column,
            Some(UndoLocation::Lines { .. }) => line
                .chars()
                .position(|c| c != ' ' && c != '\t')
                .unwrap_or(length),
            None => 0,
        }
        .min(length.saturating_sub(1));
        self.preferred_column = self.column;
        self.left = 0;
        self.top = self.top.min(self.row);
        if !editor.buffer().is_empty() {
            editor.set_current(self.row).map_err(|e| format!("{e:?}"))?;
        }
        Ok(())
    }
}
