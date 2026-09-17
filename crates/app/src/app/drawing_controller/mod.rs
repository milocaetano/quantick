//! Window drawing ownership: commands, clipboard/editor chrome and presets.
//! The pane retains its drawing store and geometry; external actions return
//! synchronously to composition before this owner resumes ordinary commands.
use crate::drawings;
use crate::surfaces::drawing_chrome::DrawingChromeSurface;
use std::time::Instant;
mod access;
mod input;
mod view;
pub(crate) use access::{DrawingAccess, DrawingReadAccess};
#[cfg(test)]
pub(super) use input::DUPLICATE_OFFSET_BARS;

pub(crate) struct DrawingController {
    pub(crate) chrome: DrawingChromeSurface,
    pub(crate) presets: drawings::presets::PresetStore,
}
impl DrawingController {
    pub(crate) fn new() -> Self {
        Self {
            chrome: DrawingChromeSurface::default(),
            presets: drawings::presets::PresetStore::load_from(
                drawings::presets::PresetStore::default_path(),
            ),
        }
    }
}
/// Notices preserve operation order: an ordinary note following an Undo is
/// deferred by the toast owner. Seven chrome branches can announce in one
/// response; keyboard handling can announce at most three. No heap queue.
#[derive(Default)]
pub(crate) struct DrawingEffects {
    notices: [Option<(String, Instant, bool)>; 8],
    count: usize,
    pub(crate) inspector_moved: bool,
}
impl DrawingEffects {
    fn note(&mut self, text: impl Into<String>, now: Instant) {
        self.push(text.into(), now, false);
    }
    fn note_with_undo(&mut self, text: impl Into<String>, now: Instant) {
        self.push(text.into(), now, true);
    }
    fn push(&mut self, text: String, now: Instant, undo: bool) {
        self.notices[self.count] = Some((text, now, undo));
        self.count += 1;
    }
    pub(crate) fn apply_notice(self, toast: &mut crate::surfaces::toast::ToastSurface) {
        for (message, now, undo) in self.notices.into_iter().flatten() {
            if undo {
                toast.note_with_undo(message, now);
            } else {
                toast.note(message, now);
            }
        }
    }
}
