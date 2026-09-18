use crate::buffer::{Position, TextBuffer};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Register {
    Characters(String),
    Lines(Vec<String>),
}

impl Register {
    pub(crate) fn characters(
        buffer: &TextBuffer,
        start: Position,
        end: Position,
    ) -> Result<Self, String> {
        buffer.validate_position(start).map_err(str::to_owned)?;
        buffer.validate_position(end).map_err(str::to_owned)?;
        if start > end {
            return Err("reversed register range".into());
        }
        let mut text = String::new();
        for row in start.row..=end.row {
            if row > start.row {
                text.push('\n');
            }
            let line = buffer.line(row).unwrap_or("");
            let from = if row == start.row { start.column } else { 0 };
            let to = if row == end.row {
                end.column
            } else {
                line.chars().count()
            };
            text.extend(line.chars().skip(from).take(to - from));
        }
        Ok(Self::Characters(text))
    }

    pub(crate) fn lines(buffer: &TextBuffer, start: usize, end: usize) -> Result<Self, String> {
        if start > end || end >= buffer.line_count() {
            return Err("register range out of bounds".into());
        }
        Ok(Self::Lines(
            (start..=end)
                .map(|row| buffer.line(row).unwrap().to_owned())
                .collect(),
        ))
    }
}
