//! Copy insertion for chart drawings.
//!
//! Keyboard paste and the older Duplicate action share the normalization in
//! this module, so a chart never grows two subtly different kinds of copy.

use super::{Drawing, DrawingId, Drawings};

impl Drawings {
    /// Paste a drawing snapshot as one undo entry and return its fresh id.
    ///
    /// The snapshot need not come from this store. That lets one window-level
    /// clipboard cross panes and tabs without remembering a Vec index that
    /// can change under deletes or z-order edits.
    #[must_use]
    pub fn paste(&mut self, drawing: &Drawing, offset_bars: f32) -> DrawingId {
        let before = self.snapshot();
        let mut copy = drawing.clone();
        // A copy is a new object: its own identity, and never the original's
        // name, because two objects answering to one trader-authored name
        // would make every reference ambiguous.
        copy.id = self.alloc_id();
        copy.name = None;
        for point in &mut copy.points {
            point.bar += offset_bars;
            // Keeping the source's market instants would snap the offset copy
            // back onto the original after a re-cut, seek, or reconnect.
            point.time_ms = None;
        }
        copy.locked = false;
        let id = copy.id;
        self.items.push(copy);
        self.selected = Some(self.items.len() - 1);
        self.record(before);
        id
    }
}
