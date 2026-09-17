//! Shared ex/visual state. Frontends cannot independently mutate its text or dot.

use std::io;
mod text_edit;
use std::path::{Path, PathBuf};
pub use text_edit::TextEdit;

use crate::buffer::TextBuffer;
use crate::ex::address::{AddressError, LineRange};
use crate::file_io;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Editor {
    buffer: TextBuffer,
    current: Option<usize>,
    path: Option<PathBuf>,
    modified: bool,
    path_writable: bool,
    arguments: Vec<PathBuf>,
    argument_index: Option<usize>,
    warned_remaining: Option<usize>,
    last_search: Option<String>,
    undo: Option<(TextBuffer, Option<usize>)>,
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
        if at > self.buffer.line_count() {
            return Err("line insertion index out of bounds".into());
        }
        if new_lines.is_empty() {
            return Ok(());
        }
        self.edit(move |buffer| {
            let count = new_lines.len();
            buffer.insert_lines(at, new_lines)?;
            Ok(Some(at + count - 1))
        })
    }

    pub fn delete_lines(&mut self, start: usize, end: usize) -> Result<(), String> {
        self.edit(|buffer| {
            buffer.delete_lines(start, end)?;
            Ok(buffer
                .line_count()
                .checked_sub(1)
                .map(|last| start.min(last)))
        })
    }

    pub fn replace_lines<I, S>(&mut self, start: usize, end: usize, lines: I) -> Result<(), String>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let replacement: Vec<String> = lines.into_iter().map(Into::into).collect();
        self.edit(move |buffer| {
            let count = replacement.len();
            buffer.replace_lines(start, end, replacement)?;
            Ok(buffer.line_count().checked_sub(1).map(|last| {
                if count == 0 {
                    start.saturating_sub(1).min(last)
                } else {
                    (start + count - 1).min(last)
                }
            }))
        })
    }

    /// Publish one complete edit only after validation succeeds. Failed edits
    /// preserve the text, cursor, modified flag and reversible undo snapshot.
    fn edit(
        &mut self,
        apply: impl FnOnce(&mut TextBuffer) -> Result<Option<usize>, &'static str>,
    ) -> Result<(), String> {
        let mut buffer = self.buffer.clone();
        let current = apply(&mut buffer).map_err(str::to_owned)?;
        let previous = std::mem::replace(&mut self.buffer, buffer);
        self.undo = Some((previous, self.current));
        self.current = current;
        self.modified = true;
        Ok(())
    }

    /// Original ex undo is its own inverse; it is a change even if it restores
    /// saved text. Saving leaves this snapshot available.
    pub fn undo(&mut self) -> Result<(), String> {
        let (buffer, current) = self
            .undo
            .as_mut()
            .ok_or_else(|| "nothing to undo".to_string())?;
        std::mem::swap(&mut self.buffer, buffer);
        std::mem::swap(&mut self.current, current);
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
        self.load_with_force(path, false)
    }

    pub fn load_with_force(&mut self, path: &Path, force: bool) -> io::Result<()> {
        if self.modified && !force {
            return Err(io::Error::other("unsaved changes"));
        }
        let (buffer, existed) = file_io::load_with_origin(path)?;
        let mut loaded = Self::from_buffer(buffer);
        loaded.path = Some(path.to_owned());
        loaded.path_writable = existed;
        loaded.arguments = self.arguments.clone();
        loaded.argument_index = self.argument_index;
        loaded.warned_remaining = self.warned_remaining;
        loaded.last_search = self.last_search.clone();
        *self = loaded;
        Ok(())
    }

    /// Initialize a file-list session only after its first file loads.
    pub fn open_arguments(&mut self, paths: Vec<PathBuf>) -> io::Result<()> {
        let mut loaded = Self::default();
        if self.modified {
            return Err(io::Error::other("unsaved changes"));
        }
        if let Some(first) = paths.first() {
            loaded.load(first)?;
            loaded.argument_index = Some(0);
        }
        loaded.arguments = paths;
        *self = loaded;
        Ok(())
    }

    pub fn last_search(&self) -> Option<&str> {
        self.last_search.as_deref()
    }
    pub(crate) fn remember_search(&mut self, pattern: String) {
        self.last_search = Some(pattern);
    }

    pub fn arguments(&self) -> &[PathBuf] {
        &self.arguments
    }
    pub fn argument_index(&self) -> Option<usize> {
        self.argument_index
    }

    pub fn next_file(&mut self, force: bool) -> io::Result<()> {
        if self.modified && !force {
            return Err(io::Error::other("unsaved changes"));
        }
        let next = self.argument_index.map_or(0, |index| index + 1);
        let path = self
            .arguments
            .get(next)
            .ok_or_else(|| io::Error::other("no more files"))?
            .clone();
        self.load_with_force(&path, force)?;
        self.argument_index = Some(next);
        self.warned_remaining = None;
        Ok(())
    }

    /// Like ex nomore: warn once for the current remaining file count.
    pub fn check_quit(&mut self, force: bool) -> Result<(), String> {
        if force {
            return Ok(());
        }
        if self.modified {
            return Err("unsaved changes".into());
        }
        let remaining = self
            .arguments
            .len()
            .saturating_sub(self.argument_index.map_or(0, |n| n + 1));
        if remaining > 0 && self.warned_remaining != Some(remaining) {
            self.warned_remaining = Some(remaining);
            return Err(format!(
                "{remaining} more file{} to edit",
                if remaining == 1 { "" } else { "s" }
            ));
        }
        Ok(())
    }

    pub fn set_path(&mut self, path: &Path) {
        if self.path.as_deref() != Some(path) {
            self.path = Some(path.to_owned());
            self.path_writable = false;
        }
    }

    pub fn save(&mut self) -> io::Result<()> {
        self.write_to(None)
    }

    /// Writing another file exports a copy without renaming the current buffer.
    /// An unnamed buffer adopts its first successful write destination.
    pub fn write_to(&mut self, destination: Option<&Path>) -> io::Result<()> {
        self.write_range_to(destination, None)
    }

    /// An explicit inclusive range exports selected lines. Only a full write
    /// clears modified; partial writes never replace the current destination.
    pub fn write_range_to(
        &mut self,
        destination: Option<&Path>,
        range: Option<LineRange>,
    ) -> io::Result<()> {
        let full = match range {
            None => true,
            Some(range) => {
                if range.start > range.end || range.end >= self.buffer.line_count() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "invalid write range",
                    ));
                }
                range.start == 0 && range.end + 1 == self.buffer.line_count()
            }
        };
        let path = destination
            .or(self.path())
            .ok_or_else(|| io::Error::other("no file name"))?
            .to_owned();
        let current = self.path.as_deref() == Some(path.as_path());
        if current && !full {
            return Err(io::Error::other(
                "cannot write partial buffer to current file",
            ));
        }
        let selected;
        let buffer = if full {
            &self.buffer
        } else {
            let range = range.expect("partial write has a range");
            let mut text = String::new();
            for index in range.start..=range.end {
                text.push_str(self.buffer.line(index).expect("validated range"));
                if index + 1 < self.buffer.line_count() || self.buffer.has_trailing_newline() {
                    text.push('\n');
                }
            }
            selected = TextBuffer::from_text(&text).map_err(io::Error::other)?;
            &selected
        };
        if current && self.path_writable {
            file_io::save(&path, buffer)?;
        } else {
            file_io::save_new(&path, buffer)?;
        }
        if self.path.is_none() {
            self.path = Some(path);
            self.path_writable = true;
        } else if current {
            self.path_writable = true;
        }
        // A full-buffer write clears modified even when exporting, like ex.
        if full {
            self.modified = false;
        }
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

    #[test]
    fn undo_restores_previous_edit() {
        let mut editor = Editor::from_buffer(TextBuffer::from_text("one\n").unwrap());
        editor.insert_lines(1, ["two"]).unwrap();
        editor.undo().unwrap();
        assert_eq!(editor.buffer().to_text(), "one\n");
        assert!(editor.is_modified());
        editor.undo().unwrap();
        assert_eq!(editor.buffer().to_text(), "one\ntwo\n");
    }

    #[test]
    fn undo_toggles_only_the_latest_edit() {
        let mut editor = Editor::from_buffer(TextBuffer::from_text("one\n").unwrap());
        editor.insert_lines(1, ["two"]).unwrap();
        editor.insert_lines(2, ["three"]).unwrap();
        editor.undo().unwrap();
        assert_eq!(editor.buffer().to_text(), "one\ntwo\n");
        editor.undo().unwrap();
        assert_eq!(editor.buffer().to_text(), "one\ntwo\nthree\n");
    }
}
