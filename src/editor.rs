//! Shared ex/visual state. Frontends cannot independently mutate its text or dot.

use std::io;
use std::path::{Path, PathBuf};

use crate::buffer::TextBuffer;
use crate::ex::address::AddressError;
use crate::file_io;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Editor {
    buffer: TextBuffer,
    current: Option<usize>,
    path: Option<PathBuf>,
    modified: bool,
}

impl Editor {
    /// Create a clean ex buffer. ex_io.c: rop2/rop3 and ex_cmdsub.c: append
    /// leave dot at the last loaded line outside visual mode.
    pub fn from_buffer(buffer: TextBuffer) -> Self {
        Self {
            current: buffer.line_count().checked_sub(1),
            buffer,
            ..Self::default()
        }
    }

    pub fn buffer(&self) -> &TextBuffer {
        &self.buffer
    }

    pub fn current(&self) -> Option<usize> {
        self.current
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn is_modified(&self) -> bool {
        self.modified
    }

    pub fn insert_lines<I, S>(&mut self, at: usize, lines: I) -> Result<(), String>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let new_lines: Vec<String> = lines.into_iter().map(Into::into).collect();
        if new_lines.is_empty() {
            return Ok(());
        }
        self.buffer
            .insert_lines(at, new_lines.clone())
            .map_err(str::to_owned)?;
        self.current = Some(at + new_lines.len() - 1);
        self.modified = true;
        Ok(())
    }

    pub fn delete_lines(&mut self, start: usize, end: usize) -> Result<(), String> {
        self.buffer
            .delete_lines(start, end)
            .map_err(str::to_owned)?;
        self.current = if self.buffer.is_empty() {
            None
        } else {
            Some(start.min(self.buffer.line_count() - 1))
        };
        self.modified = true;
        Ok(())
    }

    pub fn replace_lines<I, S>(&mut self, start: usize, end: usize, lines: I) -> Result<(), String>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let replacement: Vec<String> = lines.into_iter().map(Into::into).collect();
        self.buffer
            .replace_lines(start, end, replacement.clone())
            .map_err(str::to_owned)?;
        self.current = if self.buffer.is_empty() {
            None
        } else if replacement.is_empty() {
            Some(start.saturating_sub(1).min(self.buffer.line_count() - 1))
        } else {
            Some((start + replacement.len() - 1).min(self.buffer.line_count() - 1))
        };
        self.modified = true;
        Ok(())
    }

    /// Move dot without permitting an invalid index or modifying text.
    pub fn set_current(&mut self, index: usize) -> Result<(), AddressError> {
        if self.buffer.is_empty() {
            return Err(AddressError::EmptyBuffer);
        }
        if index >= self.buffer.line_count() {
            return Err(AddressError::OutOfBounds);
        }
        self.current = Some(index);
        Ok(())
    }

    /// Replace the current file only after a complete successful load.
    /// Missing files become clean empty buffers with a target path.
    pub fn load(&mut self, path: &Path) -> io::Result<()> {
        if self.modified {
            return Err(io::Error::other("unsaved changes"));
        }
        let buffer = file_io::load(path)?;
        let mut loaded = Self::from_buffer(buffer);
        loaded.path = Some(path.to_owned());
        *self = loaded;
        Ok(())
    }

    pub fn save(&mut self) -> io::Result<()> {
        let path = self
            .path
            .clone()
            .ok_or_else(|| io::Error::other("no file name"))?;
        file_io::save(&path, &self.buffer)?;
        self.modified = false;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loading_cannot_discard_modified_state() {
        let mut editor = Editor::from_buffer(TextBuffer::from_text("keep\n").unwrap());
        editor.modified = true;
        let before = editor.clone();
        assert!(editor.load(Path::new("unused-path")).is_err());
        assert_eq!(editor, before);
        use crate::ex::{command::execute, parser::parse};
        assert!(execute(parse("q").unwrap(), &mut editor).is_err());
        assert_eq!(editor, before);
        assert!(execute(parse("q!").unwrap(), &mut editor).unwrap().quit);
        assert_eq!(editor, before);
    }

    #[test]
    fn mutations_update_dot_and_modified() {
        let mut editor = Editor::from_buffer(TextBuffer::from_text("one\ntwo\n").unwrap());
        editor.insert_lines(1, ["middle"]).unwrap();
        assert_eq!(editor.buffer().to_text(), "one\nmiddle\ntwo\n");
        assert_eq!(editor.current(), Some(1));
        assert!(editor.is_modified());

        editor.delete_lines(1, 1).unwrap();
        assert_eq!(editor.buffer().to_text(), "one\ntwo\n");
        assert_eq!(editor.current(), Some(1));

        editor.replace_lines(0, 0, ["ONE", "ONE+"]).unwrap();
        assert_eq!(editor.buffer().to_text(), "ONE\nONE+\ntwo\n");
        assert_eq!(editor.current(), Some(1));
    }

    #[test]
    fn failed_mutation_preserves_editor() {
        let mut editor = Editor::from_buffer(TextBuffer::from_text("one\ntwo\n").unwrap());
        let before = editor.clone();
        assert!(editor.delete_lines(2, 2).is_err());
        assert_eq!(editor, before);
        assert!(editor.replace_lines(0, 1, ["bad\nline"]).is_err());
        assert_eq!(editor, before);
    }
}
