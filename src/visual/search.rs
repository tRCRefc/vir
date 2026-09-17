use super::VisualState;
use crate::editor::Editor;
use regex::Regex;

impl VisualState {
    /// Search by scalar position, publishing history and cursor only on success.
    pub fn search(
        &mut self,
        editor: &mut Editor,
        pattern: &str,
        forward: bool,
    ) -> Result<(), String> {
        self.find(editor, pattern, forward)?;
        self.search_forward = Some(forward);
        Ok(())
    }

    pub fn repeat_search(&mut self, editor: &mut Editor, reverse: bool) -> Result<(), String> {
        self.find(editor, "", self.search_forward.unwrap_or(true) ^ reverse)
    }

    fn find(&mut self, editor: &mut Editor, pattern: &str, forward: bool) -> Result<(), String> {
        let pattern = if pattern.is_empty() {
            editor.last_search().ok_or("no previous search pattern")?
        } else {
            pattern
        };
        let regex = Regex::new(pattern).map_err(|e| format!("invalid search pattern: {e}"))?;
        let count = editor.buffer().line_count();
        if count == 0 {
            return Err("EmptyBuffer".into());
        }
        let origin = self.row.min(count - 1);
        let mut row = origin;
        // Visit the origin twice: first the unvisited suffix/prefix, then wrap.
        for step in 0..=count {
            let line = editor.buffer().line(row).unwrap();
            let mut positions = line
                .char_indices()
                .map(|(byte, _)| byte)
                .collect::<Vec<_>>();
            positions.push(line.len());
            let mut offset = 0;
            let mut found = None;
            while let Some(matched) = regex.find_at(line, offset) {
                let column = positions
                    .binary_search(&matched.start())
                    .expect("scalar boundary");
                let eligible = if step == 0 {
                    if forward {
                        column > self.column
                    } else {
                        column < self.column
                    }
                } else if step == count {
                    if forward {
                        column <= self.column
                    } else {
                        column >= self.column
                    }
                } else {
                    true
                };
                if eligible {
                    found = Some(column);
                    if forward {
                        break;
                    }
                }
                // Advance one scalar from the start to retain overlapping matches.
                let Some(next) = positions.get(column + 1) else {
                    break;
                };
                offset = *next;
            }
            if let Some(column) = found {
                let pattern = pattern.to_owned();
                editor.set_current(row).map_err(|e| format!("{e:?}"))?;
                editor.remember_search(pattern);
                self.row = row;
                self.column = column.min(positions.len().saturating_sub(2));
                self.preferred_column = self.column;
                return Ok(());
            }
            row = if forward {
                (row + 1) % count
            } else {
                (row + count - 1) % count
            };
        }
        Err("pattern not found".into())
    }
}
