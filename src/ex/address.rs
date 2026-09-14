use super::parser::{Address, Range};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineRange {
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddressError {
    EmptyBuffer,
    OutOfBounds,
    Reversed,
}

pub fn evaluate(
    range: Range,
    line_count: usize,
    current: Option<usize>,
) -> Result<LineRange, AddressError> {
    if line_count == 0 {
        return Err(AddressError::EmptyBuffer);
    }
    let value = |a: Address| -> Result<(usize, usize), AddressError> {
        match a {
            Address::All => Ok((0, line_count - 1)),
            Address::Line(n) if n > 0 && n <= line_count => Ok((n - 1, n - 1)),
            Address::Current => current.map(|n| (n, n)).ok_or(AddressError::EmptyBuffer),
            Address::Last => Ok((line_count - 1, line_count - 1)),
            Address::Relative { base, offset } => {
                let n = match base {
                    super::parser::BaseAddress::Line(n) => {
                        n.checked_sub(1).ok_or(AddressError::OutOfBounds)?
                    }
                    super::parser::BaseAddress::Current => {
                        current.ok_or(AddressError::EmptyBuffer)?
                    }
                    super::parser::BaseAddress::Last => line_count - 1,
                };
                let x = n as isize + offset;
                if x < 0 || x >= line_count as isize {
                    Err(AddressError::OutOfBounds)
                } else {
                    Ok((x as usize, x as usize))
                }
            }
            _ => Err(AddressError::OutOfBounds),
        }
    };
    let (s, e) = match (range.start, range.end) {
        (None, None) => {
            let n = current.ok_or(AddressError::EmptyBuffer)?;
            (n, n)
        }
        (Some(a), None) => value(a)?,
        (Some(a), Some(b)) => (value(a)?.0, value(b)?.1),
        (None, Some(_)) => return Err(AddressError::OutOfBounds),
    };
    if s > e {
        Err(AddressError::Reversed)
    } else {
        Ok(LineRange { start: s, end: e })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ex::parser::*;
    #[test]
    fn evaluates() {
        assert_eq!(
            evaluate(
                Range {
                    start: Some(Address::Line(2)),
                    end: Some(Address::Last)
                },
                3,
                Some(0)
            )
            .unwrap(),
            LineRange { start: 1, end: 2 }
        );
        assert_eq!(
            evaluate(
                Range {
                    start: Some(Address::All),
                    end: None
                },
                3,
                Some(1)
            )
            .unwrap()
            .start,
            0
        );
    }
}
