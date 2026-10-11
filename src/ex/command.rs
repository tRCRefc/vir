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
    if matches!(parsed.command, Command::Yank { register: Some(name) } if !name.is_ascii_alphabetic())
    {
        return Err("invalid named register".into());
    }
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
        Command::SetAutoIndent(_) => {
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
        Command::Append | Command::Insert | Command::Put { .. } | Command::Read => {
            insertion_index(
                parsed.range.clone(),
                editor.buffer().line_count(),
                editor.current(),
                matches!(
                    parsed.command,
                    Command::Append | Command::Put { .. } | Command::Read
                ),
            )
            .map_err(|e| format!("{e:?}"))?;
        }
        Command::Move | Command::Copy => {
            transfer_addresses(&parsed, editor)?;
        }
        Command::Substitute
        | Command::Join
        | Command::Print
        | Command::Number
        | Command::Delete
        | Command::Yank { .. }
        | Command::Change => {
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

// Both preflight and execution use the same source and destination policy.
fn transfer_addresses(
    parsed: &ParsedCommand,
    editor: &Editor,
) -> Result<(super::address::LineRange, usize), String> {
    let range = evaluate(
        parsed.range.clone(),
        editor.buffer().line_count(),
        editor.current(),
    )
    .map_err(|e| format!("{e:?}"))?;
    let destination = parsed
        .destination
        .clone()
        .ok_or("missing destination address")?;
    if destination == super::parser::Address::All {
        return Err("destination must be a single address".into());
    }
    let at = insertion_index(
        super::parser::Range {
            start: Some(destination),
            end: None,
        },
        editor.buffer().line_count(),
        editor.current(),
        true,
    )
    .map_err(|e| format!("{e:?}"))?;
    if parsed.command == Command::Move && (range.start..=range.end + 1).contains(&at) {
        return Err("move destination overlaps source or would do nothing".into());
    }
    Ok((range, at))
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
    if parsed.command == Command::Substitute {
        validate_addresses(&parsed, editor)?;
        let spec = parsed
            .substitution
            .as_ref()
            .ok_or_else(|| "missing substitution".to_owned())?;
        let range = evaluate(
            parsed.range.clone(),
            editor.buffer().line_count(),
            editor.current(),
        )
        .map_err(|e| format!("{e:?}"))?;
        super::substitute::execute(
            spec,
            range,
            pattern
                .as_deref()
                .or(editor.last_search())
                .map(str::to_owned),
            editor,
        )?;
        return Ok(ResultValue {
            output: String::new(),
            quit: false,
        });
    }
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
                destination: None,
                substitution: None,
            },
            editor,
        );
    }
    if let Command::SetAutoIndent(enabled) = parsed.command {
        editor.set_autoindent(enabled);
        return Ok(ResultValue {
            output: String::new(),
            quit: false,
        });
    }
    if matches!(parsed.command, Command::Move | Command::Copy) {
        let (range, at) = transfer_addresses(&parsed, editor)?;
        editor.transfer_lines(range.start, range.end, at, parsed.command == Command::Copy)?;
        return Ok(ResultValue {
            output: String::new(),
            quit: false,
        });
    }
    if parsed.command == Command::Read {
        let at = insertion_index(
            parsed.range.clone(),
            editor.buffer().line_count(),
            editor.current(),
            true,
        )
        .map_err(|e| format!("{e:?}"))?;
        let path = parsed
            .path
            .as_deref()
            .map(std::path::Path::new)
            .or(editor.path())
            .ok_or("no file name")?;
        // :read inserts only existing regular UTF-8/LF files. It must never
        // adopt a new file name, origin, or undo history as :edit does.
        if !std::fs::metadata(path)
            .map_err(|e| e.to_string())?
            .is_file()
        {
            return Err("read requires a regular file".into());
        }
        let (buffer, origin) = crate::file_io::load_with_origin(path).map_err(|e| e.to_string())?;
        if origin.is_none() {
            return Err("file no longer exists".into());
        }
        editor.insert_lines(
            at,
            (0..buffer.line_count()).map(|row| buffer.line(row).unwrap().to_owned()),
        )?;
        return Ok(ResultValue {
            output: String::new(),
            quit: false,
        });
    }
    if parsed.command == Command::Join {
        let mut range = evaluate(
            parsed.range.clone(),
            editor.buffer().line_count(),
            editor.current(),
        )
        .map_err(|e| format!("{e:?}"))?;
        if parsed.range.end.is_none() && parsed.range.start != Some(super::parser::Address::All) {
            range.end = (range.end + 1).min(editor.buffer().line_count() - 1);
        }
        if range.start == range.end {
            editor
                .begin_text_edit(crate::buffer::Position {
                    row: range.start,
                    column: 0,
                })?
                .commit_even_if_unchanged()?;
        } else {
            let mut edit = editor.begin_text_edit(crate::buffer::Position {
                row: range.start,
                column: 0,
            })?;
            edit.join_lines(range.end - range.start + 1)?;
            edit.commit()?;
        }
        return Ok(ResultValue {
            output: String::new(),
            quit: false,
        });
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
        Command::PrintNext
        | Command::Substitute
        | Command::Join
        | Command::Read
        | Command::Move
        | Command::Copy => unreachable!(),
        Command::Write | Command::WriteQuit | Command::Edit { .. } | Command::File => {
            unreachable!()
        }
        Command::Undo | Command::SetAutoIndent(_) => unreachable!(),
        Command::Print
        | Command::Number
        | Command::Delete
        | Command::Yank { .. }
        | Command::Put { .. }
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
            if let Command::Put { register: selected } = parsed.command {
                let register = match selected {
                    None => editor
                        .unnamed_register()
                        .ok_or("register is empty")?
                        .clone(),
                    Some(name @ 'a'..='z') | Some(name @ 'A'..='Z') => editor
                        .named_register(name)
                        .ok_or("register is empty")?
                        .clone(),
                    Some(name @ '1'..='9') => editor
                        .numbered_register((name as u8 - b'0') as usize)
                        .ok_or("register is empty")?
                        .clone(),
                    Some(_) => return Err("invalid register".into()),
                };
                let lines = match register {
                    crate::editor::Register::Lines(lines) => lines,
                    crate::editor::Register::Characters(text) => {
                        text.split('\n').map(str::to_owned).collect()
                    }
                };
                let at = insertion_index(
                    parsed.range.clone(),
                    editor.buffer().line_count(),
                    editor.current(),
                    true,
                )
                .map_err(|e| format!("{e:?}"))?;
                editor.insert_lines(at, lines)?;
                return Ok(ResultValue {
                    output: String::new(),
                    quit: false,
                });
            }
            let buffer = editor.buffer();
            let range = evaluate(parsed.range.clone(), buffer.line_count(), editor.current())
                .map_err(|e| format!("{e:?}"))?;
            if parsed.command == Command::Delete {
                let register =
                    crate::editor::Register::lines(editor.buffer(), range.start, range.end)?;
                editor
                    .delete_lines(range.start, range.end)
                    .map_err(|e| e.to_string())?;
                editor.push_numbered_register(register);
                return Ok(ResultValue {
                    output: String::new(),
                    quit: false,
                });
            }
            if let Command::Yank { register: _ } = parsed.command {
                let register =
                    crate::editor::Register::lines(editor.buffer(), range.start, range.end)?;
                match parsed.command {
                    Command::Yank { register: None } => editor.unnamed = Some(register),
                    Command::Yank {
                        register: Some(name @ 'a'..='z'),
                    }
                    | Command::Yank {
                        register: Some(name @ 'A'..='Z'),
                    } => editor.set_named_register(name, register, name.is_ascii_uppercase())?,
                    _ => return Err("invalid named register".into()),
                }
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
