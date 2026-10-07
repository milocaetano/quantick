//! UI-side indicator state: the columns the renderer reads.
//!
//! The worker owns the truth (the [`IndicatorHost`]); this module owns the
//! UI's *copy* of it, kept in sync by applying the session's
//! [`IndicatorEvent`] deltas each frame — the same shape as the `FeedEvent`
//! pattern. No locks anywhere near the render path: the renderer reads plain
//! vectors this struct owns, and none of it names a renderer, so the copy is
//! unit-tested here without a display.
//!
//! [`IndicatorHost`]: quantick_indicators::IndicatorHost
//! [`IndicatorEvent`]: quantick_indicator_session::IndicatorEvent

// One instance in `view`, the collection and its delta handling in `views`,
// the height a pane asks for in `pane_sizing`. The leaves are private: the
// re-exports below are this module's address.
mod pane_sizing;
mod view;
mod views;

pub use pane_sizing::{COLLAPSED_PANE_HEIGHT_PX, MIN_PANE_HEIGHT_PX, PaneSizing};
pub use view::{IndicatorView, MAX_PANES, PANE_HEIGHT_FRAC};
pub use views::IndicatorViews;

#[cfg(test)]
#[path = "indicator_views_tests.rs"]
mod tests;
