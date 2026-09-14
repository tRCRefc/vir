//! Editable text storage shared by ex and vi.

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TextBuffer {
    lines: Vec<String>,
    trailing_newline: bool,
}

impl TextBuffer {
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
}
