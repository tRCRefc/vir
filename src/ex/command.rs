use super::address::evaluate;
use super::parser::{Command, ParsedCommand};
use crate::editor::Editor;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResultValue {
    pub output: String,
    pub quit: bool,
}

pub fn execute(parsed: ParsedCommand, editor: &mut Editor) -> Result<ResultValue, String> {
    if matches!(parsed.command, Command::Quit { .. } | Command::Next)
        && (parsed.range.start.is_some() || parsed.range.end.is_some())
    {
        return Err("command does not accept addresses".into());
    }
    match parsed.command {
        Command::Quit { force } => {
            if editor.is_modified() && !force {
                return Err("unsaved changes".into());
            }
            Ok(ResultValue {
                output: String::new(),
                quit: true,
            })
        }
        Command::Next => Err("next is not implemented".into()),
        Command::Print | Command::Number => {
            let buffer = editor.buffer();
            let range = evaluate(parsed.range, buffer.line_count(), editor.current())
                .map_err(|e| format!("{e:?}"))?;
            let mut output = String::new();
            for index in range.start..=range.end {
                if let Some(line) = buffer.line(index) {
                    if matches!(parsed.command, Command::Number) {
                        output.push_str(&format!("{:6}  ", index + 1));
                    }
                    output.push_str(line);
                    output.push('\n');
                }
            }
            editor
                .set_current(range.end)
                .map_err(|e| format!("{e:?}"))?;
            Ok(ResultValue {
                output,
                quit: false,
            })
        }
    }
}
