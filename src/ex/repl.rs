//! Injectable ex command loop for scripts and terminals.

use std::io::{self, BufRead, Write};

use crate::{editor::Editor, recovery::Recovery};

/// Run a script until q or EOF, including EOF with unsaved changes.
/// Command errors are printed and the loop continues. Input errors abort the
/// current edit. Like original gettty, input EOF commits complete lines only.
pub fn run<R: BufRead, W: Write>(
    input: &mut R,
    output: &mut W,
    editor: &mut Editor,
) -> io::Result<()> {
    run_with_prompt(input, output, editor, false)
}

/// Run the loop, optionally writing the traditional `:` prompt before reads.
pub fn run_with_prompt<R: BufRead, W: Write>(
    input: &mut R,
    output: &mut W,
    editor: &mut Editor,
    prompt: bool,
) -> io::Result<()> {
    run_with_recovery(input, output, editor, prompt, &mut Recovery::disabled())
}

/// CLI session with explicit recovery ownership. Errors retain the last checkpoint.
pub fn run_with_recovery<R: BufRead, W: Write>(
    input: &mut R,
    output: &mut W,
    editor: &mut Editor,
    prompt: bool,
    recovery: &mut Recovery,
) -> io::Result<()> {
    checkpoint(recovery, editor, output)?;
    let mut line = String::new();
    loop {
        if prompt {
            write!(output, ":")?;
            output.flush()?;
        }
        line.clear();
        if input.read_line(&mut line)? == 0 {
            return Ok(());
        }
        let parsed = super::parser::parse(&line)
            .map_err(|error| error.to_string())
            .and_then(|parsed| {
                super::command::validate_addresses(&parsed, editor)?;
                Ok(parsed)
            });
        let result = match parsed {
            Ok(
                parsed @ super::parser::ParsedCommand {
                    command:
                        super::parser::Command::Change
                        | super::parser::Command::Append
                        | super::parser::Command::Insert,
                    ..
                },
            ) => {
                let mut lines = Vec::new();
                loop {
                    line.clear();
                    if input.read_line(&mut line)? == 0 {
                        break;
                    }
                    // gettty discards a partial last line at EOF. Accept CRLF
                    // as a transport delimiter without stripping content CRs.
                    let Some(value) = line.strip_suffix('\n') else {
                        break;
                    };
                    let value = value.strip_suffix('\r').unwrap_or(value);
                    if value == "." {
                        break;
                    }
                    lines.push(value.to_owned());
                    if recovery.is_enabled() {
                        let mut draft = editor.clone();
                        if super::command::execute_with_lines(parsed.clone(), &mut draft, &lines)
                            .is_ok()
                        {
                            checkpoint(recovery, &draft, output)?;
                        }
                    }
                }
                super::command::execute_with_lines(parsed, editor, &lines)
            }
            Ok(parsed) => super::command::execute(parsed, editor),
            Err(error) => Err(error),
        };
        checkpoint(recovery, editor, output)?;
        match result {
            Ok(result) => {
                output.write_all(result.output.as_bytes())?;
                output.flush()?;
                if result.quit {
                    return Ok(());
                }
            }
            Err(error) => {
                writeln!(output, "? {error}")?;
                output.flush()?;
            }
        }
    }
}

fn checkpoint<W: Write>(
    recovery: &mut Recovery,
    editor: &Editor,
    output: &mut W,
) -> io::Result<()> {
    if let Err(error) = recovery.observe(editor) {
        writeln!(output, "? recovery unavailable: {error}")?;
        output.flush()?;
    }
    Ok(())
}
