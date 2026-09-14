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
    Number,
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
        return Err(ParseError::Empty);
    }
    let bytes = s.as_bytes();
    let mut i = 0;
    let first = parse_addr(s, &mut i)?;
    let second = if i < bytes.len() && bytes[i] == b',' {
        i += 1;
        parse_addr(s, &mut i)?
    } else {
        None
    };
    if i >= bytes.len() {
        return Err(ParseError::MissingCommand);
    }
    let command = match bytes[i] as char {
        'p' => Command::Print,
        'n' => Command::Number,
        'q' => {
            i += 1;
            let force = i < bytes.len() && bytes[i] == b'!';
            if force {
                i += 1;
            }
            Command::Quit { force }
        }
        _ => return Err(ParseError::UnknownCommand),
    };
    if !matches!(command, Command::Quit { .. }) {
        i += 1;
    }
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
            Address::Line(s[start..*i].parse().unwrap())
        }
        b' ' | b'\t' | b'p' | b'n' | b'q' => return Ok(None),
        _ => return Err(ParseError::InvalidAddress),
    };
    let mut result = a;
    if *i < s.len() && matches!(s.as_bytes()[*i], b'+' | b'-') {
        let sign = if s.as_bytes()[*i] == b'+' { 1 } else { -1 };
        *i += 1;
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
        assert_eq!(parse("1,3n").unwrap().command, Command::Number);
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
