//! Binding note sizes to the pane zoom before edits and placement.
use crate::text_scale::TextSpacing;
use crate::{Drawing, Drawings, TextPayload};
use std::cell::Cell;

#[cfg(test)]
#[path = "text_zoom_tests.rs"]
mod tests;

/// Records the first display of legacy notes on their owning store, even
/// when another pane renders them at a different spacing.
#[derive(Debug, Default)]
pub struct NoteDisplay(Cell<u64>);

impl NoteDisplay {
    pub fn observe(&self, note: &TextPayload, spacing: TextSpacing) {
        if note.bind_zoom(spacing) {
            self.0.set(self.0.get() + 1);
        }
    }

    pub(crate) fn revision(&self) -> u64 {
        self.0.get()
    }
}

impl<T> Drawings<T> {
    /// Supply the operation's viewport, including edits to a hidden note.
    /// Only the time axis is known here; the value axis binds on display.
    pub fn set_text_spacing(&mut self, px_per_bar: f32) {
        self.text_spacing = TextSpacing::horizontal(px_per_bar);
    }

    pub fn note_display(&self) -> &NoteDisplay {
        &self.note_display
    }

    /// Consume the display effect on the object's store. The scene can then
    /// be reused for the next mark, including when a companion is painting it.
    pub fn observe_scene(
        &self,
        scene: &mut crate::MarkScene,
        drawing: &Drawing<T>,
        spacing: TextSpacing,
    ) {
        if std::mem::take(&mut scene.displayed_note)
            && let Some(note) = drawing.payload.as_any().downcast_ref::<TextPayload>()
        {
            self.note_display.observe(note, spacing);
        }
    }
}

impl TextPayload {
    fn bind_zoom(&self, spacing: TextSpacing) -> bool {
        let mut zoom = self.zoom.get();
        let changed = zoom.bind(self.size_px, spacing);
        if changed {
            self.zoom.set(zoom);
        }
        changed
    }
}

impl<T> Drawing<T> {
    pub(crate) fn bind_text_zoom(&mut self, spacing: TextSpacing) -> bool {
        self.payload
            .as_any_mut()
            .downcast_mut::<TextPayload>()
            .is_some_and(|note| note.bind_zoom(spacing))
    }

    pub(crate) fn bind_resized_text(&mut self, before: &Self, spacing: TextSpacing) {
        if let Some(old) = before.payload.as_any().downcast_ref::<TextPayload>()
            && let Some(note) = self.payload.as_any_mut().downcast_mut::<TextPayload>()
            && old.size_px != note.size_px
        {
            note.bind_zoom(spacing);
        }
    }

    /// A snapshot taken before a note was shown cannot undo the scales its
    /// first display bound. An actual size edit keeps its own reference.
    pub(crate) fn retain_legacy_text_zoom(&mut self, held: &Self) {
        if let Some(note) = self.payload.as_any_mut().downcast_mut::<TextPayload>()
            && !note.zoom.get().complete(note.size_px)
            && let Some(previous) = held.payload.as_any().downcast_ref::<TextPayload>()
            && note.size_px == previous.size_px
            && previous.zoom.get().bound_to(previous.size_px)
        {
            note.zoom.set(previous.zoom.get());
        }
    }
}
