//! Binding note sizes to the pane zoom before edits and placement.
use crate::{Drawing, Drawings, TextPayload};

#[cfg(test)]
#[path = "text_zoom_tests.rs"]
mod tests;

impl<T: Clone + PartialEq> Drawings<T> {
    /// Supply the operation's viewport, including edits to a hidden note.
    pub fn set_text_spacing(&mut self, px_per_bar: f32) {
        self.text_px_per_bar = px_per_bar;
    }

    /// Bind a loaded note at its first visible zoom. Ordinary frames only
    /// update the spacing used by subsequent placements and size edits.
    pub fn sync_text_zoom(&mut self, px_per_bar: f32) {
        self.set_text_spacing(px_per_bar);
        if !px_per_bar.is_finite()
            || px_per_bar <= 0.0
            || self.all_hidden
            || self.text_zoom_revision == Some(self.revision)
        {
            return;
        }
        let mut changed = false;
        for index in 0..self.items.len() {
            if self.is_visible(index) {
                changed |= self.items[index].bind_text_zoom(px_per_bar);
            }
        }
        self.revision += u64::from(changed);
        self.text_zoom_revision = Some(self.revision);
    }
}

impl<T> Drawing<T> {
    pub(crate) fn bind_text_zoom(&mut self, px_per_bar: f32) -> bool {
        self.payload
            .as_any_mut()
            .downcast_mut::<TextPayload>()
            .is_some_and(|note| note.zoom.bind(note.size_px, px_per_bar))
    }

    pub(crate) fn bind_resized_text(&mut self, before: &Self, px_per_bar: f32) {
        if let Some(old) = before.payload.as_any().downcast_ref::<TextPayload>()
            && let Some(note) = self.payload.as_any_mut().downcast_mut::<TextPayload>()
            && old.size_px != note.size_px
        {
            note.zoom.bind(note.size_px, px_per_bar);
        }
    }

    /// A snapshot taken before a legacy note was shown cannot undo its first
    /// visible spacing. An actual size edit keeps its own historical reference.
    pub(crate) fn retain_legacy_text_zoom(&mut self, held: &Self) {
        if let Some(note) = self.payload.as_any_mut().downcast_mut::<TextPayload>()
            && !note.zoom.valid()
            && let Some(previous) = held.payload.as_any().downcast_ref::<TextPayload>()
            && note.size_px == previous.size_px
        {
            note.zoom = previous.zoom;
        }
    }
}
