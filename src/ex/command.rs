use super::address::evaluate;
use super::parser::{Command, ParsedCommand};
use crate::buffer::TextBuffer;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResultValue {
    pub output: String,
    pub quit: bool,
}

pub fn execute(
    parsed: ParsedCommand,
    buffer: &TextBuffer,
    current: Option<usize>,
) -> Result<ResultValue, String> {
    match parsed.command {
        Command::Quit { force } => Ok(ResultValue {
            output: String::new(),
            quit: true || force,
        }),
        Command::Print | Command::Number => {
            let range = evaluate(parsed.range, buffer.line_count(), current)
                .map_err(|e| format!("{e:?}"))?;
            let mut output = String::new();
            for index in range.start..=range.end {
                if let Some(line) = buffer.line(index) {
                    if matches!(parsed.command, Command::Number) {
                        output.push_str(&format!("{}\t", index + 1));
                    }
                    output.push_str(line);
                    output.push('\n');
                }
            }
            Ok(ResultValue {
                output,
                quit: false,
            })
        }
    }
}
