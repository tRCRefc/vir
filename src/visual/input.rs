//! Input transactions shared by live input and dot, independent of terminal I/O.
use super::{InputStart, VisualState};
use crate::{
    buffer::{Position, TextBuffer},
    editor::{Editor, InputAction, TextEdit},
};

pub(super) const MAX_INPUT_WORK: usize = 1_000_000;

pub(super) fn action_work(action: &InputAction) -> usize {
    match action {
        InputAction::Insert(text) | InputAction::ReplaceText(text) => text.len().max(1),
        _ => 1,
    }
}

pub(super) fn validate_work(actions: &[InputAction], count: usize) -> Result<(), String> {
    let work = actions.iter().try_fold(1usize, |total, action| {
        total.checked_add(action_work(action))
    });
    check_work(work, count)
}

fn check_work(work: Option<usize>, count: usize) -> Result<(), String> {
    if work
        .and_then(|work| work.checked_mul(count))
        .is_none_or(|work| work > MAX_INPUT_WORK)
    {
        Err("input repeat too large".into())
    } else {
        Ok(())
    }
}

pub(super) trait InputTarget {
    fn apply_input(&mut self, action: &InputAction) -> Result<(), String>;
}

impl InputTarget for TextEdit<'_> {
    fn apply_input(&mut self, action: &InputAction) -> Result<(), String> {
        TextEdit::apply_input(self, action)
    }
}

/// The first input line consumes the count, matching historical vappend.
/// Actual edits and the unexpanded recipe are kept separate, so dot cannot
/// accidentally multiply a count that was already expanded during live input.
pub struct InputSession<'a> {
    edit: TextEdit<'a>,
    start: InputStart,
    repetitions: usize,
    remaining: usize,
    prefix: String,
    actions: Vec<InputAction>,
    work: usize,
}

impl VisualState {
    pub fn begin_counted_insert<'a>(
        &self,
        editor: &'a mut Editor,
        mode: char,
        count: usize,
    ) -> Result<InputSession<'a>, String> {
        InputSession::new(
            self.begin_insert(editor, mode)?,
            InputStart::Insert(mode),
            count,
        )
    }
}

impl<'a> InputSession<'a> {
    pub(super) fn new(
        mut edit: TextEdit<'a>,
        start: InputStart,
        count: usize,
    ) -> Result<Self, String> {
        let count = count.max(1);
        if count > 1 && !matches!(start, InputStart::Insert('i' | 'a' | 'I' | 'A')) {
            return Err("counted input is supported only for i/a/I/A".into());
        }
        check_work(Some(1), count)?;
        edit.take_input_actions();
        Ok(Self {
            edit,
            start,
            repetitions: count,
            remaining: count,
            prefix: String::new(),
            actions: Vec::new(),
            work: 1,
        })
    }

    pub fn buffer(&self) -> &TextBuffer {
        self.edit.buffer()
    }
    pub fn cursor(&self) -> Position {
        self.edit.cursor()
    }
    pub fn has_changes(&self) -> bool {
        self.edit.has_changes()
    }
    pub(crate) fn original(&self) -> &Editor {
        self.edit.original()
    }
    pub(super) fn mode(&self) -> char {
        match self.start {
            InputStart::Insert(mode) => mode,
            InputStart::Change { .. } => 'c',
        }
    }

    pub fn apply_input(&mut self, action: &InputAction) -> Result<(), String> {
        if matches!(action, InputAction::Insert(text) | InputAction::ReplaceText(text) if text.is_empty())
        {
            return Ok(());
        }
        let position = self.edit.cursor();
        if matches!(action, InputAction::Backspace) && position == Position::default() {
            return Ok(());
        }
        if matches!(action, InputAction::Delete)
            && position.column
                >= self
                    .edit
                    .buffer()
                    .line(position.row)
                    .unwrap_or("")
                    .chars()
                    .count()
            && position.row + 1 >= self.edit.buffer().line_count()
        {
            return Ok(());
        }
        // Erasing freshly typed text before the first LF reduces the recipe as
        // well as its expansion cost, so reaching the limit never traps Backspace.
        let erase_insert = if self.remaining > 1 && matches!(action, InputAction::Backspace) {
            match self.actions.last() {
                Some(InputAction::Insert(text)) => text.chars().last().map(char::len_utf8),
                _ => None,
            }
        } else {
            None
        };
        let work = match erase_insert {
            Some(bytes) => self.work.checked_sub(bytes),
            None => self.work.checked_add(action_work(action)),
        };
        // Uncounted live typing retains its existing unlimited-input policy.
        // Counted input is bounded before accepting each key/paste, not on Esc.
        if self.repetitions > 1 {
            check_work(work, self.repetitions)?;
        }
        if self.remaining > 1 {
            match action {
                InputAction::Insert(text) => {
                    if let Some((head, tail)) = text.split_once('\n') {
                        let unit = format!("{}{head}", self.prefix);
                        let expanded = format!("{head}{}\n{tail}", unit.repeat(self.remaining - 1));
                        self.edit.insert(&expanded)?;
                        self.prefix.clear();
                        self.remaining = 1;
                    } else {
                        self.edit.insert(text)?;
                        self.prefix.push_str(text);
                    }
                }
                InputAction::Backspace => {
                    self.edit.backspace()?;
                    self.prefix.pop();
                }
                _ => self.edit.apply_input(action)?,
            }
        } else {
            self.edit.apply_input(action)?;
        }
        if matches!(action, InputAction::RestoreReplacement) && self.edit.cursor() == position {
            return Ok(());
        }
        self.work = work.unwrap_or(usize::MAX);
        if erase_insert.is_some() {
            if let Some(InputAction::Insert(previous)) = self.actions.last_mut() {
                previous.pop();
                if previous.is_empty() {
                    self.actions.pop();
                }
            }
            return Ok(());
        }
        if let (Some(InputAction::Insert(previous)), InputAction::Insert(text)) =
            (self.actions.last_mut(), action)
        {
            previous.push_str(text);
        } else {
            self.actions.push(action.clone());
        }
        Ok(())
    }

    pub fn paste(&mut self, text: &str) -> Result<(), String> {
        let text = super::paste::normalize(text)?;
        let action = if self.mode() == 'R' {
            InputAction::ReplaceText(text)
        } else {
            InputAction::Insert(text)
        };
        self.apply_input(&action)
    }

    pub fn commit(self, state: &mut VisualState) -> Result<(), String> {
        self.finish(state, true)
    }

    /// I/O failure retains only input already performed, without inventing the
    /// still-pending copies. Counts consumed at an earlier LF remain recorded.
    pub(super) fn interrupt(self, state: &mut VisualState) -> Result<(), String> {
        self.finish(state, false)
    }

    fn finish(mut self, state: &mut VisualState, expand: bool) -> Result<(), String> {
        if self.remaining > 1 {
            if expand {
                self.edit.insert(&self.prefix.repeat(self.remaining - 1))?;
            } else {
                self.repetitions = 1;
            }
        }
        state.publish_input(self.edit, self.start, self.actions, self.repetitions)
    }
}

impl InputTarget for InputSession<'_> {
    fn apply_input(&mut self, action: &InputAction) -> Result<(), String> {
        InputSession::apply_input(self, action)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replace_noop_backspace_does_not_rewind_dot_at_another_position() {
        let mut editor = Editor::from_buffer(TextBuffer::from_text("abcd").unwrap());
        let mut state = VisualState::default();
        let mut input = state.begin_counted_insert(&mut editor, 'R', 1).unwrap();
        input.apply_input(&InputAction::RestoreReplacement).unwrap();
        input.apply_input(&InputAction::Replace('X')).unwrap();
        input.commit(&mut state).unwrap();
        state.column = 2;
        state.repeat_change(&mut editor, None).unwrap();
        assert_eq!(editor.buffer().to_text(), "XbXd");
        editor.undo().unwrap();
        assert_eq!(editor.buffer().to_text(), "Xbcd");
    }

    #[test]
    fn interrupted_input_retains_completed_expansion_only() {
        for (typed, expected, repeated) in [
            ("X", "Xabc", "XXabc"),
            ("X\nY", "XXX\nYabc", "XXX\nXXX\nYYabc"),
        ] {
            let mut editor = Editor::from_buffer(TextBuffer::from_text("abc").unwrap());
            let mut state = VisualState::default();
            let mut input = state.begin_counted_insert(&mut editor, 'i', 3).unwrap();
            input
                .apply_input(&InputAction::Insert(typed.into()))
                .unwrap();
            input.interrupt(&mut state).unwrap();
            assert_eq!(editor.buffer().to_text(), expected);
            state.repeat_change(&mut editor, None).unwrap();
            assert_eq!(editor.buffer().to_text(), repeated);
        }
    }
}
