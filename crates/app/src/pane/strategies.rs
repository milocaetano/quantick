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
use crate::state::dec_from_f64;

use super::{ChartPane, region_pause};

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

impl ChartPane {
    /// The closed bars awaiting strategy evaluation, slot each. Drained by
    /// the tab right after the ingestion sweep that queued them.
    #[must_use]
    pub fn take_strategy_bars(&mut self) -> Vec<(quantick_engine::Bar, usize)> {
        std::mem::take(&mut self.strategies.pending)
    }

    /// Resolve the drawing an instance is anchored to into the kernel's
    /// terms, for the bar that closed at `slot`: the price band between the
    /// rectangle's anchors, and whether that slot falls inside its span of
    /// the tape. `None` when the drawing is gone, marks another market, or
    /// lost its footing on this series — a region that cannot honestly be
    /// tested holds fire.
    #[must_use]
    pub fn strategy_region(
        &self,
        id: drawings::DrawingId,
        slot: usize,
    ) -> Option<(quantick_strategy::Region, bool)> {
        let index = self.drawings.index_of(id)?;
        let drawing = self.drawings.items().get(index)?;
        // Every reason a region cannot honestly be tested — another market,
        // a lost series, a drawing nobody can see — is one rule, shared with
        // the badge that has to say so ([`region_pause`]). An order fired
        // from a region nobody can see is an invisible bot; showing the
        // drawing resumes it.
        if region_pause(drawing, self.drawings.all_hidden()).is_some() {
            return None;
        }
        let [a, b] = drawing.points.as_slice() else {
            return None;
        };
        let region = quantick_strategy::Region::new(dec_from_f64(a.price), dec_from_f64(b.price));
        // An extended rectangle runs to the chart's right edge until
        // further notice, and its region does too — otherwise the bot
        // silently expires at the drawn end while the band visibly keeps
        // going (the replay trap this option exists to close).
        let extend_right = drawing
            .payload
            .as_any()
            .downcast_ref::<drawings::RectanglePayload>()
            .is_some_and(|payload| payload.extend_right);
        #[allow(clippy::cast_precision_loss)]
        let slot = slot as f32;
        let active = slot >= a.bar.min(b.bar) && (extend_right || slot <= a.bar.max(b.bar));
        Some((region, active))
    }
}
