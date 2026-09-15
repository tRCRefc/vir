use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Address {
    Line(usize),
    Current,
    Last,
    All,
    Relative { base: BaseAddress, offset: isize },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BaseAddress {
    Line(usize),
    Current,
    Last,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Range {
    pub start: Option<Address>,
    pub end: Option<Address>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Print,
    PrintNext,
    Number,
    Delete,
    Change,
    Append,
    Insert,
    Write,
    Undo,
    Next,
    Quit { force: bool },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParsedCommand {
    pub range: Range,
    pub command: Command,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    Empty,
    InvalidAddress,
    MissingCommand,
    UnknownCommand,
    UnexpectedArguments,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid ex command: {self:?}")
    }
}
impl std::error::Error for ParseError {}

pub fn parse(input: &str) -> Result<ParsedCommand, ParseError> {
    let s = input.trim();
    if s.is_empty() {
        return Ok(ParsedCommand {
            range: Range {
                start: None,
                end: None,
            },
            command: Command::PrintNext,
        });
    }
    let bytes = s.as_bytes();
    let mut i = 0;
    let first = parse_addr(s, &mut i)?;
    skip_space(s, &mut i);
    let (first, second) = if i < bytes.len() && bytes[i] == b',' {
        if first == Some(Address::All) {
            return Err(ParseError::InvalidAddress);
        }
        i += 1;
        skip_space(s, &mut i);
        let second = parse_addr(s, &mut i)?.unwrap_or(Address::Current);
        if second == Address::All {
            return Err(ParseError::InvalidAddress);
        }
        (Some(first.unwrap_or(Address::Current)), Some(second))
    } else {
        (first, None)
    };
    skip_space(s, &mut i);
    if i >= bytes.len() {
        // ex_cmds.c: a newline after explicit addresses prints that range.
        return Ok(ParsedCommand {
            range: Range {
                start: first,
                end: second,
            },
            command: Command::Print,
        });
    }
    let start = i;
    if bytes[i] == b'#' {
        i += 1;
    } else {
        while i < bytes.len() && bytes[i].is_ascii_alphabetic() {
            i += 1;
        }
    }
    let name = &s[start..i];
    let command = match name {
        "p" | "pr" | "pri" | "prin" | "print" => Command::Print,
        "#" | "nu" | "num" | "numb" | "numbe" | "number" => Command::Number,
        "d" | "de" | "del" | "dele" | "delet" | "delete" => Command::Delete,
        "c" | "ch" | "cha" | "chan" | "chang" | "change" => Command::Change,
        "a" | "ap" | "app" | "appe" | "appen" | "append" => Command::Append,
        "i" | "in" | "ins" | "inse" | "inser" | "insert" => Command::Insert,
        "w" | "wr" | "wri" | "writ" | "write" => Command::Write,
        "u" | "un" | "und" | "undo" => Command::Undo,
        "n" | "ne" | "nex" | "next" => Command::Next,
        "q" | "qu" | "qui" | "quit" => {
            let force = i < bytes.len() && bytes[i] == b'!';
            if force {
                i += 1;
            }
            Command::Quit { force }
        }
        _ => return Err(ParseError::UnknownCommand),
    };
    if !s[i..].trim().is_empty() {
        return Err(ParseError::UnexpectedArguments);
    }
    Ok(ParsedCommand {
        range: Range {
            start: first,
            end: second,
        },
        command,
    })
}

fn skip_space(s: &str, i: &mut usize) {
    while *i < s.len() && matches!(s.as_bytes()[*i], b' ' | b'\t') {
        *i += 1;
    }
}

fn parse_addr(s: &str, i: &mut usize) -> Result<Option<Address>, ParseError> {
    if *i >= s.len() {
        return Ok(None);
    }
    let b = s.as_bytes()[*i];
    let a = match b {
        b'.' => {
            *i += 1;
            Address::Current
        }
        b'$' => {
            *i += 1;
            Address::Last
        }
        b'%' => {
            *i += 1;
            Address::All
        }
        b'0'..=b'9' => {
            let start = *i;
            while *i < s.len() && s.as_bytes()[*i].is_ascii_digit() {
                *i += 1;
            }
            Address::Line(
                s[start..*i]
                    .parse()
                    .map_err(|_| ParseError::InvalidAddress)?,
            )
        }
        b'+' | b'-' => Address::Current,
        _ => return Ok(None),
    };
    skip_space(s, i);
    let mut result = a;
    if *i < s.len() && matches!(s.as_bytes()[*i], b'+' | b'-') {
        let sign = if s.as_bytes()[*i] == b'+' { 1 } else { -1 };
        *i += 1;
        skip_space(s, i);
        let start = *i;
        while *i < s.len() && s.as_bytes()[*i].is_ascii_digit() {
            *i += 1;
        }
        let n = if start == *i {
            1
        } else {
            s[start..*i]
                .parse()
                .map_err(|_| ParseError::InvalidAddress)?
        };
        let base = match a {
            Address::Line(n) => BaseAddress::Line(n),
            Address::Current => BaseAddress::Current,
            Address::Last => BaseAddress::Last,
            _ => return Err(ParseError::InvalidAddress),
        };
        result = Address::Relative {
            base,
            offset: sign * n,
        };
    }
    Ok(Some(result))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_commands() {
        assert_eq!(parse("1,3nu").unwrap().command, Command::Number);
        assert_eq!(parse("%p").unwrap().range.start, Some(Address::All));
        assert_eq!(parse("q").unwrap().command, Command::Quit { force: false });
        assert_eq!(parse("q!").unwrap().command, Command::Quit { force: true });
    }
    #[test]
    fn rejects_bad() {
        assert!(parse("x").is_err());
        assert!(parse("1p foo").is_err());
    }
}
