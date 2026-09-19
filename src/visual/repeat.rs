//! Replay logical edits through the same range and transaction APIs as live input.
use super::{
    Motion, VisualState,
    input::{InputSession, validate_work},
};
use crate::editor::{Editor, InputAction, TextEdit};

/// The selection/entry operation preceding a text input transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputStart {
    Insert(char),
    Change {
        motion: Option<Motion>,
        count: Option<usize>,
    },
}

impl InputStart {
    /// Historical voOpen uses its count only for terminal screen space.
    /// It always calls vappend with one input repetition.
    pub(super) fn input_count(&self, count: usize) -> usize {
        if matches!(self, Self::Insert('o' | 'O')) {
            1
        } else {
            count.max(1)
        }
    }

    fn normalized(self) -> Self {
        if self == Self::Insert('C') {
            Self::Change {
                motion: Some(Motion::End),
                count: None,
            }
        } else {
            self
        }
    }

    fn begin<'a>(
        &self,
        state: &VisualState,
        editor: &'a mut Editor,
    ) -> Result<TextEdit<'a>, String> {
        match *self {
            Self::Insert(mode) => state.begin_insert(editor, mode),
            Self::Change { motion, count } => state.begin_change(editor, motion, count),
        }
    }
}

#[derive(Debug, Clone)]
pub(super) enum ChangeRecord {
    Delete {
        motion: Option<Motion>,
        count: Option<usize>,
    },
    Join(usize),
    Replace {
        character: char,
        count: usize,
    },
    Put {
        before: bool,
        count: usize,
    },
    Input {
        start: InputStart,
        actions: Vec<InputAction>,
        repetitions: usize,
    },
}

impl ChangeRecord {
    fn override_count(&mut self, count: usize) {
        let count = count.max(1);
        match self {
            Self::Delete { count: stored, .. } => *stored = Some(count),
            Self::Join(stored) => *stored = count.max(2),
            Self::Replace { count: stored, .. } | Self::Put { count: stored, .. } => {
                *stored = count
            }
            Self::Input {
                start: InputStart::Change { count: stored, .. },
                ..
            } => *stored = Some(count),
            Self::Input {
                start, repetitions, ..
            } => *repetitions = start.input_count(count),
        }
    }

    fn apply(&self, state: &mut VisualState, editor: &mut Editor) -> Result<(), String> {
        match self {
            Self::Delete {
                motion: Some(motion),
                count,
            } => state.delete_motion(editor, *motion, *count),
            Self::Delete {
                motion: None,
                count,
            } => state.delete_lines(editor, count.unwrap_or(1)),
            Self::Join(count) => state.join_lines(editor, *count),
            Self::Replace { character, count } => state.replace_counted(editor, *character, *count),
            Self::Put { before, count } => state.put(editor, *before, *count),
            Self::Input {
                start,
                actions,
                repetitions,
            } => {
                validate_work(actions, *repetitions)?;
                let edit = start.begin(state, editor)?;
                // Live input and replay share one transaction controller. Its
                // recipe stays unexpanded; the caller publishes the effective count.
                let mut session = InputSession::new(edit, start.clone(), *repetitions)?;
                for action in actions {
                    session.apply_input(action)?;
                }
                session.commit(state)
            }
        }
    }
}

impl VisualState {
    /// Commit one live input session and publish its logical replay record only
    /// when it changes the text. Dropping the TextEdit instead cancels both.
    pub fn commit_input(
        &mut self,
        mut edit: TextEdit<'_>,
        start: InputStart,
    ) -> Result<(), String> {
        let actions = edit.take_input_actions();
        self.publish_input(edit, start, actions, 1)
    }

    pub(super) fn publish_input(
        &mut self,
        edit: TextEdit<'_>,
        start: InputStart,
        actions: Vec<InputAction>,
        repetitions: usize,
    ) -> Result<(), String> {
        let changed = edit.has_changes();
        let position = edit.commit()?;
        self.finish_insert(position);
        if changed {
            self.last_change = Some(ChangeRecord::Input {
                start: start.normalized(),
                actions,
                repetitions,
            });
        }
        Ok(())
    }

    /// Replay on private state, publishing text, register, cursor, undo and the
    /// effective count together. Error/no-op cannot damage the original record.
    pub fn repeat_change(
        &mut self,
        editor: &mut Editor,
        count: Option<usize>,
    ) -> Result<(), String> {
        let mut change = self.last_change.clone().ok_or("no change to repeat")?;
        if let Some(count) = count {
            change.override_count(count);
        }
        let mut staged_editor = editor.clone();
        let mut staged_state = self.clone();
        change.apply(&mut staged_state, &mut staged_editor)?;
        if staged_editor.buffer() != editor.buffer() {
            staged_state.last_change = Some(change);
            *editor = staged_editor;
            *self = staged_state;
        }
        Ok(())
    }
}
