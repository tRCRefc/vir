//! Bracketed-paste is a terminal extension; feed literal text to vi transactions.
use super::{InputStart, VisualState};
use crate::editor::{Editor, TextEdit};

pub(super) const MAX_PASTE_BYTES: usize = 64 * 1024;

pub(super) fn normalize(text: &str) -> Result<String, String> {
    if text.len() > MAX_PASTE_BYTES {
        return Err("paste exceeds 64 KiB; nothing pasted".into());
    }
    // The buffer uses LF. Do not interpret Esc, control keys or command text.
    Ok(text.replace("\r\n", "\n").replace('\r', "\n"))
}

/// Join the active i/a/o/c/R transaction; the caller commits on Esc as usual.
pub fn paste_input(edit: &mut TextEdit<'_>, replace: bool, text: &str) -> Result<(), String> {
    let text = normalize(text)?;
    if replace {
        edit.replace_input_text(&text)
    } else {
        edit.insert(&text)
    }
}

impl VisualState {
    /// Like Vim's terminal paste: insert before column zero, otherwise after
    /// the current character, then return to Normal. Dot appends the same text.
    pub fn paste(&mut self, editor: &mut Editor, text: &str) -> Result<(), String> {
        let text = normalize(text)?;
        if text.is_empty() {
            return Ok(());
        }
        let entry = if self.column == 0 { 'i' } else { 'a' };
        let mut edit = self.begin_insert(editor, entry)?;
        edit.insert(&text)?;
        // Normal-mode bracketed paste repeats after the cursor, even when the
        // initial paste was inserted before column zero (Vim reference behavior).
        self.commit_input(edit, InputStart::Insert('a'))
    }
}
