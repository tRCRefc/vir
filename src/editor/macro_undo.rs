//! ex_vops.c::vmacchng promotes the second change to whole-buffer undo.
use super::{Editor, UndoLocation};
use crate::buffer::TextBuffer;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum MacroPhase {
    #[default]
    Inactive,
    Unchanged,
    Once,
    Grouped,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(super) struct MacroUndo {
    phase: MacroPhase,
    // U uses its line undo even while the full-buffer save remains available.
    // getbr restores VMANY at the end of the stream (ex_vget.c).
    saved_group: Option<(TextBuffer, Option<usize>)>,
}

impl MacroUndo {
    pub(super) fn after_reload(&self) -> Self {
        Self {
            phase: if self.phase == MacroPhase::Inactive {
                MacroPhase::Inactive
            } else {
                MacroPhase::Unchanged
            },
            saved_group: None,
        }
    }

    /// Keep the live reversible pair: an intervening u can have swapped it.
    pub(super) fn save_change(&mut self) -> bool {
        match self.phase {
            MacroPhase::Inactive => true,
            MacroPhase::Unchanged => {
                self.phase = MacroPhase::Once;
                true
            }
            MacroPhase::Once | MacroPhase::Grouped => {
                self.phase = MacroPhase::Grouped;
                self.saved_group.is_some()
            }
        }
    }

    pub(super) fn is_grouped(&self) -> bool {
        self.phase == MacroPhase::Grouped
    }

    pub(super) fn uses_line_undo(&self) -> bool {
        !self.is_grouped() || self.saved_group.is_some()
    }
}

impl Editor {
    pub(super) fn save_undo(&mut self, previous: TextBuffer) {
        let grouped = self.macro_undo.is_grouped();
        if self.macro_undo.save_change() {
            self.undo = Some((previous, self.current));
            self.undo_location = None;
        } else {
            // vmacchng first performs vundo(0), then saves the resulting dot.
            // A line insertion/deletion can restore a different row from the
            // cursor at command entry; preserve that row on promotion.
            if !grouped && let Some((_, current)) = &mut self.undo {
                match self.undo_location {
                    Some(UndoLocation::Lines { next, .. }) => *current = Some(next),
                    Some(UndoLocation::InitialCharacter(p)) => *current = Some(p.row),
                    _ => {}
                }
            }
            self.undo_location = Some(UndoLocation::Buffer);
        }
    }

    pub(crate) fn begin_macro_undo(&mut self) {
        // macpush resets VC_NOCHANGE for a nested call only while FIXUNDO
        // is still enabled; once grouped, nested calls share the same save.
        if !self.macro_undo.is_grouped() {
            self.macro_undo.phase = MacroPhase::Unchanged;
        }
    }

    pub(crate) fn end_macro_undo(&mut self) {
        if let Some(saved) = self.macro_undo.saved_group.take() {
            self.undo = Some(saved);
            self.undo_location = Some(UndoLocation::Buffer);
        }
        self.macro_undo = MacroUndo::default();
    }

    /// vUndo does not call vmacchng: U installs a line snapshot independently
    /// of the macro's change count, retaining any existing whole-buffer save.
    pub(super) fn with_line_undo<T>(
        &mut self,
        apply: impl FnOnce(&mut Self) -> Result<T, String>,
    ) -> Result<T, String> {
        let mut group = std::mem::take(&mut self.macro_undo);
        let saved = (group.is_grouped() && group.saved_group.is_none())
            .then(|| self.undo.clone())
            .flatten();
        let result = apply(self);
        if result.is_ok() && saved.is_some() {
            group.saved_group = saved;
        }
        self.macro_undo = group;
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{buffer::TextBuffer, visual::VisualState};

    fn editor() -> Editor {
        Editor::from_buffer(TextBuffer::from_text("  abc\n  def\n  ghi\n").unwrap())
    }

    #[test]
    fn several_changes_swap_together_and_retain_live_dot_and_registers() {
        let mut editor = editor();
        let mut state = VisualState::default();
        editor.set_current(0).unwrap();
        editor.begin_macro_undo();
        state.column = 2;
        state.delete_character(&mut editor).unwrap();
        state.row = 1;
        editor.set_current(1).unwrap();
        state.delete_character(&mut editor).unwrap();
        editor.end_macro_undo();
        assert_eq!(editor.buffer().to_text(), "  bc\n  ef\n  ghi\n");
        let register = editor.unnamed_register().cloned();
        // Movement after the macro determines redo's dot (UNDALL/undadot).
        state.row = 2;
        editor.set_current(2).unwrap();
        state.undo(&mut editor).unwrap();
        assert_eq!(editor.buffer().to_text(), "  abc\n  def\n  ghi\n");
        assert_eq!((state.row, state.column), (0, 2));
        state.undo(&mut editor).unwrap();
        assert_eq!(editor.buffer().to_text(), "  bc\n  ef\n  ghi\n");
        assert_eq!((state.row, state.column), (2, 2));
        assert_eq!(editor.unnamed_register(), register.as_ref());
        // Dot still represents the last edit inside the macro.
        state.repeat_change(&mut editor, None).unwrap();
        assert_eq!(editor.buffer().to_text(), "  bc\n  ef\n  hi\n");
        state.undo(&mut editor).unwrap();
        assert_eq!(editor.buffer().to_text(), "  bc\n  ef\n  ghi\n");
    }

    #[test]
    fn no_change_preserves_history_and_one_change_preserves_character_placement() {
        let mut editor = editor();
        let mut state = VisualState::default();
        state.column = 3;
        state.replace_character(&mut editor, 'X').unwrap();
        let before = editor.clone();
        editor.begin_macro_undo();
        editor.end_macro_undo();
        assert_eq!(editor, before);
        editor.begin_macro_undo();
        state.replace_character(&mut editor, 'X').unwrap();
        editor.end_macro_undo();
        state.column = 0;
        state.undo(&mut editor).unwrap();
        assert_eq!(state.column, 3);
        assert_eq!(editor.buffer().line(0), Some("  aXc"));
    }

    #[test]
    fn nested_calls_restart_before_promotion_and_share_after_promotion() {
        for edits_before_nested in [1, 2] {
            let mut editor = editor();
            let mut state = VisualState::default();
            state.column = 2;
            editor.begin_macro_undo();
            for _ in 0..edits_before_nested {
                state.delete_character(&mut editor).unwrap();
            }
            editor.begin_macro_undo();
            state.replace_character(&mut editor, 'X').unwrap();
            state.replace_character(&mut editor, 'Y').unwrap();
            editor.end_macro_undo();
            state.undo(&mut editor).unwrap();
            assert_eq!(
                editor.buffer().line(0),
                Some(if edits_before_nested == 1 {
                    "  bc"
                } else {
                    "  abc"
                })
            );
        }
    }

    #[test]
    fn undo_inside_macro_swaps_the_pair_without_counting_as_another_edit() {
        for edits_before_undo in [1, 2] {
            let mut editor = editor();
            let mut state = VisualState::default();
            state.column = 2;
            editor.begin_macro_undo();
            for _ in 0..edits_before_undo {
                state.delete_character(&mut editor).unwrap();
            }
            state.undo(&mut editor).unwrap();
            assert_eq!(editor.buffer().line(0), Some("  abc"));
            state.replace_character(&mut editor, 'Z').unwrap();
            editor.end_macro_undo();
            state.undo(&mut editor).unwrap();
            assert_eq!(
                editor.buffer().line(0),
                Some(if edits_before_undo == 1 {
                    "  bc"
                } else {
                    "  c"
                })
            );
        }
    }

    #[test]
    fn line_restore_keeps_its_own_undo_and_preserves_an_existing_group() {
        for changes in [1, 2] {
            let mut editor = editor();
            let mut state = VisualState::default();
            state.column = 2;
            state.checkpoint_line(&editor);
            editor.begin_macro_undo();
            for _ in 0..changes {
                state.delete_character(&mut editor).unwrap();
            }
            state.restore_line(&mut editor).unwrap();
            assert_eq!(editor.buffer().line(0), Some("  abc"));
            // Inside the stream, u undoes U using the line snapshot.
            state.undo(&mut editor).unwrap();
            assert_eq!(
                editor.buffer().line(0),
                Some(if changes == 1 { "  bc" } else { "  c" })
            );
            state.replace_character(&mut editor, 'Z').unwrap();
            editor.end_macro_undo();
            // The second change promotes the swapped line pair; an already
            // grouped macro instead keeps its original full-buffer save.
            state.undo(&mut editor).unwrap();
            assert_eq!(editor.buffer().line(0), Some("  abc"));
        }
    }

    #[test]
    fn failed_edits_preserve_the_group_and_group_undo_invalidates_line_visit() {
        let mut editor = editor();
        let mut state = VisualState::default();
        state.column = 2;
        state.checkpoint_line(&editor);
        state.replace_character(&mut editor, 'Z').unwrap();
        editor.begin_macro_undo();
        state.replace_character(&mut editor, 'X').unwrap();
        state.replace_character(&mut editor, 'Y').unwrap();
        assert!(editor.delete_lines(99, 100).is_err());
        editor.end_macro_undo();
        state.undo(&mut editor).unwrap();
        state.restore_line(&mut editor).unwrap();
        assert_eq!(editor.buffer().line(0), Some("  Zbc"));
    }

    #[test]
    fn promotion_preserves_the_first_edits_line_undo_destination() {
        let mut editor = editor();
        let mut state = VisualState::default();
        state.row = 1;
        editor.set_current(1).unwrap();
        editor.begin_macro_undo();
        let mut input = state.begin_counted_insert(&mut editor, 'O', 1).unwrap();
        input.paste("new").unwrap();
        input.commit(&mut state).unwrap();
        state.replace_character(&mut editor, 'X').unwrap();
        editor.end_macro_undo();
        state.undo(&mut editor).unwrap();
        assert_eq!(editor.buffer().to_text(), "  abc\n  def\n  ghi\n");
        assert_eq!((state.row, state.column), (0, 2));
    }

    #[test]
    fn saving_keeps_group_and_reload_discards_old_file_history() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("file");
        std::fs::write(&path, "abc\n").unwrap();
        let mut editor = Editor::default();
        editor.load(&path).unwrap();
        editor.begin_macro_undo();
        editor.replace_lines(0, 0, ["Xbc"]).unwrap();
        editor.save().unwrap();
        editor.replace_lines(0, 0, ["XYc"]).unwrap();
        editor.end_macro_undo();
        editor.undo().unwrap();
        assert_eq!(editor.buffer().to_text(), "abc\n");
        editor.begin_macro_undo();
        editor.replace_lines(0, 0, ["old"]).unwrap();
        editor.load_with_force(&path, true).unwrap();
        assert!(editor.undo().is_err());
        editor.replace_lines(0, 0, ["new"]).unwrap();
        editor.replace_lines(0, 0, ["newer"]).unwrap();
        editor.end_macro_undo();
        editor.undo().unwrap();
        assert_eq!(editor.buffer().to_text(), "Xbc\n");
    }
}
