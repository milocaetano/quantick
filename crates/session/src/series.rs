//! A pane's two series and the venue prefix installed in front of them.
//!
//! The pane keeps owning its [`PaneSeries`]; this module decides what a
//! venue prefix does to it — the seam lead merged into the first engine bar,
//! the prefix admitted or not, and the view effects the window then applies
//! in order — so the window only carries those effects out.

use quantick_chart::state::ChartState;
use quantick_chart::venue_history::VenueHistory;
use quantick_chart_interaction::pane_history::{HistoryState, PrefixChange, PrefixInstallEffect};
use quantick_engine::Bar;

use crate::SeamTrace;

/// The bars one pane holds: the venue prefix in front, then the bars the
/// engine cut from prints.
pub struct PaneSeries {
    pub state: ChartState,
    /// Venue candles in front of the trade-derived series, folded to the
    /// pane's interval.
    ///
    /// Deliberately outside `ChartState`, which rebuilds its bars from
    /// retained trades on every spec change and would eat a prefix kept
    /// inside it on the first chip click.
    ///
    /// Non-empty on any pane cutting by a foldable time interval. A venue
    /// candle has no tape, so the flow layers draw nothing over it.
    pub history_prefix: VenueHistory,
}

/// What installing one venue prefix did, for the window to apply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrefixInstall {
    /// Whether the prefix or the seam lead changed.
    pub changed: bool,
    /// The view effects, in the order the window must apply them.
    pub effects: Vec<PrefixInstallEffect>,
}

impl PaneSeries {
    #[must_use]
    pub fn new(state: ChartState) -> Self {
        Self {
            state,
            history_prefix: VenueHistory::default(),
        }
    }

    /// The bar rule the engine cuts this series by, as a config spells it.
    #[must_use]
    pub fn bar_config(&self) -> String {
        self.state.spec().to_config_string()
    }

    /// Put `bars` in front of the engine's bars, the venue's seam `lead`
    /// merged into the first one (`ChartState::set_venue_lead`); `covers_seam`
    /// says nothing of that bar's bucket is left unaccounted for. A seam bar
    /// that changed, or says why it holds no lead, is logged beside pane
    /// `pane`.
    pub fn install_venue_prefix(
        &mut self,
        history: &mut HistoryState,
        pane: u64,
        bars: Vec<Bar>,
        lead: Option<Bar>,
        covers_seam: bool,
        seam: Option<SeamTrace>,
    ) -> PrefixInstall {
        let had = seam.and_then(|_| self.state.venue_lead().cloned());
        let lead_changed = self.state.set_venue_lead(lead, covers_seam);
        let change = self.history_prefix.install(bars, history, lead_changed);
        if let Some(seam) = seam {
            log_seam_lead(pane, seam, had.as_ref(), self.state.venue_lead());
        }
        PrefixInstall {
            changed: change != PrefixChange::Unchanged,
            effects: change.install_effects().collect(),
        }
    }
}

fn log_seam_lead(pane: u64, seam: SeamTrace, had: Option<&Bar>, seated: Option<&Bar>) {
    if had == seated && (seated.is_some() || seam.token == "nothing_before_first_trade") {
        return;
    }
    tracing::info!(
        target: "quantick::app",
        schema_version = 1_u8,
        event_code = "SEAM_LEAD",
        pane,
        interval_ms = seam.interval_ms,
        first_print_ms = seam.first_print_ms,
        lead_open_ms = seated.map_or(0, |bar| bar.open_time),
        lead_trades = seated.map_or(0, |bar| bar.trade_count),
        action = if seated.is_some() { "merged_into_seam_bar" } else { seam.token },
        "the venue's history of the seam bucket before the first trade"
    );
}
