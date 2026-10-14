//! Expire current-preview eligibility when a pane consumes a cached publication.

use std::sync::Arc;

use crate::engine::VisibleOrderflow;
use crate::projection::BOOK_CARRY_MAX_MS;

/// Cache the expired read model, so delayed publication copies it only once.
#[derive(Default)]
pub(crate) struct CachedDepthPreview {
    expired: Option<(Arc<VisibleOrderflow>, Arc<VisibleOrderflow>)>,
}

impl CachedDepthPreview {
    /// `now_ms` is the pane's market clock, anchored by market timestamps and
    /// advanced by monotonic elapsed time (or replay position), never wall time.
    /// Historical geometry and source identity survive expiry unchanged.
    pub(crate) fn consume(
        &mut self,
        published: Option<&Arc<VisibleOrderflow>>,
        now_ms: Option<i64>,
    ) -> Option<Arc<VisibleOrderflow>> {
        let published = published?;
        let Some(now_ms) = now_ms else {
            return Some(Arc::clone(published));
        };
        let stale = |cell: &crate::projection::HeatmapCell| {
            cell.current_book
                && cell.current_book_source.is_none_or(|source| {
                    !(0..=BOOK_CARRY_MAX_MS).contains(&now_ms.saturating_sub(source.event_ms))
                })
        };
        // Check time again before reusing the cache: replay can move backwards.
        if !published.projection.cells.iter().any(stale) {
            self.expired = None;
            return Some(Arc::clone(published));
        }
        if let Some((input, output)) = &self.expired
            && Arc::ptr_eq(input, published)
            && input
                .projection
                .cells
                .iter()
                .zip(output.projection.cells.iter())
                .all(|(original, expired)| {
                    expired.current_book == (original.current_book && !stale(original))
                })
        {
            return Some(Arc::clone(output));
        }
        let mut frame = published.as_ref().clone();
        let projection = Arc::make_mut(&mut frame.projection);
        for cell in Arc::make_mut(&mut projection.cells) {
            if stale(cell) {
                cell.current_book = false;
            }
        }
        let output = Arc::new(frame);
        self.expired = Some((Arc::clone(published), Arc::clone(&output)));
        Some(output)
    }
}
