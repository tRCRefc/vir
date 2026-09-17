//! Editable text storage shared by ex and vi.

/// Zero-based row and Unicode scalar gap; column may equal the line length.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Position {
    pub row: usize,
    pub column: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TextBuffer {
    lines: Vec<String>,
    trailing_newline: bool,
}

impl TextBuffer {
    pub fn validate_position(&self, position: Position) -> Result<(), &'static str> {
        if self.is_empty() && position == Position::default() {
            return Ok(());
        }
        match self.line(position.row) {
            Some(line) if position.column <= line.chars().count() => Ok(()),
            _ => Err("character position out of bounds"),
        }
    }

    /// Replace a half-open scalar range atomically, returning the inserted text's end.
    pub fn replace_text(
        &mut self,
        start: Position,
        end: Position,
        text: &str,
    ) -> Result<Position, &'static str> {
        self.validate_position(start)?;
        self.validate_position(end)?;
        if start > end {
            return Err("reversed character range");
        }
        if text.contains('\r') {
            return Err("carriage returns are not supported");
        }
        if start == end && text.is_empty() {
            return Ok(start);
        }
        let first = self.line(start.row).unwrap_or("");
        let last = self.line(end.row).unwrap_or("");
        let byte = |line: &str, column| {
            line.char_indices()
                .nth(column)
                .map_or(line.len(), |(i, _)| i)
        };
        let mut replacement = text.split('\n').map(str::to_owned).collect::<Vec<_>>();
        let cursor = Position {
            row: start.row + replacement.len() - 1,
            column: replacement.last().unwrap().chars().count()
                + if replacement.len() == 1 {
                    start.column
                } else {
                    0
                },
        };
        replacement[0].insert_str(0, &first[..byte(first, start.column)]);
        replacement
            .last_mut()
            .unwrap()
            .push_str(&last[byte(last, end.column)..]);
        if self.is_empty() {
            self.lines = replacement;
            self.trailing_newline = true;
        } else {
            self.lines.splice(start.row..=end.row, replacement);
        }
        Ok(cursor)
    }

    /// Build a buffer from UTF-8 text using LF line endings.
    pub fn from_text(text: &str) -> Result<Self, &'static str> {
        if text.contains('\r') {
            return Err("carriage returns are not supported");
        }
        if text.is_empty() {
            return Ok(Self::default());
        }
        let trailing_newline = text.ends_with('\n');
        let body = text.strip_suffix('\n').unwrap_or(text);
        Ok(Self {
            lines: body.split('\n').map(str::to_owned).collect(),
            trailing_newline,
        })
    }

    pub fn line_count(&self) -> usize {
        self.lines.len()
    }
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }
    pub fn line(&self, index: usize) -> Option<&str> {
        self.lines.get(index).map(String::as_str)
    }
    pub fn has_trailing_newline(&self) -> bool {
        self.trailing_newline
    }

    /// Insert complete lines before `at`. `at == line_count()` appends.
    pub fn insert_lines<I, S>(&mut self, at: usize, lines: I) -> Result<(), &'static str>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        if at > self.lines.len() {
            return Err("line insertion index out of bounds");
        }
        let new_lines: Vec<String> = lines.into_iter().map(Into::into).collect();
        if new_lines
            .iter()
            .any(|line| line.contains('\n') || line.contains('\r'))
        {
            return Err("inserted lines must not contain line endings");
        }
        self.lines.splice(at..at, new_lines);
        if !self.lines.is_empty() {
            self.trailing_newline = true;
        }
        Ok(())
    }

    pub fn delete_lines(&mut self, start: usize, end: usize) -> Result<(), &'static str> {
        if start > end || end >= self.lines.len() {
            return Err("line deletion range out of bounds");
        }
        self.lines.drain(start..=end);
        if self.lines.is_empty() {
            self.trailing_newline = false;
        }
        Ok(())
    }

    pub fn replace_lines<I, S>(
        &mut self,
        start: usize,
        end: usize,
        lines: I,
    ) -> Result<(), &'static str>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        if start > end || end >= self.lines.len() {
            return Err("line replacement range out of bounds");
        }
        let replacement: Vec<String> = lines.into_iter().map(Into::into).collect();
        if replacement
            .iter()
            .any(|line| line.contains('\n') || line.contains('\r'))
        {
            return Err("replacement lines must not contain line endings");
        }
        self.lines.splice(start..=end, replacement);
        self.trailing_newline = !self.lines.is_empty();
        Ok(())
    }

    pub fn to_text(&self) -> String {
        let mut text = self.lines.join("\n");
        if self.trailing_newline {
            text.push('\n');
        }
        text
    }
}

#[cfg(test)]
mod tests {
    use super::TextBuffer;

    #[test]
    fn preserves_lines_and_final_newline() {
        for input in ["", "\n", "one", "one\n", "one\ntwo\n"] {
            let buffer = TextBuffer::from_text(input).unwrap();
            assert_eq!(buffer.to_text(), input);
        }
    }

    #[test]
    fn rejects_cr() {
        assert!(TextBuffer::from_text("one\r\ntwo").is_err());
    }

    #[test]
    fn inserts_lines_and_preserves_state_on_error() {
        let mut buffer = TextBuffer::from_text("one\ntwo").unwrap();
        buffer.insert_lines(1, ["middle"]).unwrap();
        assert_eq!(buffer.to_text(), "one\nmiddle\ntwo\n");
        let before = buffer.clone();
        assert!(buffer.insert_lines(9, ["bad"]).is_err());
        assert_eq!(buffer, before);
        assert!(buffer.insert_lines(1, ["bad\nline"]).is_err());
        assert_eq!(buffer, before);
    }

    #[test]
    fn inserts_into_empty_buffer() {
        let mut buffer = TextBuffer::default();
        buffer.insert_lines(0, ["hello", "world"]).unwrap();
        assert_eq!(buffer.to_text(), "hello\nworld\n");
    }

    #[test]
    fn deletes_and_replaces_ranges() {
        let mut buffer = TextBuffer::from_text("one\ntwo\nthree\n").unwrap();
        buffer.delete_lines(1, 1).unwrap();
        assert_eq!(buffer.to_text(), "one\nthree\n");
        buffer.replace_lines(0, 1, ["new"]).unwrap();
        assert_eq!(buffer.to_text(), "new\n");
        let before = buffer.clone();
        assert!(buffer.delete_lines(1, 0).is_err());
        assert_eq!(buffer, before);
        assert!(buffer.replace_lines(0, 2, ["bad"]).is_err());
        assert_eq!(buffer, before);
    }
}
