use super::address::{evaluate, insertion_index};
use super::parser::{Command, ParsedCommand};
use crate::editor::Editor;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResultValue {
    pub output: String,
    pub quit: bool,
}

/// Validate addresses without changing editor state or consuming input.
/// Explicit write ranges must name existing lines.
pub fn validate_addresses(parsed: &ParsedCommand, editor: &Editor) -> Result<(), String> {
    let (parsed, _) = super::search::resolve(parsed.clone(), editor)?;
    let addressed = parsed.range.start.is_some() || parsed.range.end.is_some();
    match parsed.command {
        Command::Quit { .. }
        | Command::Next { .. }
        | Command::Args
        | Command::Undo
        | Command::PrintNext
        | Command::Edit { .. }
        | Command::File => {
            if addressed {
                return Err("command does not accept addresses".into());
            }
        }
        Command::Write | Command::WriteQuit => {
            if addressed {
                evaluate(
                    parsed.range.clone(),
                    editor.buffer().line_count(),
                    editor.current(),
                )
                .map_err(|e| format!("{e:?}"))?;
            }
        }
        Command::Append | Command::Insert => {
            insertion_index(
                parsed.range.clone(),
                editor.buffer().line_count(),
                editor.current(),
                parsed.command == Command::Append,
            )
            .map_err(|e| format!("{e:?}"))?;
        }
        Command::Print | Command::Number | Command::Delete | Command::Change => {
            evaluate(
                parsed.range.clone(),
                editor.buffer().line_count(),
                editor.current(),
            )
            .map_err(|e| format!("{e:?}"))?;
        }
    }
    Ok(())
}

pub fn execute(parsed: ParsedCommand, editor: &mut Editor) -> Result<ResultValue, String> {
    execute_with_lines(parsed, editor, &[])
}

pub fn execute_with_lines(
    parsed: ParsedCommand,
    editor: &mut Editor,
    input_lines: &[String],
) -> Result<ResultValue, String> {
    let (parsed, pattern) = super::search::resolve(parsed, editor)?;
    let result = execute_resolved(parsed, editor, input_lines)?;
    if let Some(pattern) = pattern {
        editor.remember_search(pattern);
    }
    Ok(result)
}

fn execute_resolved(
    parsed: ParsedCommand,
    editor: &mut Editor,
    input_lines: &[String],
) -> Result<ResultValue, String> {
    validate_addresses(&parsed, editor)?;
    if matches!(parsed.command, Command::Write | Command::WriteQuit) {
        let range = if parsed.range.start.is_some() || parsed.range.end.is_some() {
            Some(
                evaluate(
                    parsed.range.clone(),
                    editor.buffer().line_count(),
                    editor.current(),
                )
                .map_err(|e| format!("{e:?}"))?,
            )
        } else {
            None
        };
        editor
            .write_range_to(parsed.path.as_deref().map(std::path::Path::new), range)
            .map_err(|e| e.to_string())?;
        if parsed.command == Command::WriteQuit {
            editor.check_quit(false)?;
        }
        return Ok(ResultValue {
            output: String::new(),
            quit: parsed.command == Command::WriteQuit,
        });
    }
    if let Command::Edit { force } = parsed.command {
        let path = parsed
            .path
            .as_deref()
            .map(std::path::Path::new)
            .or(editor.path())
            .ok_or_else(|| "no file name".to_owned())?
            .to_owned();
        editor
            .load_with_force(&path, force)
            .map_err(|e| e.to_string())?;
        return Ok(ResultValue {
            output: String::new(),
            quit: false,
        });
    }
    if parsed.command == Command::File {
        if let Some(path) = &parsed.path {
            editor.set_path(std::path::Path::new(path));
        }
        let name = editor
            .path()
            .map(|p| format!("\"{}\"", p.display()))
            .unwrap_or_else(|| "No file".into());
        return Ok(ResultValue {
            output: format!(
                "{}{} line {} of {}\n",
                name,
                if editor.is_modified() {
                    " [Modified]"
                } else {
                    ""
                },
                editor.current().map_or(0, |n| n + 1),
                editor.buffer().line_count()
            ),
            quit: false,
        });
    }
    if parsed.command == Command::Undo {
        editor.undo().map_err(|e| e.to_string())?;
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
                path: None,
            },
            editor,
        );
    }
    match parsed.command {
        Command::Quit { force } => {
            editor.check_quit(force)?;
            Ok(ResultValue {
                output: String::new(),
                quit: true,
            })
        }
        Command::Next { force } => {
            editor.next_file(force).map_err(|e| e.to_string())?;
            Ok(ResultValue {
                output: String::new(),
                quit: false,
            })
        }
        Command::Args => {
            let entries: Vec<String> = editor
                .arguments()
                .iter()
                .enumerate()
                .map(|(index, path)| {
                    if editor.argument_index() == Some(index) {
                        format!("[{}]", path.display())
                    } else {
                        path.display().to_string()
                    }
                })
                .collect();
            Ok(ResultValue {
                output: format!("{}\n", entries.join(" ")),
                quit: false,
            })
        }
        Command::PrintNext => unreachable!(),
        Command::Write | Command::WriteQuit | Command::Edit { .. } | Command::File => {
            unreachable!()
        }
        Command::Undo => unreachable!(),
        Command::Print
        | Command::Number
        | Command::Delete
        | Command::Change
        | Command::Append
        | Command::Insert => {
            if matches!(parsed.command, Command::Append | Command::Insert) {
                let at = insertion_index(
                    parsed.range.clone(),
                    editor.buffer().line_count(),
                    editor.current(),
                    parsed.command == Command::Append,
                )
                .map_err(|e| format!("{e:?}"))?;
                editor
                    .insert_lines(at, input_lines.iter().cloned())
                    .map_err(|e| e.to_string())?;
                return Ok(ResultValue {
                    output: String::new(),
                    quit: false,
                });
            }
            let buffer = editor.buffer();
            let range = evaluate(parsed.range.clone(), buffer.line_count(), editor.current())
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
                editor
                    .replace_lines(range.start, range.end, input_lines.iter().cloned())
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
