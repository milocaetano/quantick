//! Binding note sizes to the pane zoom before edits and placement.
use crate::{Drawings, TextPayload};

#[cfg(test)]
#[path = "text_zoom_tests.rs"]
mod tests;

impl<T> Drawings<T> {
    /// Supply the current chart zoom before placement/editing and each frame.
    /// Legacy notes bind once, preserving their current appearance. Navigation
    /// alone changes no persisted state and creates no undo step.
    pub fn sync_text_zoom(&mut self, px_per_bar: f32) {
        if !px_per_bar.is_finite() || px_per_bar <= 0.0 {
            return;
        }
        self.text_px_per_bar = px_per_bar;
        let mut changed = false;
        for drawing in self.items.iter_mut().chain(self.draft.iter_mut()) {
            if let Some(note) = drawing.payload.as_any_mut().downcast_mut::<TextPayload>() {
                changed |= note.zoom.bind(note.size_px, px_per_bar);
            }
        }
        self.revision += u64::from(changed);
    }
}
