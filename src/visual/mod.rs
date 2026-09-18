//! Visual frontend with search and ex editing commands. Layout is independent of terminal I/O.
mod command;
mod insert;
mod operator;
mod search;
mod terminal;
use crate::buffer::TextBuffer;
pub use terminal::run;
use unicode_width::UnicodeWidthChar;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Motion {
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    First,
    Last,
    PageUp,
    PageDown,
}

impl VisualState {
    pub fn delete_lines(
        &mut self,
        editor: &mut crate::editor::Editor,
        count: usize,
    ) -> Result<(), String> {
        if editor.buffer().is_empty() {
            return Err("EmptyBuffer".into());
        }
        let end = self
            .row
            .saturating_add(count.max(1) - 1)
            .min(editor.buffer().line_count() - 1);
        editor.delete_lines(self.row, end)?;
        self.row = editor.current().unwrap_or(0);
        self.column = 0;
        self.preferred_column = 0;
        self.left = 0;
        self.top = self.top.min(self.row);
        Ok(())
    }
}

#[derive(Debug, Default)]
pub struct VisualState {
    pub row: usize,
    /// Unicode scalar index, not a byte or terminal column.
    pub column: usize,
    pub top: usize,
    pub left: usize,
    preferred_column: usize,
    search_forward: Option<bool>,
}

#[derive(Debug)]
pub struct Frame {
    pub lines: Vec<String>,
    pub status: String,
    pub cursor: Option<(u16, u16)>,
}

// One display token per scalar. Zero-width scalars are shown as a visible
// placeholder until grapheme-cluster layout is supported. Never emit controls.
fn token(c: char, column: usize) -> (String, usize) {
    if c == '\t' {
        let width = 8 - column % 8;
        return (" ".repeat(width), width);
    }
    if c.is_ascii_control() {
        let glyph = if c == '\x7f' {
            '?'
        } else {
            (c as u8 + 64) as char
        };
        return (format!("^{glyph}"), 2);
    }
    match c.width() {
        Some(width @ 1..=2) => (c.to_string(), width),
        _ => ("�".into(), 1),
    }
}

fn screen_column(line: &str, index: usize) -> usize {
    line.chars()
        .take(index)
        .fold(0, |column, c| column + token(c, column).1)
}

/// Clip on cell boundaries, padding rather than outputting half of a wide
/// glyph. This is also used for file names in the status line.
pub fn clip(line: &str, left: usize, width: usize) -> String {
    let mut output = String::new();
    let mut column = 0;
    let right = left.saturating_add(width);
    for c in line.chars() {
        let (text, size) = token(c, column);
        let end = column + size;
        if column >= right {
            break;
        }
        if end > left {
            if column >= left && end <= right {
                output.push_str(&text);
            } else {
                output.push_str(&" ".repeat(end.min(right) - column.max(left)));
            }
        }
        column = end;
    }
    output
}

impl VisualState {
    pub fn move_cursor(&mut self, motion: Motion, buffer: &TextBuffer, height: u16) {
        self.move_counted(motion, None, buffer, height);
    }

    pub fn move_counted(
        &mut self,
        motion: Motion,
        count: Option<usize>,
        buffer: &TextBuffer,
        height: u16,
    ) {
        if buffer.is_empty() {
            return;
        }
        let steps = count.unwrap_or(1).max(1);
        let last = buffer.line_count() - 1;
        let page = usize::from(height.saturating_sub(1).max(1)).saturating_mul(steps);
        let vertical = matches!(
            motion,
            Motion::Up | Motion::Down | Motion::PageUp | Motion::PageDown
        );
        match motion {
            Motion::Left => self.column = self.column.saturating_sub(steps),
            Motion::Right => self.column = self.column.saturating_add(steps),
            Motion::Up => self.row = self.row.saturating_sub(steps),
            Motion::Down => self.row = self.row.saturating_add(steps).min(last),
            Motion::PageUp => self.row = self.row.saturating_sub(page),
            Motion::PageDown => self.row = self.row.saturating_add(page).min(last),
            Motion::Home => self.column = 0,
            Motion::End => {
                self.row = self.row.saturating_add(steps - 1).min(last);
                self.column = usize::MAX;
            }
            Motion::First => {
                self.row = if count.is_some() {
                    steps.saturating_sub(1).min(last)
                } else {
                    0
                };
                self.column = 0;
            }
            Motion::Last => {
                self.row = if count.is_some() {
                    steps.saturating_sub(1).min(last)
                } else {
                    last
                };
                self.column = 0;
            }
        }
        if vertical {
            self.column = self.preferred_column;
        }
        self.column = self.column.min(
            buffer
                .line(self.row)
                .unwrap()
                .chars()
                .count()
                .saturating_sub(1),
        );
        if !vertical {
            self.preferred_column = self.column;
        }
    }

    pub fn frame(&mut self, buffer: &TextBuffer, name: &str, width: u16, height: u16) -> Frame {
        self.layout(buffer, name, width, height, "VIEW")
    }

    pub fn insert_frame(
        &mut self,
        buffer: &TextBuffer,
        name: &str,
        width: u16,
        height: u16,
    ) -> Frame {
        self.layout(buffer, name, width, height, "INSERT")
    }

    pub fn replace_frame(
        &mut self,
        buffer: &TextBuffer,
        name: &str,
        width: u16,
        height: u16,
    ) -> Frame {
        self.layout(buffer, name, width, height, "REPLACE")
    }

    fn layout(
        &mut self,
        buffer: &TextBuffer,
        name: &str,
        width: u16,
        height: u16,
        mode: &str,
    ) -> Frame {
        let inserting = mode != "VIEW";
        // Reserve the final column to avoid terminal autowrap at bottom-right.
        let columns = usize::from(width.saturating_sub(1));
        let rows = usize::from(height.saturating_sub(1));
        self.row = self.row.min(buffer.line_count().saturating_sub(1));
        let line = buffer.line(self.row).unwrap_or("");
        self.column = self
            .column
            .min(line.chars().count().saturating_sub(usize::from(!inserting)));
        let x = screen_column(line, self.column);
        if self.row < self.top {
            self.top = self.row;
        }
        if rows > 0 && self.row >= self.top + rows {
            self.top = self.row + 1 - rows;
        }
        if x < self.left {
            self.left = x;
        }
        let glyph_width = line.chars().nth(self.column).map_or(1, |c| token(c, x).1);
        if columns > 0 && x + glyph_width > self.left + columns {
            self.left = (x + glyph_width).saturating_sub(columns).min(x);
        }
        let lines = (0..rows)
            .map(|offset| match buffer.line(self.top + offset) {
                Some(line) => clip(line, self.left, columns),
                None => clip("~", 0, columns),
            })
            .collect();
        let status = clip(
            &format!(
                "[{}] {name}  {}/{}  col {}  {}",
                mode,
                if buffer.is_empty() { 0 } else { self.row + 1 },
                buffer.line_count(),
                self.column + 1,
                if inserting {
                    "Esc:normal"
                } else {
                    ":command q:quit"
                }
            ),
            0,
            columns,
        );
        let cursor = if rows > 0 && columns > 0 {
            Some(((x - self.left) as u16, (self.row - self.top) as u16))
        } else {
            None
        };
        Frame {
            lines,
            status,
            cursor,
        }
    }
}
