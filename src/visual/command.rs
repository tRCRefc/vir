use super::VisualState;
use crate::{
    editor::Editor,
    ex::{
        command::{ResultValue, execute},
        parser::{Command, parse},
    },
};

impl VisualState {
    /// Execute a supported ex command, then adopt its cursor before the next redraw.
    /// The caller must synchronize Editor.current with the visual row before entry.
    pub fn execute_ex(&mut self, text: &str, editor: &mut Editor) -> Result<ResultValue, String> {
        self.sync_line_undo();
        let parsed = parse(text).map_err(|e| e.to_string())?;
        let command = parsed.command;
        if !matches!(
            command,
            Command::Write
                | Command::WriteQuit
                | Command::Quit { .. }
                | Command::Edit { .. }
                | Command::File
                | Command::Delete
                | Command::Undo
                | Command::Substitute
        ) {
            return Err("visual supports only :w, :q, :q!, :wq, :e[!], :f, :d, :u and :s".into());
        }
        let mut result = execute(parsed, editor)?;
        if matches!(
            command,
            Command::Edit { .. } | Command::Delete | Command::Substitute
        ) {
            self.forget_line();
        }
        if matches!(
            command,
            Command::Edit { .. } | Command::Delete | Command::Undo | Command::Substitute
        ) {
            self.row = editor.current().unwrap_or(0);
            self.column = 0;
            self.preferred_column = 0;
            self.left = 0;
            self.top = self.top.min(self.row);
            if matches!(command, Command::Edit { .. }) {
                self.top = 0;
            }
        }
        if matches!(command, Command::Write | Command::WriteQuit) && !result.quit {
            result.output = "Written".into();
        }
        Ok(result)
    }
}
