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
    InvalidRange,
    SearchRequiresBuffer,
    MarkRequiresBuffer,
}

pub fn evaluate(
    range: Range,
    line_count: usize,
    current: Option<usize>,
) -> Result<LineRange, AddressError> {
    if line_count == 0 {
        return Err(AddressError::EmptyBuffer);
    }
    let (start, end) = evaluate_coordinates(range, line_count, current, false)?;
    Ok(LineRange {
        start: start - 1,
        end: end - 1,
    })
}

/// Resolve append/insert to a zero-based gap, including an empty buffer.
/// Append accepts address zero; insert uses the last addressed line, like ex.
/// Empty-buffer insert is an intentional extension of the original nonzero rule.
pub fn insertion_index(
    range: Range,
    line_count: usize,
    current: Option<usize>,
    append: bool,
) -> Result<usize, AddressError> {
    let (_, end) = evaluate_coordinates(range, line_count, current, append || line_count == 0)?;
    Ok(if append { end } else { end.saturating_sub(1) })
}

/// Evaluate in ex coordinates: 1..=line_count, with zero allowed only by policy.
fn evaluate_coordinates(
    range: Range,
    line_count: usize,
    current: Option<usize>,
    allow_zero: bool,
) -> Result<(usize, usize), AddressError> {
    let dot = match (line_count, current) {
        (0, None) => 0,
        (_, Some(n)) if n < line_count => n + 1,
        _ => return Err(AddressError::OutOfBounds),
    };
    if range.end.is_some() && (range.start == Some(Address::All) || range.end == Some(Address::All))
    {
        return Err(AddressError::InvalidRange);
    }
    let value = |a: Address| -> Result<(usize, usize), AddressError> {
        let n = match a {
            Address::All if line_count > 0 => return Ok((1, line_count)),
            Address::All => return Err(AddressError::EmptyBuffer),
            Address::Search { .. } => return Err(AddressError::SearchRequiresBuffer),
            Address::Mark { .. } => return Err(AddressError::MarkRequiresBuffer),
            Address::Line(n) => n,
            Address::Current => dot,
            Address::Last => line_count,
            Address::Relative { base, offset } => {
                let n = match base {
                    super::parser::BaseAddress::Line(n) => n,
                    super::parser::BaseAddress::Current => dot,
                    super::parser::BaseAddress::Last => line_count,
                };
                n.checked_add_signed(offset)
                    .ok_or(AddressError::OutOfBounds)?
            }
        };
        if n > line_count || (n == 0 && !allow_zero) {
            Err(AddressError::OutOfBounds)
        } else {
            Ok((n, n))
        }
    };
    let (start, end) = match (range.start, range.end) {
        (None, None) => value(Address::Current)?,
        (Some(a), None) => value(a)?,
        (Some(a), Some(b)) => (value(a)?.0, value(b)?.1),
        (None, Some(_)) => return Err(AddressError::InvalidRange),
    };
    if start > end {
        Err(AddressError::Reversed)
    } else {
        Ok((start, end))
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
