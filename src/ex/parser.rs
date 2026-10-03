use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Address {
    Line(usize),
    Current,
    Last,
    All,
    Search {
        pattern: String,
        forward: bool,
        offset: isize,
    },
    Relative {
        base: BaseAddress,
        offset: isize,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BaseAddress {
    Line(usize),
    Current,
    Last,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Range {
    pub start: Option<Address>,
    pub end: Option<Address>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Substitute,
    Join,
    Move,
    Copy,
    Read,
    Print,
    PrintNext,
    Number,
    Delete,
    Yank { register: Option<char> },
    Put { register: Option<char> },
    SetAutoIndent(bool),
    Change,
    Append,
    Insert,
    Write,
    WriteQuit,
    Edit { force: bool },
    File,
    Undo,
    Next { force: bool },
    Args,
    Quit { force: bool },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedCommand {
    pub range: Range,
    pub command: Command,
    pub path: Option<String>,
    pub destination: Option<Address>,
    pub substitution: Option<Substitution>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Substitution {
    pub pattern: String,
    pub replacement: String,
    pub global: bool,
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
            path: None,
            destination: None,
            substitution: None,
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
            path: None,
            destination: None,
            substitution: None,
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
    if name == "set" {
        let option = s[i..].trim();
        return match option {
            "ai" | "autoindent" => Ok(ParsedCommand {
                range: Range {
                    start: first,
                    end: second,
                },
                command: Command::SetAutoIndent(true),
                path: None,
                destination: None,
                substitution: None,
            }),
            "noai" | "noautoindent" => Ok(ParsedCommand {
                range: Range {
                    start: first,
                    end: second,
                },
                command: Command::SetAutoIndent(false),
                path: None,
                destination: None,
                substitution: None,
            }),
            _ => Err(ParseError::UnexpectedArguments),
        };
    }
    if matches!(
        name,
        "s" | "su"
            | "sub"
            | "subs"
            | "subst"
            | "substi"
            | "substit"
            | "substitu"
            | "substitut"
            | "substitute"
    ) {
        skip_space(s, &mut i);
        if s.as_bytes().get(i) != Some(&b'/') {
            return Err(ParseError::UnexpectedArguments);
        }
        i += 1;
        let pattern = substitution_part(s, &mut i, true)?;
        let replacement = substitution_part(s, &mut i, false)?;
        let global = match s[i..].trim() {
            "" => false,
            "g" => true,
            _ => return Err(ParseError::UnexpectedArguments),
        };
        return Ok(ParsedCommand {
            range: Range {
                start: first,
                end: second,
            },
            command: Command::Substitute,
            path: None,
            destination: None,
            substitution: Some(Substitution {
                pattern,
                replacement,
                global,
            }),
        });
    }
    let mut command = match name {
        "m" | "mo" | "mov" | "move" => Command::Move,
        "t" | "co" | "cop" | "copy" => Command::Copy,
        "j" | "jo" | "joi" | "join" => Command::Join,
        "r" | "re" | "rea" | "read" => Command::Read,
        "p" | "pr" | "pri" | "prin" | "print" => Command::Print,
        "#" | "nu" | "num" | "numb" | "numbe" | "number" => Command::Number,
        "d" | "de" | "del" | "dele" | "delet" | "delete" => Command::Delete,
        "y" | "ya" | "yan" | "yank" => Command::Yank { register: None },
        "pu" | "put" => Command::Put { register: None },
        "c" | "ch" | "cha" | "chan" | "chang" | "change" => Command::Change,
        "a" | "ap" | "app" | "appe" | "appen" | "append" => Command::Append,
        "i" | "in" | "ins" | "inse" | "inser" | "insert" => Command::Insert,
        "w" | "wr" | "wri" | "writ" | "write" => Command::Write,
        "wq" => Command::WriteQuit,
        "f" | "fi" | "fil" | "file" => Command::File,
        "e" | "ed" | "edi" | "edit" => {
            let force = i < bytes.len() && bytes[i] == b'!';
            if force {
                i += 1;
            }
            Command::Edit { force }
        }
        "u" | "un" | "und" | "undo" => Command::Undo,
        "ar" | "arg" | "args" => Command::Args,
        "n" | "ne" | "nex" | "next" => {
            let force = i < bytes.len() && bytes[i] == b'!';
            if force {
                i += 1;
            }
            Command::Next { force }
        }
        "q" | "qu" | "qui" | "quit" => {
            let force = i < bytes.len() && bytes[i] == b'!';
            if force {
                i += 1;
            }
            Command::Quit { force }
        }
        _ => return Err(ParseError::UnknownCommand),
    };
    let destination = if matches!(command, Command::Move | Command::Copy) {
        skip_space(s, &mut i);
        let address = parse_addr(s, &mut i)?.ok_or(ParseError::InvalidAddress)?;
        if address == Address::All {
            return Err(ParseError::InvalidAddress);
        }
        skip_space(s, &mut i);
        if i != s.len() {
            return Err(ParseError::UnexpectedArguments);
        }
        Some(address)
    } else {
        None
    };
    let rest = &s[i..];
    let path = if rest.is_empty() {
        None
    } else if matches!(command, Command::Yank { .. } | Command::Put { .. })
        && rest.starts_with([' ', '\t'])
    {
        let argument = rest.trim();
        let mut chars = argument.chars();
        let register = chars
            .next()
            .filter(|c| c.is_ascii_alphabetic() || ('1'..='9').contains(c));
        if register.is_none() || chars.next().is_some() {
            return Err(ParseError::UnexpectedArguments);
        }
        if matches!(command, Command::Yank { .. }) && register.is_some_and(|c| c.is_ascii_digit()) {
            return Err(ParseError::UnexpectedArguments);
        }
        command = match command {
            Command::Yank { .. } => Command::Yank { register },
            Command::Put { .. } => Command::Put { register },
            _ => unreachable!(),
        };
        None
    } else if matches!(
        command,
        Command::Write | Command::WriteQuit | Command::Edit { .. } | Command::File | Command::Read
    ) && rest.starts_with([' ', '\t'])
    {
        // One literal path; no shell expansion or command chaining.
        let path = rest.trim();
        if path.contains(['\n', '\r', '\0']) || path.starts_with(['!', '>']) {
            return Err(ParseError::UnexpectedArguments);
        }
        Some(path.to_owned())
    } else {
        return Err(ParseError::UnexpectedArguments);
    };
    Ok(ParsedCommand {
        range: Range {
            start: first,
            end: second,
        },
        command,
        path,
        destination,
        substitution: None,
    })
}

fn substitution_part(s: &str, i: &mut usize, pattern: bool) -> Result<String, ParseError> {
    let mut value = String::new();
    while *i < s.len() {
        let c = s[*i..].chars().next().unwrap();
        *i += c.len_utf8();
        if c == '/' {
            return Ok(value);
        }
        if matches!(c, '\r' | '\n' | '\0') {
            return Err(ParseError::UnexpectedArguments);
        }
        if c == '\\' {
            let next = s[*i..]
                .chars()
                .next()
                .ok_or(ParseError::UnexpectedArguments)?;
            *i += next.len_utf8();
            if matches!(next, '\r' | '\n' | '\0') {
                return Err(ParseError::UnexpectedArguments);
            }
            if !pattern || next != '/' {
                value.push('\\');
            }
            value.push(next);
        } else {
            value.push(c);
        }
    }
    Err(ParseError::UnexpectedArguments)
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
        b'/' | b'?' => {
            *i += 1;
            let mut pattern = String::new();
            loop {
                if *i >= s.len() {
                    return Err(ParseError::InvalidAddress);
                }
                let c = s[*i..].chars().next().unwrap();
                *i += c.len_utf8();
                if c == b as char {
                    break;
                }
                if c == '\\' {
                    let escaped = s[*i..].chars().next().ok_or(ParseError::InvalidAddress)?;
                    *i += escaped.len_utf8();
                    if escaped == b as char {
                        if escaped == '?' {
                            pattern.push_str("\\?");
                        } else {
                            pattern.push(escaped);
                        }
                    } else {
                        pattern.push('\\');
                        pattern.push(escaped);
                    }
                } else {
                    pattern.push(c);
                }
            }
            Address::Search {
                pattern,
                forward: b == b'/',
                offset: 0,
            }
        }
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
    let mut result = a.clone();
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
        if let Address::Search {
            pattern, forward, ..
        } = a
        {
            return Ok(Some(Address::Search {
                pattern,
                forward,
                offset: sign * n,
            }));
        }
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
