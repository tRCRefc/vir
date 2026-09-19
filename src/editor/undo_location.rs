//! Text-only visual undo placement attached to the reversible buffer pair.
use crate::buffer::Position;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UndoLocation {
    Character(Position),
    Lines { next: usize, opposite: usize },
}

impl UndoLocation {
    pub(super) fn reverse(&mut self) {
        if let Self::Lines { next, opposite } = self {
            std::mem::swap(next, opposite);
        }
    }
}
