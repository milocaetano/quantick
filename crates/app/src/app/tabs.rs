//! The tab lifecycle: opening a market, closing one, moving between them.
//!
//! `adopt_tab` — the step that actually builds a `Tab` from a live feed
//! handle — stays in `super`, beside the constructor it shares its
//! inheritance rules with. What is here is everything that decides *which*
//! tab, and what happens to the window when the set of them changes.

use eframe::egui;

// Read by `heatmap_lamp_on`, which is test-only, so the import is gated the
// same way rather than kept alive by a `chart_layers::` prefix on one line.
#[cfg(test)]
use crate::chart_layers::ChartLayer;
use crate::indicator_worker::SlotId;
use crate::state::BarSpec;
use crate::symbols_file;
use crate::tabstrip::TabAction;

use quantick_feed as feed;

use super::menu_bar::{
    CLOSE_TAB_SHORTCUT, NEW_TAB_SHORTCUT, NEXT_TAB_SHORTCUT, PREVIOUS_TAB_SHORTCUT,
};
use super::{QuantickApp, TabSlot};

/// The interval a saved bar rule names, when it is a time rule at all — a
/// workspace that recorded `tick:50` for a context chart is a file written by
/// hand, and the chart opens on the default rather than on a guess.
fn saved_time_interval(text: Option<&str>) -> Option<i64> {
    text.and_then(|text| match BarSpec::parse(text) {
        Ok(BarSpec::Time(ms)) => Some(ms),
        _ => None,
    })
}

/// Every context chart's opening interval, top to bottom, from the rules a
/// workspace saved. A rule that is not a time rule keeps the default for its
/// slot so the slots after it still line up with their charts. A file written
/// before the stack existed carries only `time_bars`, which is the top chart's.
pub(super) fn saved_context_intervals(bars: &[String], time_bars: Option<&str>) -> Vec<i64> {
    if bars.is_empty() {
        return saved_time_interval(time_bars).into_iter().collect();
    }
    bars.iter()
        .map(|text| {
            saved_time_interval(Some(text)).unwrap_or(crate::time_header::DEFAULT_INTERVAL_MS)
        })
        .collect()
}

impl QuantickApp {
    /// The slot a command from the chrome addresses: the active tab, its
    /// focused pane, that slot.
    pub(super) fn target_slot(&self, slot: SlotId) -> TabSlot {
        TabSlot {
            tab: self.active_tab().id,
            side: self.active_tab().focused_side(),
            slot,
        }
    }

    /// Open `feed_id`/`symbol` in a new tab and make it active.
    ///
    /// Opening a market a tab already holds is allowed — two views of one
    /// book are a legitimate thing to want. For MetaTrader that means two
    /// listeners on one port, and the second one loses the bind: that tab
    /// shows the bridge's own bind-failure notice, which is the honest answer
    /// and the reason `[metatrader.ports]` maps a port per symbol.
    pub(super) fn open_tab(&mut self, feed_id: String, symbol: String, spec: Option<BarSpec>) {
        let Some(provider) = self.config.provider_of(&feed_id) else {
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "TAB_OPEN_UNKNOWN_FEED",
                feed = %feed_id,
                action = "ignore_request",
                "asked to open a feed the config does not have"
            );
            return;
        };
        // One feed per tab, resolved per symbol: a MetaTrader tab binds the
        // port `[metatrader.ports]` maps its symbol to (`endpoint_for`), so two
        // MT5 tabs on different symbols listen on different ports and each
        // finds its own bridge. Two tabs on the *same* MT5 symbol is allowed
        // and means one port for two listeners: the second loses the bind and
        // shows the feed's own MT5_BIND_FAILED notice, which is the honest
        // answer rather than a silently dead chart.
        let handle = feed::spawn_live(
            provider,
            &symbol,
            &self.config.metatrader,
            crate::paper_home::shelf_dir(),
        );
        self.adopt_tab(feed_id, symbol, handle, spec);
    }

    /// Close the tab at `index`, activating a neighbour.
    ///
    /// The last tab stays: a window with no market has nothing to draw. What
    /// the closed tab owned goes with it — dropping its `FeedHandle` closes
    /// the receivers its feed thread sends into, and dropping its panes drops
    /// the indicator worker and book worker handles, whose run loops end when
    /// their command channels disconnect. No joins, no shutdown protocol.
    pub(super) fn close_tab(&mut self, index: usize) {
        if self.tabs.len() <= 1 || index >= self.tabs.len() {
            return;
        }
        let mut closed = self.tabs.remove(index);
        // The tab's session ends here. Everything else it owns can simply be
        // dropped — the feed thread and the workers stop when their channels
        // go — but a simulated position is state the user created, and the
        // paper-trading contract says it ends in a labeled, journaled flatten,
        // never by vanishing with its window.
        closed.close();
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "TAB_CLOSED",
            tab = closed.id,
            feed = %closed.feed_id,
            symbol = %closed.symbol,
            tabs = self.tabs.len(),
            action = "drop_feed_and_workers",
            "closing a market tab"
        );
        // Its slots are gone with its panes; the bookkeeping must not outlive
        // them or a later tab reusing a slot number would inherit its kind.
        self.slot_kinds.retain(|(owner, _)| owner.tab != closed.id);
        self.operator_slots.retain(|owner| owner.tab != closed.id);
        self.script_files
            .retain(|(owner, ..)| owner.tab != closed.id);
        self.pending_hidden.retain(|owner| owner.tab != closed.id);
        self.pending_styles
            .retain(|(owner, _)| owner.tab != closed.id);
        self.active_tab = self.active_tab.min(self.tabs.len() - 1);
        drop(closed);
    }

    /// Move `delta` tabs along the strip, wrapping (§10: Ctrl+Tab).
    pub(super) fn cycle_tab(&mut self, delta: isize) {
        if self.tabs.len() < 2 {
            return;
        }
        let count = self.tabs.len() as isize;
        let next = (self.active_tab as isize + delta).rem_euclid(count);
        self.active_tab = next as usize;
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
                &self.style,
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
        let config = &self.config;
        let progressive_history = self.progressive_history;
        let history_reach = self.history_reach;
        let history_reach_span_minutes = self.history_reach_span_minutes;
        let venue_lead_in = self.venue_lead_in;
        let mut trades = 0_u64;
        for tab in &mut self.tabs {
            let before = tab.live_trades;
            tab.drain_feed();
            for pane in tab.panes_mut() {
                pane.apply_indicator_events();
            }
            tab.drain_book_feed();
            tab.drain_notices();
            // Heartbeat for the recorder. The lifecycle calls elsewhere already
            // start it at every point that knows the market changed; this one
            // makes "always recording" true by construction, so a start command
            // lost to a momentarily full channel heals on the next frame
            // instead of leaving the session silently unrecorded. Free while it
            // is running: one bool read and an early return.
            tab.ensure_book_capture(config);
            // MetaTrader narrows its capabilities when the bridge says hello,
            // after the pane may already have asked and been told there was
            // nothing held. Watching the edge is what asks again once the
            // answer can be a real one.
            // The switch lives on the window, the request is phrased by the
            // tab: mirrored here so every tab asks the way the trader last
            // said, including one opened after the choice was made.
            tab.progressive_history = progressive_history;
            tab.history_reach = history_reach;
            tab.history_reach_span_minutes = history_reach_span_minutes;
            // Through the setter, not the field: flipping the lead-in refolds
            // the prefix, and a tab that only had the field written would keep
            // drawing the answer to the previous choice until the next candle
            // landed. Idempotent, so the steady state costs one comparison.
            tab.set_venue_lead_in(venue_lead_in);
            tab.poll_ohlcv_capability(config);
            trades += tab.live_trades - before;
        }
        // What the window ingested, across every market it is holding.
        self.trades_since_summary += trades;
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
            self.close_tab(self.active_tab);
        }
        if next {
            self.cycle_tab(1);
        }
        if previous {
            self.cycle_tab(-1);
        }
    }

    /// Do what the "Open market" dialog settled on.
    pub(super) fn apply_market_request(&mut self, request: crate::surfaces::MarketRequest) {
        use crate::surfaces::MarketRequest;
        match request {
            MarketRequest::Open { feed_id, symbol } => self.open_tab(feed_id, symbol, None),
            MarketRequest::Add { feed_id, symbol } => match self.add_symbol(&feed_id, &symbol) {
                Ok(()) => {
                    self.surfaces.source_picker.close();
                    self.open_tab(feed_id, symbol, None);
                }
                // The dialog stays open carrying the reason: the user is one
                // keystroke from a symbol that does fit, and closing would
                // make the refusal look like a crash.
                Err(reason) => self.surfaces.source_picker.refuse(reason),
            },
            MarketRequest::Remove { feed_id, symbol } => self.remove_symbol(&feed_id, &symbol),
        }
    }

    /// Put `symbol` in feed `feed_id`'s catalog and remember it across
    /// restarts. Reports whether the catalog took it.
    ///
    /// The config file itself is never written: it is hand-written, comments
    /// and all, and a program that rewrote it would eat them. The addition
    /// lives in its own sidecar, which the next launch folds back in before
    /// the config is validated (see [`crate::symbols_file`]).
    pub(super) fn add_symbol(&mut self, feed_id: &str, symbol: &str) -> Result<(), String> {
        // Against the *whole* config, on a copy. A symbol is not just a name
        // in a list: it takes part in every cross-check the config has, and
        // the MetaTrader port map is one where a single mapped symbol offered
        // by two feeds is a configuration the app refuses to load. Persisting
        // one of those would write a file that kills the next launch — and the
        // error would name the config, which is not the file that broke.
        let mut candidate = self.config.clone();
        if !candidate.add_symbol(feed_id, symbol) {
            return Err(format!(
                "{} already offers {symbol}",
                self.config.feed_name(feed_id)
            ));
        }
        candidate.validate()?;
        self.config = candidate;
        self.added_symbols.add(feed_id, symbol);
        if let Err(error) = symbols_file::save(self.workspace.symbols_path(), &self.added_symbols) {
            // The catalog took it for this session either way; what is lost is
            // the next launch, and the user is told which file did not take it.
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "SYMBOL_CATALOG_WRITE_FAILED",
                path = %self.workspace.symbols_path().display(),
                error = %error,
                action = "addition_is_session_only",
                "cannot write the added-symbols file"
            );
        }
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "SYMBOL_ADDED",
            feed = %feed_id,
            symbol = %symbol,
            path = %self.workspace.symbols_path().display(),
            action = "open_in_new_tab",
            "a symbol was added from the source picker"
        );
        Ok(())
    }

    /// Take a user-added `symbol` back out of feed `feed_id`'s catalog.
    ///
    /// Only ever a catalog edit: a tab already showing that market keeps
    /// streaming it. The picker will not offer this for a market a tab is on,
    /// which is what stops the selection correction from retargeting it.
    pub(super) fn remove_symbol(&mut self, feed_id: &str, symbol: &str) {
        if !self.config.remove_symbol(feed_id, symbol) {
            return;
        }
        self.added_symbols.remove(feed_id, symbol);
        if let Err(error) = symbols_file::save(self.workspace.symbols_path(), &self.added_symbols) {
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "SYMBOL_CATALOG_WRITE_FAILED",
                path = %self.workspace.symbols_path().display(),
                error = %error,
                action = "removal_is_session_only",
                "cannot write the added-symbols file"
            );
        }
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "SYMBOL_REMOVED",
            feed = %feed_id,
            symbol = %symbol,
            path = %self.workspace.symbols_path().display(),
            action = "leave_open_tabs_alone",
            "a user-added symbol left the catalog"
        );
    }

    /// Carry out what the tab strip asked for.
    pub(super) fn apply_tab_action(&mut self, action: TabAction) {
        match action {
            TabAction::Activate(index) => {
                if index < self.tabs.len() {
                    self.active_tab = index;
                }
            }
            TabAction::Close(index) => self.close_tab(index),
            TabAction::New => self.surfaces.source_picker.open(&self.config),
        }
    }
}
