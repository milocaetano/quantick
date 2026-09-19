//! The tab lifecycle: opening a market, closing one, moving between them.
//!
//! `adopt_tab` — the step that actually builds a `Tab` from a live feed
//! handle — lives on the arrangement adapter, beside the constructor it
//! shares its inheritance rules with. What is here is everything that
//! decides *which* tab, and what happens to the window when the set of them
//! changes — plus the two owners the window mirrors onto every tab: the
//! history reach ([`HistorySettings`]) and the symbol catalog
//! ([`SymbolCatalog`]).

use eframe::egui;

// Read by `heatmap_lamp_on`, which is test-only, so the import is gated the
// same way rather than kept alive by a `chart_layers::` prefix on one line.
#[cfg(test)]
use crate::chart_layers::ChartLayer;
use crate::indicator_worker::SlotId;
use crate::symbols_file;
use crate::tabstrip::TabAction;
use quantick_feed::history_reach;

use crate::config::AppConfig;

use super::menu_bar::{
    CLOSE_TAB_SHORTCUT, NEW_TAB_SHORTCUT, NEXT_TAB_SHORTCUT, PREVIOUS_TAB_SHORTCUT,
};
use super::{QuantickApp, TabSlot};

/// The interval a saved bar rule names, when it is a time rule at all — a
/// workspace that recorded `tick:50` for a context chart is a file written by
/// hand, and the chart opens on the default rather than on a guess.
fn saved_time_interval(text: Option<&str>) -> Option<i64> {
    text.and_then(|text| {
        quantick_engine::bar_registry::BUILTIN_BARS
            .parse(text)
            .ok()?
            .time_interval_ms()
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

/// How much tape one press of *load older* reaches for, and how it is
/// fetched — a standing choice of the window, mirrored onto every tab by
/// [`QuantickApp::drain_tabs`], which is where a press is actually served.
///
/// Owned here because that mirroring is this module's job. The toolbar
/// edits it, the menu bar toggles two of it, `launch_hooks` seeds it from
/// the environment and `workspace_save` persists it.
pub(super) struct HistorySettings {
    /// Whether venue candle history is asked for in slices, newest first
    /// (View → progressive venue history).
    ///
    /// On by default. A span of one-minute candles is a run of sequential
    /// venue round trips — seconds for the opening week, and another such run
    /// for every span the trader reaches back through — and fetched whole the
    /// chart shows nothing at all for the whole of it. Off restores exactly
    /// that: one
    /// request, one reply, one very late frame — kept because a trader on a
    /// metered or rate-limited connection may prefer the smaller number of
    /// requests, and because a setting whose "off" is not the old behaviour is
    /// not a setting the user can fall back to.
    pub(super) progressive_history: bool,

    /// How far one press of the chart's *load older* button reaches — one
    /// page of trades, or back past the market's last close with a lead into
    /// the session before it.
    ///
    /// A standing choice of the window rather than of a market: a trader who
    /// wants to see yesterday wants it in the tab they open next too. Mirrored
    /// onto every tab each frame, which is where the press is actually served.
    pub(super) history_reach: history_reach::HistoryReach,

    /// Minutes of *traded* time one press of the `by time` reach pulls.
    ///
    /// On the window beside the reach it belongs to, and mirrored onto every
    /// tab by `drain_tabs`, exactly as the reach itself is: the two are one
    /// choice, and a tab opened after the trader set it must press the way
    /// they said. Seeded from `[history] reach_span_minutes` and editable
    /// afterwards, because it is the trader's own answer to "how much more
    /// tape per press" and that differs between a contract printing a million
    /// times a day and one printing a thousand.
    pub(super) history_reach_span_minutes: u32,

    /// Whether a chart *not* cut by time may carry the venue's own candles in
    /// front of its bars.
    ///
    /// Off by default: a tick chart has always opened on the prints this
    /// session saw, and nothing is put in front of them unasked. On, a chart
    /// cut by trades gets the venue's 1-minute candles as a labelled prefix —
    /// the only way such a chart can show yesterday at all, since a candle
    /// cannot be folded into a tick bar and must never pretend to be one.
    pub(super) venue_lead_in: bool,
}

impl QuantickApp {
    /// The slot a command from the chrome addresses: the active tab, its
    /// focused pane, that slot.
    pub(super) fn target_slot(&self, slot: SlotId) -> TabSlot {
        TabSlot {
            tab: self.tabs.active_id(),
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
        // Before the drain: a reading that arrives this frame lands on a
        // recorder built for the market it belongs to.
        super::deal_recording_wiring::ensure(self);
        let config = &self.config;
        let policy = self.history.policy();
        let mut trades = 0_u64;
        for (tab_id, tab) in self.tabs.iter_with_ids_mut() {
            let before = tab.live_trades;
            tab.drain_frame(tab_id, config, policy);
            trades += tab.live_trades - before;
        }
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
            let index = self.tabs.active_index();
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
                if index < self.tabs.len() {
                    self.tabs.select(index);
                }
            }
            TabAction::Close(index) => self.arrangement_adapter().close_tab(index),
            TabAction::New => self.surfaces.source_picker.open(&self.config),
        }
    }
}

impl HistorySettings {
    /// Choose how far one press of *load older* reaches.
    ///
    /// The named call behind the history menu's reach chips and the
    /// `QUANTICK_HISTORY_REACH` hook — one path, so an operator without a
    /// mouse sets what a click sets. Mirrored onto every tab by `drain_tabs`,
    /// where a run in flight also reads it: withdrawing the longer reach is
    /// how a trader calls that run off.
    pub(super) fn set_reach(&mut self, reach: history_reach::HistoryReach) {
        self.history_reach = reach;
    }

    /// How far back one press of the `by time` reach pulls, in minutes of
    /// traded time.
    ///
    /// Clamped rather than refused: a span of zero is a press that asks for
    /// nothing, and the operator that sent it meant *some* history. The
    /// ceiling is the campaign's own span cap, past which no run can reach
    /// anyway, so accepting a larger number would be promising a reach the
    /// budgets forbid.
    pub(super) fn set_span_minutes(&mut self, minutes: u32) {
        let ceiling = (history_reach::MAX_CAMPAIGN_SPAN_MS / 60_000) as u32;
        self.history_reach_span_minutes = minutes.clamp(1, ceiling);
    }

    /// The window's standing choice, phrased for the tabs. The switch lives
    /// on the window, the request is phrased by the tab: handed to every
    /// tab's drain each frame so every tab asks the way the trader last said,
    /// including one opened after the choice was made.
    pub(super) fn policy(&self) -> crate::tab::HistoryPolicy {
        crate::tab::HistoryPolicy {
            progressive: self.progressive_history,
            reach: self.history_reach,
            reach_span_minutes: self.history_reach_span_minutes,
            venue_lead_in: self.venue_lead_in,
        }
    }
}

/// The instruments the picker can add to a feed: the running config, the
/// user's own additions kept apart from it, and the sidecar they persist in.
///
/// The config file itself is never written: it is hand-written, comments
/// and all, and a program that rewrote it would eat them. An addition lives
/// in its own sidecar, which the next launch folds back in before the config
/// is validated (see [`crate::symbols_file`]).
pub(crate) struct SymbolCatalog<'a> {
    pub(super) config: &'a mut AppConfig,
    pub(super) added: &'a mut symbols_file::AddedSymbols,
    pub(super) path: &'a std::path::Path,
}

impl SymbolCatalog<'_> {
    /// Put `symbol` in feed `feed_id`'s catalog and remember it across
    /// restarts. Reports whether the catalog took it.
    pub(crate) fn add(&mut self, feed_id: &str, symbol: &str) -> Result<(), String> {
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
        *self.config = candidate;
        self.added.add(feed_id, symbol);
        if let Err(error) = symbols_file::save(self.path, self.added) {
            // The catalog took it for this session either way; what is lost is
            // the next launch, and the user is told which file did not take it.
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "SYMBOL_CATALOG_WRITE_FAILED",
                path = %self.path.display(),
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
            path = %self.path.display(),
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
    pub(crate) fn remove(&mut self, feed_id: &str, symbol: &str) {
        if !self.config.remove_symbol(feed_id, symbol) {
            return;
        }
        self.added.remove(feed_id, symbol);
        if let Err(error) = symbols_file::save(self.path, self.added) {
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "SYMBOL_CATALOG_WRITE_FAILED",
                path = %self.path.display(),
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
            path = %self.path.display(),
            action = "leave_open_tabs_alone",
            "a user-added symbol left the catalog"
        );
    }
}
