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
    if parsed.command == Command::PrintNext {
        let next = editor
            .current()
            .and_then(|n| n.checked_add(1))
            .ok_or_else(|| "at end of file".to_string())?;
        if next >= editor.buffer().line_count() {
            return Err("at end of file".into());
        }
        return execute(
            ParsedCommand {
                range: super::parser::Range {
                    start: Some(super::parser::Address::Line(next + 1)),
                    end: None,
                },
                command: Command::Print,
            },
            editor,
        );
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
        Command::PrintNext => unreachable!(),
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
