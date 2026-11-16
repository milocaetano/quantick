//! Running the armed strategy instances against the bars that just closed.
//!
//! The bridge between a print landing and a paper account hearing about it:
//! the tab's [`TabStrategies`](crate::strategy_anchors::TabStrategies) judges,
//! and this lends it what only the panes know — which drawings still exist,
//! the region each marks, the bar still forming. Deliberately cheap when
//! nothing is armed — the common case, and the one a per-trade path must not
//! start charging for.

use smallvec::SmallVec;

use crate::canvas_layout::MAX_CANVAS_PANES;
use crate::pane::ChartPane;
use quantick_session::strategies::{Forming, Print};

/// Evaluate the panes against the print the paper account has already consumed.
/// The supplied clock only governs alarms; commands still acknowledge immediately.
pub(super) fn evaluate(
    paper: &mut crate::paper_trading::PaperTrading,
    strategies: &mut crate::strategy_anchors::TabStrategies,
    (flow_pane, time_panes): (&ChartPane, &[ChartPane]),
    pending_sounds: &mut Vec<crate::audio::Cue>,
    now_ms: i64,
) {
    let now_ms = u64::try_from(now_ms).unwrap_or(0);
    let events = paper.account_mut().drain_bot_events();
    // Flow first, then the context stack: a strategy armed on the second
    // stacked chart has to trade and ring like one armed on the first.
    let panes: SmallVec<[&ChartPane; MAX_CANVAS_PANES]> =
        std::iter::once(flow_pane).chain(time_panes).collect();
    let ids: SmallVec<[u64; MAX_CANVAS_PANES]> = panes.iter().map(|pane| pane.id).collect();
    let pane = |id: u64| panes.iter().copied().find(|pane| pane.id == id);
    let answer = strategies.evaluate(
        &ids,
        &mut **paper,
        Print {
            events: &events,
            now_ms,
        },
        |id, drawing| pane(id).is_some_and(|pane| pane.drawings.index_of(drawing).is_some()),
        |id, drawing, slot| {
            pane(id).and_then(|pane| {
                crate::pane::strategies::drawing_region(&pane.drawings, drawing, slot)
            })
        },
        |id| {
            let pane = pane(id)?;
            Some(Forming {
                bar: pane.series.state.partial().cloned()?,
                progress: (pane.series.state.bar_progress_at(None))
                    .and_then(quantick_chart::bar_progress::BarProgressReading::fixed_progress),
                slot: pane.closed_slots(),
            })
        },
    );
    paper.account_mut().set_bot_listening(answer.watching > 0);
    pending_sounds.extend(answer.alarms);
}

#[cfg(test)]
impl super::Tab {
    /// The instances riding the pane on `side`; the pane must have held one.
    pub(crate) fn anchors_on(
        &self,
        side: crate::pane::PaneSide,
    ) -> &crate::strategy_anchors::StrategyAnchors {
        (self.strategies.anchors(self.pane(side).id)).expect("the pane has held a strategy")
    }

    /// The strategies of the pane on `side`.
    pub(crate) fn strategies_on(
        &mut self,
        side: crate::pane::PaneSide,
    ) -> &mut crate::strategy_anchors::PaneStrategies {
        let id = self.pane(side).id;
        self.strategies.pane_mut(id)
    }
}
