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
}
