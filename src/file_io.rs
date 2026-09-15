//! File loading for the shared editor. Loading never creates or writes files.

use std::fs;
use std::io;
use std::path::Path;

use crate::buffer::TextBuffer;

/// Load UTF-8/LF text, or start an empty buffer for a nonexistent path.
/// Other I/O errors and unsupported text formats are returned to the caller.
pub fn load(path: &Path) -> io::Result<TextBuffer> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(TextBuffer::default());
        }
        Err(error) => return Err(error),
    };
    TextBuffer::from_text(&text)
        .map_err(|message| io::Error::new(io::ErrorKind::InvalidData, message))
}
