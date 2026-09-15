//! Injectable ex command loop for scripts and terminals.

use std::io::{self, BufRead, Write};

use crate::editor::Editor;

/// Run commands until q or clean EOF. Command errors are printed and the loop continues.
pub fn run<R: BufRead, W: Write>(
    input: &mut R,
    output: &mut W,
    editor: &mut Editor,
) -> io::Result<()> {
    let mut line = String::new();
    loop {
        line.clear();
        if input.read_line(&mut line)? == 0 {
            return Ok(());
        }
        let result = match super::parser::parse(&line) {
            Ok(parsed) => super::command::execute(parsed, editor),
            Err(error) => Err(error.to_string()),
        };
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
