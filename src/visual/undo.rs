//! Visual placement after shared undo; ex retains its own line-address semantics.
use super::VisualState;
use crate::editor::Editor;

impl VisualState {
    pub fn undo(&mut self, editor: &mut Editor) -> Result<(), String> {
        let anchor = editor.undo_position();
        editor.undo()?;
        self.row = anchor
            .map_or_else(|| editor.current().unwrap_or(0), |p| p.row)
            .min(editor.buffer().line_count().saturating_sub(1));
        let length = editor.buffer().line(self.row).unwrap_or("").chars().count();
        self.column = anchor.map_or(0, |p| p.column).min(length.saturating_sub(1));
        self.preferred_column = self.column;
        self.left = 0;
        self.top = self.top.min(self.row);
        if !editor.buffer().is_empty() {
            editor.set_current(self.row).map_err(|e| format!("{e:?}"))?;
        }
        Ok(())
    }
}
