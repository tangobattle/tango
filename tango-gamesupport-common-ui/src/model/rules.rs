//! UI-facing re-exports of the headless legality model, plus the one
//! editor-only slot-reordering helper.

pub use crate::dataview::build::{FolderSelections, FolderUsage, MAX_COPIES_PER_PART, MAX_PATCH_CARD56_MB};

/// New index of an element originally at `i` after an ordered move that takes
/// the element at `from` and reinserts it at `to`.
pub fn reorder_index(i: usize, from: usize, to: usize) -> usize {
    if i == from {
        to
    } else if from < to && i > from && i <= to {
        i - 1
    } else if from > to && i >= to && i < from {
        i + 1
    } else {
        i
    }
}
