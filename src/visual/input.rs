//! Input transactions shared by live input and dot, independent of terminal I/O.
use super::{InputStart, VisualState};
use crate::{
    buffer::{Position, TextBuffer},
    editor::{Editor, InputAction, TextEdit},
};

pub(super) const MAX_INPUT_WORK: usize = 1_000_000;
// Corrections do not expand text, but their saved recipe must still be bounded.
const MAX_INPUT_ACTIONS: usize = 1_000_000;

fn check_action_count(count: usize) -> Result<(), String> {
    if count > MAX_INPUT_ACTIONS {
        Err("too many input actions".into())
    } else {
        Ok(())
    }
}

pub(super) fn action_work(action: &InputAction) -> usize {
    match action {
        InputAction::Insert(text)
        | InputAction::InsertLiteral(text)
        | InputAction::ReplaceText(text)
        | InputAction::ReplaceTyped(text) => text.len().max(1),
        InputAction::Replace(c) => c.len_utf8(),
        InputAction::Backspace | InputAction::RestoreReplacement => 0,
        _ => 1,
    }
}

pub(super) fn validate_work(actions: &[InputAction], count: usize) -> Result<(), String> {
    check_action_count(actions.len())?;
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
    line_structure_changed: bool,
}

impl VisualState {
    pub fn begin_counted_insert<'a>(
        &self,
        editor: &'a mut Editor,
        mode: char,
        count: usize,
    ) -> Result<InputSession<'a>, String> {
        self.begin_counted_insert_register(editor, mode, count, None)
    }

    pub fn begin_counted_insert_register<'a>(
        &self,
        editor: &'a mut Editor,
        mode: char,
        count: usize,
        register: Option<char>,
    ) -> Result<InputSession<'a>, String> {
        let start = if mode == 'C' {
            InputStart::ChangeEnd {
                count: count.max(1),
                register,
            }
        } else {
            InputStart::Insert(mode)
        };
        let repetitions = start.repetitions(self, editor, count);
        InputSession::new(start.begin(self, editor)?, start, repetitions)
    }
}

impl<'a> InputSession<'a> {
    pub(super) fn new(
        mut edit: TextEdit<'a>,
        start: InputStart,
        count: usize,
    ) -> Result<Self, String> {
        let count = start.input_count(count);
        if count > 1
            && !matches!(
                start,
                InputStart::Insert('i' | 'a' | 'I' | 'A' | 'R') | InputStart::ChangeEnd { .. }
            )
        {
            return Err("counted input is supported only for i/a/I/A/R".into());
        }
        check_work(Some(1), count)?;
        let line_structure_changed =
            edit.buffer().line_count() != edit.original().buffer().line_count();
        edit.take_input_actions();
        Ok(Self {
            edit,
            start,
            repetitions: count,
            remaining: count,
            prefix: String::new(),
            actions: Vec::new(),
            work: 1,
            line_structure_changed,
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
            InputStart::ChangeNamed { .. } => 'c',
            InputStart::ChangeEnd { .. } => 'c',
        }
    }

    pub fn apply_input(&mut self, action: &InputAction) -> Result<(), String> {
        if let InputAction::Replace(c) = action {
            if c.is_control() && *c != '\n' && *c != '\t' {
                return Err("unsupported replacement character".into());
            }
            let typed = InputAction::ReplaceTyped(c.to_string());
            self.apply_input(&typed)?;
            if let Some(last) = self.actions.last_mut()
                && matches!(last, InputAction::ReplaceTyped(_))
            {
                *last = InputAction::Replace(*c);
            }
            return Ok(());
        }
        if matches!(action, InputAction::ReplaceText(text) if text.contains('\r')) {
            return Err("carriage returns are not supported".into());
        }
        if matches!(action, InputAction::Insert(text) | InputAction::InsertLiteral(text) | InputAction::ReplaceText(text) | InputAction::ReplaceTyped(text) if text.is_empty())
        {
            return Ok(());
        }
        if matches!(action, InputAction::RestoreReplacement) && !self.edit.can_restore_replacement()
        {
            return Ok(());
        }
        let position = self.edit.cursor();
        let line_count_before = self.edit.buffer().line_count();
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
        let erase_insert = if self.remaining > 1 {
            match self.actions.last() {
                Some(InputAction::Insert(text) | InputAction::InsertLiteral(text))
                    if matches!(action, InputAction::Backspace) =>
                {
                    text.chars().last().map(char::len_utf8)
                }
                Some(InputAction::ReplaceText(text) | InputAction::ReplaceTyped(text))
                    if matches!(action, InputAction::RestoreReplacement) =>
                {
                    text.chars().last().map(char::len_utf8)
                }
                Some(InputAction::Replace(c))
                    if matches!(action, InputAction::RestoreReplacement) =>
                {
                    Some(c.len_utf8())
                }
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
            let merges = matches!(
                (self.actions.last(), action),
                (Some(InputAction::Insert(_)), InputAction::Insert(_))
                    | (
                        Some(InputAction::InsertLiteral(_)),
                        InputAction::InsertLiteral(_)
                    )
                    | (
                        Some(InputAction::ReplaceText(_)),
                        InputAction::ReplaceText(_)
                    )
            );
            check_action_count(
                self.actions.len() + usize::from(erase_insert.is_none() && !merges),
            )?;
        }
        if self.remaining > 1 {
            match action {
                InputAction::Insert(text) | InputAction::InsertLiteral(text) => {
                    if let Some((head, tail)) = text.split_once('\n') {
                        let unit = format!("{}{head}", self.prefix);
                        let expanded = format!("{head}{}\n{tail}", unit.repeat(self.remaining - 1));
                        if matches!(action, InputAction::InsertLiteral(_)) {
                            self.edit.insert(&expanded)?;
                        } else {
                            self.edit.insert_input(&expanded)?;
                        }
                        self.prefix.clear();
                        self.remaining = 1;
                    } else {
                        self.edit.apply_input(action)?;
                        self.prefix.push_str(text);
                    }
                }
                InputAction::ReplaceText(text) | InputAction::ReplaceTyped(text) => {
                    if let Some((head, tail)) = text.split_once('\n') {
                        let unit = format!("{}{head}", self.prefix);
                        if matches!(action, InputAction::ReplaceTyped(_)) {
                            self.edit.replace_typed_text(head)?;
                        } else {
                            self.edit.replace_input_text(head)?;
                        }
                        self.edit
                            .insert_replace_copies(&unit.repeat(self.remaining - 1))?;
                        if matches!(action, InputAction::ReplaceTyped(_)) {
                            self.edit.replace_typed_text(&format!("\n{tail}"))?;
                        } else {
                            self.edit.replace_input_text(&format!("\n{tail}"))?;
                        }
                        self.prefix.clear();
                        self.remaining = 1;
                    } else {
                        if matches!(action, InputAction::ReplaceTyped(_)) {
                            self.edit.replace_typed_text(text)?;
                        } else {
                            self.edit.replace_input_text(text)?;
                        }
                        self.prefix.push_str(text);
                    }
                }
                InputAction::RestoreReplacement => {
                    self.edit.replace_backspace()?;
                    if self.edit.cursor() != position {
                        self.prefix.pop();
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
        self.line_structure_changed |= self.edit.cursor().row != position.row
            || self.edit.buffer().line_count() != line_count_before;
        if matches!(action, InputAction::RestoreReplacement) && self.edit.cursor() == position {
            return Ok(());
        }
        self.work = work.unwrap_or(usize::MAX);
        self.record_action(action, erase_insert.is_some());
        Ok(())
    }

    /// Keep the replay recipe normalized without mixing it into text editing.
    fn record_action(&mut self, action: &InputAction, erased_insert: bool) {
        if erased_insert {
            if matches!(self.actions.last(), Some(InputAction::Replace(_))) {
                self.actions.pop();
                return;
            }
            if let Some(
                InputAction::Insert(previous)
                | InputAction::InsertLiteral(previous)
                | InputAction::ReplaceText(previous)
                | InputAction::ReplaceTyped(previous),
            ) = self.actions.last_mut()
            {
                previous.pop();
                if previous.is_empty() {
                    self.actions.pop();
                }
            }
            return;
        }
        match (self.actions.last_mut(), action) {
            (Some(InputAction::Insert(previous)), InputAction::Insert(text))
            | (Some(InputAction::InsertLiteral(previous)), InputAction::InsertLiteral(text))
            | (Some(InputAction::ReplaceText(previous)), InputAction::ReplaceText(text)) => {
                previous.push_str(text)
            }
            _ => self.actions.push(action.clone()),
        }
    }

    pub fn paste(&mut self, text: &str) -> Result<(), String> {
        let text = super::paste::normalize(text)?;
        let action = if self.mode() == 'R' {
            InputAction::ReplaceText(text)
        } else {
            InputAction::InsertLiteral(text)
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
                if let InputStart::ChangeEnd { count, .. } = &mut self.start {
                    *count = 1;
                }
            }
        }
        if self.line_structure_changed {
            state.forget_line();
        }
        state.publish_input(
            self.edit,
            self.start,
            self.actions,
            self.repetitions,
            expand,
        )
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
    fn interrupted_empty_c_keeps_only_the_completed_input_count() {
        let mut editor = Editor::from_buffer(TextBuffer::from_text("\nsecond\nthird\n").unwrap());
        let mut state = VisualState::default();
        let mut input = state.begin_counted_insert(&mut editor, 'C', 3).unwrap();
        input.paste("X").unwrap();
        input.interrupt(&mut state).unwrap();
        assert_eq!(editor.buffer().to_text(), "X\nsecond\nthird\n");
        state.row = 1;
        state.column = 0;
        state.repeat_change(&mut editor, None).unwrap();
        assert_eq!(editor.buffer().to_text(), "X\nX\nthird\n");
    }

    #[test]
    fn zero_expansion_corrections_still_have_a_bounded_replay_recipe() {
        let mut actions = vec![InputAction::Backspace; MAX_INPUT_ACTIONS];
        assert!(validate_work(&actions, MAX_INPUT_WORK).is_ok());
        actions.push(InputAction::RestoreReplacement);
        assert_eq!(
            validate_work(&actions, 1),
            Err("too many input actions".into())
        );
        assert!(validate_work(&[InputAction::Insert("x".into())], MAX_INPUT_WORK).is_err());
    }

    #[test]
    fn interrupted_erased_input_preserves_the_previous_history() {
        for mode in ['i', 'R'] {
            let mut editor = Editor::from_buffer(TextBuffer::from_text("abc").unwrap());
            let mut state = VisualState::default();
            state.replace_character(&mut editor, 'Z').unwrap();
            let before = editor.clone();
            let mut input = state.begin_counted_insert(&mut editor, mode, 1).unwrap();
            input.paste("X").unwrap();
            input
                .apply_input(&if mode == 'R' {
                    InputAction::RestoreReplacement
                } else {
                    InputAction::Backspace
                })
                .unwrap();
            input.interrupt(&mut state).unwrap();
            assert_eq!(editor, before);
            state.column = 1;
            state.repeat_change(&mut editor, None).unwrap();
            assert_eq!(editor.buffer().to_text(), "ZZc");
        }
    }

    #[test]
    fn interrupted_replace_keeps_only_completed_copies() {
        for (text, expected) in [("X", "Xbcdef"), ("X\nY", "XXX\nYcdef")] {
            let mut editor = Editor::from_buffer(TextBuffer::from_text("abcdef").unwrap());
            let mut state = VisualState::default();
            let mut input = state.begin_counted_insert(&mut editor, 'R', 3).unwrap();
            input.paste(text).unwrap();
            input.interrupt(&mut state).unwrap();
            assert_eq!(editor.buffer().to_text(), expected);
            editor.undo().unwrap();
            assert_eq!(editor.buffer().to_text(), "abcdef");
        }
    }

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
