//! Text-only visual undo placement attached to the reversible buffer pair.
use crate::buffer::Position;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UndoLocation {
    Character(Position),
    /// Cross-line d/c preserve the range column only for the first undo.
    InitialCharacter(Position),
    Lines {
        next: usize,
        opposite: usize,
    },
}

impl UndoLocation {
    pub(super) fn reverse(&mut self) {
        match self {
            Self::Lines { next, opposite } => std::mem::swap(next, opposite),
            Self::InitialCharacter(p) => {
                *self = Self::Lines {
                    next: p.row,
                    opposite: p.row,
                };
            }
            Self::Character(_) => {}
        }
    }
}
