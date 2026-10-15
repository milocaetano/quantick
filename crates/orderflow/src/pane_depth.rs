//! Whether a published current book is still current when a pane draws it.

use std::sync::Arc;

use crate::engine::VisibleOrderflow;
use crate::projection::book_is_current;

/// The pane's answer for the last publication whose book it no longer
/// holds current, kept so a stale publication is relabelled only once.
#[derive(Default)]
pub(crate) struct CachedDepthPreview {
    relabelled: Option<(Arc<VisibleOrderflow>, Arc<VisibleOrderflow>)>,
    /// The publication last consumed and the frame it was drawn as.
    drawn: Option<(Arc<VisibleOrderflow>, Arc<VisibleOrderflow>)>,
}

impl CachedDepthPreview {
    /// The frame to draw `published` as at `now_ms`: the publication itself
    /// while its book is current ([`book_is_current`]), else the same frame
    /// with [`VisibleOrderflow::current_book_live`] cleared. O(1): one check
    /// of the publication's one book source, and at most one shallow copy
    /// per publication that shares every cell, mark and gap.
    ///
    /// `now_ms` is the pane's market clock, anchored by market timestamps and
    /// advanced by monotonic elapsed time (or replay position), never wall
    /// time; without one the publication is drawn as published.
    pub(crate) fn consume(
        &mut self,
        published: Option<&Arc<VisibleOrderflow>>,
        now_ms: Option<i64>,
    ) -> Option<Arc<VisibleOrderflow>> {
        let Some(published) = published else {
            self.drawn = None;
            return None;
        };
        let output = self.relabel(published, now_ms);
        self.drawn = Some((Arc::clone(published), Arc::clone(&output)));
        Some(output)
    }

    /// `published` as the last [`consume`](Self::consume) drew it: its
    /// relabelled copy when that call relabelled this very publication, else
    /// the publication itself.
    pub(crate) fn drawn<'a>(
        &'a self,
        published: &'a Arc<VisibleOrderflow>,
    ) -> &'a Arc<VisibleOrderflow> {
        match &self.drawn {
            Some((input, output)) if Arc::ptr_eq(input, published) => output,
            _ => published,
        }
    }

    fn relabel(
        &mut self,
        published: &Arc<VisibleOrderflow>,
        now_ms: Option<i64>,
    ) -> Arc<VisibleOrderflow> {
        let live = match (published.projection.current_book, now_ms) {
            (Some(source), Some(now_ms)) => book_is_current(now_ms, source.event_ms),
            _ => published.current_book_live,
        };
        if live == published.current_book_live {
            return Arc::clone(published);
        }
        // Checked again on every frame: replay can move the clock backwards.
        if let Some((input, output)) = &self.relabelled
            && Arc::ptr_eq(input, published)
            && output.current_book_live == live
        {
            return Arc::clone(output);
        }
        let output = Arc::new(VisibleOrderflow {
            current_book_live: live,
            ..published.as_ref().clone()
        });
        self.relabelled = Some((Arc::clone(published), Arc::clone(&output)));
        output
    }
}
