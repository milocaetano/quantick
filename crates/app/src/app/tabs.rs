//! The tab lifecycle: opening a market, closing one, moving between them.
//!
//! `adopt_tab` — the step that actually builds a `Tab` from a live feed
//! handle — lives on the arrangement adapter, beside the constructor it
//! shares its inheritance rules with. What is here is everything that
//! decides *which* tab, and what happens to the window when the set of them
//! changes — plus the two owners the window mirrors onto every tab: the
//! history reach ([`HistorySettings`]) and the symbol catalog
//! ([`quantick_stores::symbol_catalog::SymbolCatalog`]).

use eframe::egui;

// Read by `heatmap_lamp_on`, which is test-only, so the import is gated the
// same way rather than kept alive by a `chart_layers::` prefix on one line.
#[cfg(test)]
use crate::chart_layers::ChartLayer;
use crate::indicator_worker::SlotId;
use crate::tabstrip::TabAction;

use super::menu_bar::{
    CLOSE_TAB_SHORTCUT, NEW_TAB_SHORTCUT, NEXT_TAB_SHORTCUT, PREVIOUS_TAB_SHORTCUT,
};
use super::{QuantickApp, TabSlot};

pub(super) fn saved_context_intervals(bars: &[String], time_bars: Option<&str>) -> Vec<i64> {
    quantick_stores::ui_state::saved_context_intervals(
        bars,
        time_bars,
        crate::time_header::DEFAULT_INTERVAL_MS,
    )
}

/// The window's history choices; see [`quantick_session::HistorySettings`].
pub(super) use quantick_session::HistorySettings;

impl QuantickApp {
    /// The slot a command from the chrome addresses: the active tab, its
    /// focused pane, that slot.
    pub(super) fn target_slot(&self, slot: SlotId) -> TabSlot {
        TabSlot {
            workspace: self.runtime.id(),
            pane: self.active_tab().focused_pane().id,
            tab: self.runtime.tabs.active_id(),
            side: self.active_tab().focused_side(),
            slot,
        }
    }

    /// Whether the toolbar's heatmap lamp is lit.
    ///
    /// The *switch*, not what capture lets through it — the same reading the
    /// layer file was taught in 848cba0, and for a sibling reason. A lamp lit
    /// from `depth_visible()` (`enabled && show_depth`) reports the heatmap off
    /// for as long as book capture is starting, and forever on a source with no
    /// book: the trader sees an unlit button, presses it, and switches the
    /// layer they wanted *off*. The button already has an honest way to say a
    /// source cannot fill it — `.enabled(...)` carrying its
    /// `disabled_explanation` — so the lamp beside it answers the only other
    /// question there is.
    ///
    /// A named reading rather than an expression inside the toolbar's own
    /// frame, so the rule can be asserted without painting a toolbar. What
    /// reads it back without looking at the screen is the semantic scene,
    /// which takes the same `Tab::layer_toggle_state` this delegates to.
    #[must_use]
    #[cfg(test)]
    pub(super) fn heatmap_lamp_on(&self) -> bool {
        // Through the group's one reading, so this named rule and the lamp the
        // toolbar actually paints cannot become two answers to one question.
        // `#[cfg(test)]` because the toolbar now takes the group's reading
        // directly: keeping a second production entry point to the same answer
        // is how the two drift.
        self.active_tab()
            .layer_toggle_state(
                ChartLayer::Heatmap,
                &self.runtime.style,
                self.active_tab().capabilities(&self.config),
            )
            .0
    }

    /// Every tab takes in what its feed sent this frame, on screen or not.
    ///
    /// §11: switching tabs never tears a feed down, so a background tab has to
    /// keep draining — its channels are bounded, and one left full backs its
    /// feed thread up until the market it is showing is hours behind. The
    /// indicator workers are fed on the same pass, so a tab brought forward is
    /// already current rather than rebuilding on the frame it appears.
    pub(super) fn drain_tabs(&mut self) {
        // Before the drain: a reading that arrives this frame lands on a
        // recorder built for the market it belongs to.
        super::deal_recording_wiring::ensure(self);
        let trades = self.runtime.drain_tabs(&self.config);
        // What the window ingested, across every market it is holding.
        self.health.trades_since_summary += trades;
    }

    /// Tab shortcuts (§10): `Ctrl+T` new, `Ctrl+W` close, `Ctrl+Tab` cycle.
    pub(super) fn handle_tab_keys(&mut self, ctx: &egui::Context) {
        // Focus-gated like `handle_drawing_keys` (audit MINOR-13): typing in
        // the source picker's field with Ctrl held must never close the tab
        // under it — closing is instant and currently irreversible.
        if ctx.memory(|memory| memory.focused().is_some()) {
            return;
        }
        let (new_tab, close_tab, next, previous) = ctx.input_mut(|input| {
            (
                input.consume_shortcut(&NEW_TAB_SHORTCUT),
                input.consume_shortcut(&CLOSE_TAB_SHORTCUT),
                input.consume_shortcut(&NEXT_TAB_SHORTCUT),
                input.consume_shortcut(&PREVIOUS_TAB_SHORTCUT),
            )
        });
        if new_tab {
            self.surfaces.source_picker.open(&self.config);
        }
        if close_tab {
            let index = self.runtime.tabs.active_index();
            self.arrangement_adapter().close_tab(index);
        }
        if next {
            self.arrangement_adapter().cycle_tab(1);
        }
        if previous {
            self.arrangement_adapter().cycle_tab(-1);
        }
    }

    /// Do what the "Open market" dialog settled on.
    pub(super) fn apply_market_request(&mut self, request: crate::surfaces::MarketRequest) {
        use crate::surfaces::MarketRequest;
        match request {
            MarketRequest::Open { feed_id, symbol } => {
                self.arrangement_adapter().open_tab(feed_id, symbol, None)
            }
            MarketRequest::Add { feed_id, symbol } => {
                match self.symbol_catalog().add(&feed_id, &symbol) {
                    Ok(()) => {
                        self.surfaces.source_picker.close();
                        self.arrangement_adapter().open_tab(feed_id, symbol, None);
                    }
                    // The dialog stays open carrying the reason: the user is one
                    // keystroke from a symbol that does fit, and closing would
                    // make the refusal look like a crash.
                    Err(reason) => self.surfaces.source_picker.refuse(reason),
                }
            }
            MarketRequest::Remove { feed_id, symbol } => {
                self.symbol_catalog().remove(&feed_id, &symbol);
            }
        }
    }

    /// Carry out what the tab strip asked for.
    pub(super) fn apply_tab_action(&mut self, action: TabAction) {
        match action {
            TabAction::Activate(index) => {
                if index < self.runtime.tabs.len() {
                    self.runtime.tabs.select(index);
                }
            }
            TabAction::Close(index) => self.arrangement_adapter().close_tab(index),
            TabAction::New => self.surfaces.source_picker.open(&self.config),
        }
    }
}
