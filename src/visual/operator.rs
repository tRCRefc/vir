//! Shared character/line ranges for visual operators.
use super::{
    Motion, VisualState,
    motion::{Endpoint, word_endpoint},
    repeat::ChangeRecord,
};
use crate::{
    buffer::{Position, TextBuffer},
    editor::{Editor, Register, TextEdit},
};

// Bound automatic register expansion, including empty lines and line separators.
const MAX_PUT_WORK: usize = 1_000_000;

fn check_put_work(work: Option<usize>, count: usize) -> Result<(), String> {
    if work
        .and_then(|work| work.checked_mul(count))
        .is_none_or(|work| work > MAX_PUT_WORK)
    {
        Err("paste too large".into())
    } else {
        Ok(())
    }
}

fn validate_put_register(register: &Register, count: usize) -> Result<(), String> {
    let work = match register {
        Register::Characters(text) => {
            if text.is_empty() {
                return Err("register is empty".into());
            }
            Some(text.len())
        }
        Register::Lines(lines) => {
            if lines.is_empty() {
                return Err("register is empty".into());
            }
            lines.iter().try_fold(0usize, |total, line| {
                total.checked_add(line.len())?.checked_add(1)
            })
        }
    };
    check_put_work(work, count.max(1))
}

/// Borrow and preflight before callers copy a register or stage the Editor.
pub(super) fn put_source(
    editor: &Editor,
    selected: Option<char>,
    count: usize,
) -> Result<&Register, String> {
    let register = match selected {
        Some(name @ ('a'..='z' | 'A'..='Z')) => editor.named_register(name),
        Some(name @ '1'..='9') => editor.numbered_register((name as u8 - b'0') as usize),
        Some(_) => return Err("invalid register".into()),
        None => editor.unnamed_register(),
    }
    .ok_or("register is empty")?;
    validate_put_register(register, count)?;
    Ok(register)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Operator {
    Delete,
    Change,
    Yank,
}

impl Operator {
    pub(super) fn from_key(key: char) -> Option<Self> {
        match key {
            'd' => Some(Self::Delete),
            'c' => Some(Self::Change),
            'y' => Some(Self::Yank),
            _ => None,
        }
    }
}

impl Motion {
    pub(super) fn supports_operator(self) -> bool {
        !matches!(self, Self::First | Self::PageUp | Self::PageDown)
    }
}

enum EditRange {
    Lines(usize, usize),
    Characters(Position, Position),
}

impl EditRange {
    fn from_word_endpoint(
        buffer: &TextBuffer,
        origin: Position,
        endpoint: Endpoint,
        operator: Operator,
    ) -> Result<Self, String> {
        let (target, inclusive) = match endpoint {
            Endpoint::Exclusive(target) => (target, false),
            Endpoint::Inclusive(target) => (target, true),
        };
        let start = origin.min(target);
        let mut end = origin.max(target);
        let starts_in_indent = buffer
            .line(start.row)
            .unwrap()
            .chars()
            .take(start.column)
            .all(char::is_whitespace);
        // Exclusive motions ending at column zero do not consume that line.
        if !inclusive && start.row < end.row && end.column == 0 {
            if starts_in_indent {
                return Ok(Self::Lines(start.row, end.row - 1));
            }
            end.row -= 1;
            end.column = buffer.line(end.row).unwrap().chars().count();
        } else if inclusive {
            end.column = end
                .column
                .saturating_add(1)
                .min(buffer.line(end.row).unwrap().chars().count());
        }
        // A multi-line delete covering all nonblank text removes whole lines.
        // Yank/change keep their character range, unless the exclusive rule
        // above already made the motion linewise.
        if operator == Operator::Delete
            && start.row < end.row
            && starts_in_indent
            && buffer
                .line(end.row)
                .unwrap()
                .chars()
                .skip(end.column)
                .all(char::is_whitespace)
        {
            return Ok(Self::Lines(start.row, end.row));
        }
        if start == end {
            return Err("empty operator motion".into());
        }
        Ok(Self::Characters(start, end))
    }

    fn capture(&self, editor: &Editor) -> Result<Register, String> {
        match *self {
            Self::Lines(start, end) => Register::lines(editor.buffer(), start, end),
            Self::Characters(start, end) => Register::characters(editor.buffer(), start, end),
        }
    }
}

impl VisualState {
    pub fn delete_lines(&mut self, editor: &mut Editor, count: usize) -> Result<(), String> {
        self.delete_lines_register(editor, count, None)
    }

    pub fn delete_lines_register(
        &mut self,
        editor: &mut Editor,
        count: usize,
        selected: Option<char>,
    ) -> Result<(), String> {
        validate_write_register(selected)?;
        let (start, end) = self.counted_lines(editor.buffer(), count)?;
        let register = Register::lines(editor.buffer(), start, end)?;
        validate_register_target(editor, selected, &register)?;
        editor.delete_lines(self.row, end)?;
        editor.push_numbered_register(register.clone());
        editor.set_line_undo(self.row, self.row.saturating_sub(1));
        self.publish_register(editor, selected, register)?;
        self.row = editor.current().unwrap_or(0);
        self.column = 0;
        self.preferred_column = 0;
        self.left = 0;
        self.top = self.top.min(self.row);
        self.last_change = Some(match selected {
            Some(register) => ChangeRecord::DeleteNamed {
                motion: None,
                count: Some(count.max(1)),
                register,
            },
            None => ChangeRecord::Delete {
                motion: None,
                count: Some(count.max(1)),
            },
        });
        Ok(())
    }
}

impl VisualState {
    /// Inclusive range for dd/cc/yy; oversized counts stop at EOF.
    fn counted_lines(&self, buffer: &TextBuffer, count: usize) -> Result<(usize, usize), String> {
        if buffer.is_empty() {
            return Err("EmptyBuffer".into());
        }
        let end = self
            .row
            .saturating_add(count.max(1) - 1)
            .min(buffer.line_count() - 1);
        Ok((self.row, end))
    }

    fn motion_range(
        &self,
        editor: &Editor,
        motion: Motion,
        count: Option<usize>,
        operator: Operator,
    ) -> Result<EditRange, String> {
        let motion = if let Motion::RepeatFind { reverse } = motion {
            self.repeated_find_motion(reverse)?
        } else {
            motion
        };
        let buffer = editor.buffer();
        let position = Position {
            row: self.row,
            column: self.column,
        };
        buffer.validate_position(position).map_err(str::to_owned)?;
        if buffer.is_empty() {
            return Err("EmptyBuffer".into());
        }
        if motion == Motion::MatchBracket {
            // Historical % ignores cnt and retains the original operator origin.
            let target = super::matching::target(buffer, position).ok_or("no matching bracket")?;
            // ex_voper includes both ends for %, in either direction.
            return EditRange::from_word_endpoint(
                buffer,
                position,
                Endpoint::Inclusive(target),
                operator,
            );
        }
        if let Motion::Mark { name, linewise } = motion {
            if count.is_some() {
                return Err("mark motions do not accept counts".into());
            }
            let target = editor.mark(name)?;
            return if linewise {
                Ok(EditRange::Lines(
                    self.row.min(target.row),
                    self.row.max(target.row),
                ))
            } else {
                EditRange::from_word_endpoint(
                    buffer,
                    position,
                    Endpoint::Exclusive(target),
                    operator,
                )
            };
        }
        let steps = count.unwrap_or(1).max(1);
        if let Some(endpoint) = word_endpoint(buffer, position, motion, steps, operator) {
            return EditRange::from_word_endpoint(buffer, position, endpoint, operator);
        }
        if let Motion::Find {
            forward,
            till,
            character,
        } = motion
        {
            let target = super::find::target(buffer, position, forward, till, character, steps)
                .ok_or("character not found")?;
            let (start, end) = if forward {
                (
                    position,
                    Position {
                        column: target.column + 1,
                        ..target
                    },
                )
            } else {
                (
                    Position {
                        column: target.column,
                        ..target
                    },
                    position,
                )
            };
            if start == end {
                return Err("empty operator motion".into());
            }
            return Ok(EditRange::Characters(start, end));
        }
        let last = buffer.line_count() - 1;
        let length = buffer.line(self.row).unwrap().chars().count();
        if matches!(motion, Motion::Up | Motion::Down | Motion::Last) {
            let target = match motion {
                Motion::Up => self.row.checked_sub(steps),
                Motion::Down => self.row.checked_add(steps).filter(|row| *row <= last),
                _ => Some(count.map_or(last, |_| steps - 1)).filter(|row| *row <= last),
            }
            .ok_or("operator motion out of bounds")?;
            Ok(EditRange::Lines(self.row.min(target), self.row.max(target)))
        } else {
            let (start, end) = match motion {
                Motion::Left => (
                    Position {
                        column: self.column.saturating_sub(steps),
                        ..position
                    },
                    position,
                ),
                Motion::Right => (
                    position,
                    Position {
                        column: self.column.saturating_add(steps).min(length),
                        ..position
                    },
                ),
                Motion::Home => (
                    Position {
                        column: 0,
                        ..position
                    },
                    position,
                ),
                Motion::End => {
                    let row = self
                        .row
                        .checked_add(steps - 1)
                        .filter(|row| *row <= last)
                        .ok_or("operator motion out of bounds")?;
                    (
                        position,
                        Position {
                            row,
                            column: buffer.line(row).unwrap().chars().count(),
                        },
                    )
                }
                _ => return Err("unsupported operator motion".into()),
            };
            if start == end {
                return Err("empty operator motion".into());
            }
            Ok(EditRange::Characters(start, end))
        }
    }

    pub fn delete_motion(
        &mut self,
        editor: &mut Editor,
        motion: Motion,
        count: Option<usize>,
    ) -> Result<(), String> {
        self.delete_motion_register(editor, motion, count, None)
    }

    pub fn delete_motion_register(
        &mut self,
        editor: &mut Editor,
        motion: Motion,
        count: Option<usize>,
        selected: Option<char>,
    ) -> Result<(), String> {
        validate_write_register(selected)?;
        let range = self.motion_range(editor, motion, count, Operator::Delete)?;
        // vdelete uses the small DEL buffer for unnamed in-line motions.
        // A named prefix or % forces xdw even on one line (ex_voper).
        // Preserve cross-line entry even when an exclusive word endpoint is
        // normalized back to the previous line's end.
        let crossed_word_line = word_endpoint(
            editor.buffer(),
            Position {
                row: self.row,
                column: self.column,
            },
            motion,
            count.unwrap_or(1).max(1),
            Operator::Delete,
        )
        .is_some_and(|endpoint| match endpoint {
            Endpoint::Exclusive(position) | Endpoint::Inclusive(position) => {
                position.row != self.row
            }
        });
        let crossed_mark_line = matches!(motion, Motion::Mark { name, .. }
            if editor.mark(name).is_ok_and(|position| position.row != self.row));
        let numbered = selected.is_some()
            || motion == Motion::MatchBracket
            || crossed_mark_line
            || crossed_word_line
            || match &range {
                EditRange::Lines(..) => true,
                EditRange::Characters(start, end) => start.row != end.row,
            };
        let register = range.capture(editor)?;
        validate_register_target(editor, selected, &register)?;
        match range {
            EditRange::Lines(start, end) => {
                editor.delete_lines(start, end)?;
                editor.set_line_undo(start, start.saturating_sub(1));
                self.row = editor.current().unwrap_or(0);
                self.column = 0;
            }
            EditRange::Characters(start, end) => {
                let mut edit = editor.begin_text_edit(start)?;
                edit.record_character_replacement(start, end);
                edit.delete_range(start, end)?;
                edit.commit()?;
                self.row = start.row;
                self.column = start.column.min(
                    editor
                        .buffer()
                        .line(self.row)
                        .unwrap_or("")
                        .chars()
                        .count()
                        .saturating_sub(1),
                );
            }
        }
        if numbered {
            editor.push_numbered_register(register.clone());
        }
        self.publish_register(editor, selected, register)?;
        self.preferred_column = self.column;
        self.top = self.top.min(self.row);
        self.left = 0;
        self.last_change = Some(match selected {
            Some(register) => ChangeRecord::DeleteNamed {
                motion: Some(motion),
                count,
                register,
            },
            None => ChangeRecord::Delete {
                motion: Some(motion),
                count,
            },
        });
        Ok(())
    }

    pub fn put(&mut self, editor: &mut Editor, before: bool, count: usize) -> Result<(), String> {
        let register = put_source(editor, None, count)?.clone();
        self.put_register(editor, before, count, &register)
    }

    pub fn put_selected(
        &mut self,
        editor: &mut Editor,
        before: bool,
        count: usize,
        selected: Option<char>,
    ) -> Result<(), String> {
        let register = put_source(editor, selected, count)?.clone();
        self.put_register(editor, before, count, &register)?;
        if let Some(register) = selected {
            self.last_change = Some(ChangeRecord::PutNamed {
                before,
                count,
                register,
            });
        }
        Ok(())
    }

    pub fn put_numbered(
        &mut self,
        editor: &mut Editor,
        before: bool,
        count: usize,
        number: usize,
    ) -> Result<(), String> {
        let register = editor
            .numbered_register(number)
            .ok_or("register is empty")?;
        validate_put_register(register, count)?;
        let register = register.clone();
        self.put_register(editor, before, count, &register)?;
        if (1..=9).contains(&number) {
            self.last_change = Some(ChangeRecord::PutNamed {
                before,
                count,
                register: char::from_digit(number as u32, 10).unwrap(),
            });
        }
        Ok(())
    }

    fn put_register(
        &mut self,
        editor: &mut Editor,
        before: bool,
        count: usize,
        register: &Register,
    ) -> Result<(), String> {
        let position = Position {
            row: self.row,
            column: self.column,
        };
        editor
            .buffer()
            .validate_position(position)
            .map_err(str::to_owned)?;
        let count = count.max(1);
        match register {
            Register::Characters(text) => {
                let capacity = text.len().checked_mul(count).ok_or("paste too large")?;
                let mut repeated = String::new();
                repeated
                    .try_reserve_exact(capacity)
                    .map_err(|_| "paste too large")?;
                for _ in 0..count {
                    repeated.push_str(text);
                }
                let length = editor.buffer().line(self.row).unwrap_or("").chars().count();
                let column = if before {
                    self.column
                } else {
                    self.column.saturating_add(1).min(length)
                };
                let mut edit = editor.begin_text_edit(Position {
                    row: self.row,
                    column,
                })?;
                if repeated.contains('\n') {
                    edit.record_line_replacement(self.row, self.row);
                }
                edit.insert_literal(&repeated)?;
                self.finish_insert(edit.commit()?);
            }
            Register::Lines(lines) => {
                let capacity = lines.len().checked_mul(count).ok_or("paste too large")?;
                let mut repeated = Vec::new();
                repeated
                    .try_reserve_exact(capacity)
                    .map_err(|_| "paste too large")?;
                for _ in 0..count {
                    repeated.extend(lines.iter().cloned());
                }
                let row = if editor.buffer().is_empty() {
                    0
                } else {
                    self.row + usize::from(!before)
                };
                editor.insert_lines(row, repeated)?;
                editor.set_line_undo(row.saturating_sub(1), row);
                // insert_lines places dot after the inserted block; visual put starts at its first line.
                editor.set_current(row).map_err(|e| format!("{e:?}"))?;
                self.row = row;
                self.column = editor
                    .buffer()
                    .line(row)
                    .unwrap()
                    .chars()
                    .position(|c| c != ' ' && c != '\t')
                    .unwrap_or(0);
            }
        }
        self.preferred_column = self.column;
        self.top = self.top.min(self.row);
        self.left = 0;
        self.last_change = Some(ChangeRecord::Put { before, count });
        Ok(())
    }

    pub fn yank(
        &self,
        editor: &mut Editor,
        motion: Option<Motion>,
        count: Option<usize>,
    ) -> Result<(), String> {
        self.yank_register(editor, motion, count, None)
    }

    pub fn yank_register(
        &self,
        editor: &mut Editor,
        motion: Option<Motion>,
        count: Option<usize>,
        selected: Option<char>,
    ) -> Result<(), String> {
        let range = if let Some(motion) = motion {
            self.motion_range(editor, motion, count, Operator::Yank)?
        } else {
            let (start, end) = self.counted_lines(editor.buffer(), count.unwrap_or(1))?;
            EditRange::Lines(start, end)
        };
        let register = range.capture(editor)?;
        validate_register_target(editor, selected, &register)?;
        self.publish_register(editor, selected, register)
    }

    fn publish_register(
        &self,
        editor: &mut Editor,
        selected: Option<char>,
        register: Register,
    ) -> Result<(), String> {
        match selected {
            None => editor.unnamed = Some(register),
            Some(name @ 'a'..='z') | Some(name @ 'A'..='Z') => {
                editor.set_named_register(name, register, name.is_ascii_uppercase())?
            }
            Some(_) => return Err("invalid register".into()),
        }
        Ok(())
    }

    /// None selects cc; motions share range resolution with operator-specific policies.
    pub fn begin_change<'a>(
        &self,
        editor: &'a mut Editor,
        motion: Option<Motion>,
        count: Option<usize>,
    ) -> Result<TextEdit<'a>, String> {
        self.begin_change_register(editor, motion, count, None)
    }

    pub fn begin_change_register<'a>(
        &self,
        editor: &'a mut Editor,
        motion: Option<Motion>,
        count: Option<usize>,
        selected: Option<char>,
    ) -> Result<TextEdit<'a>, String> {
        self.begin_change_impl(editor, motion, count, selected, true)
    }

    pub fn begin_substitute<'a>(
        &self,
        editor: &'a mut Editor,
        count: usize,
        selected: Option<char>,
    ) -> Result<TextEdit<'a>, String> {
        // Original s enters vchange, but xdw's numbered-history test only
        // matches c/d. Keep its register policy distinct from an explicit cl.
        self.begin_change_impl(
            editor,
            Some(Motion::Right),
            Some(count.max(1)),
            selected,
            false,
        )
    }

    fn begin_change_impl<'a>(
        &self,
        editor: &'a mut Editor,
        motion: Option<Motion>,
        count: Option<usize>,
        selected: Option<char>,
        save_numbered: bool,
    ) -> Result<TextEdit<'a>, String> {
        if matches!(motion, Some(Motion::End)) && count.unwrap_or(1) <= 1 {
            return self.begin_insert_register(editor, 'C', selected);
        }
        // xdw saves numbered history even when its exclusive endpoint is
        // normalized back onto the previous line (notably cb at column zero).
        let crossed_word_line = motion
            .and_then(|motion| {
                word_endpoint(
                    editor.buffer(),
                    Position {
                        row: self.row,
                        column: self.column,
                    },
                    motion,
                    count.unwrap_or(1).max(1),
                    Operator::Change,
                )
            })
            .is_some_and(|endpoint| match endpoint {
                Endpoint::Exclusive(position) | Endpoint::Inclusive(position) => {
                    position.row != self.row
                }
            });
        let crossed_mark_line = matches!(motion, Some(Motion::Mark { name, .. })
            if editor.mark(name).is_ok_and(|position| position.row != self.row));
        let range = if let Some(motion) = motion {
            self.motion_range(editor, motion, count, Operator::Change)?
        } else {
            editor
                .buffer()
                .validate_position(Position {
                    row: self.row,
                    column: self.column,
                })
                .map_err(str::to_owned)?;
            if editor.buffer().is_empty() {
                let mut edit = editor.begin_text_edit(Position::default())?;
                edit.record_input();
                return Ok(edit);
            }
            let (start, end) = self.counted_lines(editor.buffer(), count.unwrap_or(1))?;
            EditRange::Lines(start, end)
        };
        let register = range.capture(editor)?;
        validate_register_target(editor, selected, &register)?;
        let replaced_lines = match range {
            EditRange::Lines(start, end) => Some((start, end)),
            EditRange::Characters(..) => None,
        };
        let initial_indent = replaced_lines.map(|(start, _)| {
            editor
                .buffer()
                .line(start)
                .unwrap_or("")
                .chars()
                .take_while(|c| matches!(c, ' ' | '\t'))
                .fold(0, |width, c| {
                    if c == '\t' {
                        (width / 8 + 1) * 8
                    } else {
                        width + 1
                    }
                })
        });
        let (start, end) = match range {
            EditRange::Characters(start, end) => (start, end),
            EditRange::Lines(start, end) => (
                Position {
                    row: start,
                    column: 0,
                },
                Position {
                    row: end,
                    column: editor.buffer().line(end).unwrap().chars().count(),
                },
            ),
        };
        let mut edit = editor.begin_text_edit(start)?;
        if let Some((first, last)) = replaced_lines {
            if first < last {
                edit.record_line_replacement(first, last);
            }
        } else {
            edit.record_character_replacement(start, end);
        }
        edit.delete_range(start, end)?;
        if let Some(indent) = initial_indent {
            edit.seed_autoindent(indent)?;
        }
        // Whole-line and cross-line change enter xdw and save deletion
        // history; a named prefix or % also forces that entry on a single line.
        if save_numbered
            && (motion == Some(Motion::MatchBracket)
                || replaced_lines.is_some()
                || start.row != end.row
                || selected.is_some()
                || crossed_word_line
                || crossed_mark_line)
        {
            edit.stage_numbered_register(register.clone());
        }
        match selected {
            None => edit.stage_register(register),
            Some(name @ 'a'..='z') | Some(name @ 'A'..='Z') => {
                edit.stage_named_register(name, register, name.is_ascii_uppercase())
            }
            Some(_) => return Err("invalid register".into()),
        }
        edit.record_input();
        Ok(edit)
    }
}

fn validate_write_register(selected: Option<char>) -> Result<(), String> {
    if matches!(selected, Some(name) if !name.is_ascii_alphabetic()) {
        Err("invalid register".into())
    } else {
        Ok(())
    }
}

pub(super) fn validate_register_target(
    _editor: &Editor,
    selected: Option<char>,
    _register: &Register,
) -> Result<(), String> {
    validate_write_register(selected)
}
