//! Replay logical edits through the same range and transaction APIs as live input.
use super::{Motion, VisualState};
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
            Self::Input { repetitions, .. } => *repetitions = count,
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
                // Counts are untrusted input. Bound input expansion before touching
                // the draft; unlike deletion, insertion cannot clamp at EOF.
                const MAX_INPUT_REPLAY_WORK: usize = 1_000_000;
                let work = actions
                    .iter()
                    .try_fold(1usize, |total, action| {
                        total.checked_add(match action {
                            InputAction::Insert(text) => text.len().max(1),
                            _ => 1,
                        })
                    })
                    .and_then(|work| work.checked_mul(*repetitions));
                if work.is_none_or(|work| work > MAX_INPUT_REPLAY_WORK) {
                    return Err("input repeat too large".into());
                }
                let mut edit = start.begin(state, editor)?;
                // Replay uses this descriptor again, not a recording of its
                // expanded actions (which would multiply the next dot twice).
                edit.take_input_actions();
                if let (InputStart::Insert('i' | 'a' | 'I' | 'A'), [InputAction::Insert(text)]) =
                    (start, actions.as_slice())
                {
                    edit.insert(&text.repeat(*repetitions))?;
                } else {
                    for index in 0..*repetitions {
                        if index > 0 {
                            match start {
                                InputStart::Insert('o') => edit.open_line_below()?,
                                InputStart::Insert('O') => edit.open_line_above()?,
                                _ => {}
                            }
                        }
                        for action in actions {
                            edit.apply_input(action)?;
                        }
                    }
                }
                state.finish_insert(edit.commit()?);
                Ok(())
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
        let changed = edit.has_changes();
        let actions = edit.take_input_actions();
        let position = edit.commit()?;
        self.finish_insert(position);
        if changed {
            self.last_change = Some(ChangeRecord::Input {
                start: start.normalized(),
                actions,
                repetitions: 1,
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
