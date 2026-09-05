//! The strategies armed on this pane's drawings, and the two queues the pane
//! owes the tab around it.
//!
//! A pane anchors and paints; the kernel (`quantick-strategy`) judges. That
//! division is why three of these four fields are queues rather than state:
//! the pane sees the closed bar and the click, but cannot reach the simulator
//! or the dialog from inside its own input pass, so it parks what it saw and
//! the tab drains it on the same frame.
//!
//! Grouping them says which four fields the tab has to drain together. Each
//! used to carry a `strategy_` prefix on [`super::ChartPane`] saying so; the
//! prefix is this struct's name now.

use crate::drawings;

/// The armed strategies and the work they park for the tab. See the module
/// docs.
#[derive(Default)]
pub struct PaneStrategies {
    /// Armed strategy instances riding this pane's drawings. The kernel
    /// (`quantick-strategy`) judges; this pane only anchors and paints.
    pub anchors: crate::strategy_anchors::StrategyAnchors,
    /// Closed bars awaiting strategy evaluation, each with the slot it
    /// closed at. Pushed by `ingest_live_trade` only while instances
    /// exist, drained by the tab in the same ingestion sweep — the slot
    /// and the drawings' anchors are therefore read against one cut of
    /// the series.
    pub(super) pending: Vec<(quantick_engine::Bar, usize)>,
    /// The drawing whose "Add strategy…" was clicked; the app drains it
    /// and opens the arming dialog over this pane.
    pub(crate) popup_request: Option<drawings::DrawingId>,
    /// Simulator commands the drawing menu owes the paper host — cancelling
    /// a resting retest limit on disarm/removal. The pane cannot reach the
    /// tab's simulator from inside the menu, so the tab drains this on the
    /// same frame ([`crate::tab::TabState::apply_strategy_cleanup`]).
    pub(super) cleanup: Vec<quantick_sim::Command>,
}
