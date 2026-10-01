use super::VisualState;
use crate::{
    editor::Editor,
    ex::{
        command::{ResultValue, execute, validate_addresses},
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
                | Command::Yank { .. }
                | Command::Put { .. }
                | Command::Undo
                | Command::Substitute
                | Command::SetAutoIndent(_)
        ) {
            return Err(
                "visual supports only :w, :q, :q!, :wq, :e[!], :f, :d, :yank, :put, :u, :s and :set ai/noai"
                    .into(),
            );
        }
        // ex_cmds counts substitute before its execution, including a
        // successful same-text replacement. Delete first resolves setCNL.
        if command == Command::Substitute {
            editor.enter_macro_command();
        } else if matches!(
            command,
            Command::Delete | Command::Yank { .. } | Command::Put { .. }
        ) {
            validate_addresses(&parsed, editor)?;
            editor.enter_macro_command();
        }
        let result = execute(parsed, editor);
        editor.finish_macro_command();
        let mut result = result?;
        if matches!(
            command,
            Command::Edit { .. } | Command::Delete | Command::Substitute | Command::Put { .. }
        ) {
            self.forget_line();
        }
        if matches!(
            command,
            Command::Edit { .. }
                | Command::Delete
                | Command::Undo
                | Command::Substitute
                | Command::Yank { .. }
                | Command::Put { .. }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffer::TextBuffer;

    #[test]
    fn unchanged_substitute_promotes_before_nested_macro() {
        let mut editor = Editor::from_buffer(TextBuffer::from_text("abcdef").unwrap());
        let mut state = VisualState::default();
        editor.begin_macro_undo();
        state.delete_character(&mut editor).unwrap();
        state.execute_ex("s/b/b/", &mut editor).unwrap();
        editor.begin_macro_undo();
        state.delete_character(&mut editor).unwrap();
        editor.end_macro_undo();
        state.undo(&mut editor).unwrap();
        assert_eq!(editor.buffer().to_text(), "abcdef");
    }
}
