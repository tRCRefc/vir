use super::address::evaluate;
use super::parser::{Command, ParsedCommand};
use crate::editor::Editor;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResultValue {
    pub output: String,
    pub quit: bool,
}

pub fn execute(parsed: ParsedCommand, editor: &mut Editor) -> Result<ResultValue, String> {
    execute_with_lines(parsed, editor, &[])
}

pub fn execute_with_lines(
    parsed: ParsedCommand,
    editor: &mut Editor,
    input_lines: &[String],
) -> Result<ResultValue, String> {
    if matches!(parsed.command, Command::Quit { .. } | Command::Next)
        && (parsed.range.start.is_some() || parsed.range.end.is_some())
    {
        return Err("command does not accept addresses".into());
    }
    if parsed.command == Command::Write {
        editor.save().map_err(|e| e.to_string())?;
        return Ok(ResultValue {
            output: String::new(),
            quit: false,
        });
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
        Command::Write => unreachable!(),
        Command::Print
        | Command::Number
        | Command::Delete
        | Command::Change
        | Command::Append
        | Command::Insert => {
            let buffer = editor.buffer();
            let range = evaluate(parsed.range, buffer.line_count(), editor.current())
                .map_err(|e| format!("{e:?}"))?;
            if parsed.command == Command::Delete {
                editor
                    .delete_lines(range.start, range.end)
                    .map_err(|e| e.to_string())?;
                return Ok(ResultValue {
                    output: String::new(),
                    quit: false,
                });
            }
            if parsed.command == Command::Change {
                if input_lines.is_empty() {
                    return Err("change requires input".into());
                }
                editor
                    .replace_lines(range.start, range.end, input_lines.iter().cloned())
                    .map_err(|e| e.to_string())?;
                return Ok(ResultValue {
                    output: String::new(),
                    quit: false,
                });
            }
            if matches!(parsed.command, Command::Append | Command::Insert) {
                if input_lines.is_empty() {
                    return Err("append/insert requires input".into());
                }
                let at = if parsed.command == Command::Append {
                    range.end + 1
                } else {
                    range.start
                };
                editor
                    .insert_lines(at, input_lines.iter().cloned())
                    .map_err(|e| e.to_string())?;
                return Ok(ResultValue {
                    output: String::new(),
                    quit: false,
                });
            }
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
