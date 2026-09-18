//! Arrangement shell: named runtime, document and chrome ports, never the app root.
use super::ArrangementHost;
use crate::config::AppConfig;
use crate::state::BarConfiguration;
use crate::tab::Tab;
use crate::timezone::TzOffset;
use crate::ui_state::{self, SavedFocusExt};
use quantick_feed::{self as feed, FeedHandle, history_reach};

pub(crate) struct ArrangementAdapter<'a> {
    pub(super) tabs: &'a mut ArrangementHost,
    pub(super) config: &'a AppConfig,
    pub(super) style: &'a crate::style::ChartStyle,
    pub(super) pane_ids: &'a mut crate::canvas_layout::PaneIdAllocator,
    pub(super) workspace: &'a mut crate::workspace_store::WorkspaceStore,
    pub(super) indicators: &'a mut super::indicator_manager::IndicatorState,
    #[cfg(any(feature = "scenario-harness", test))]
    pub(super) harness: &'a crate::harness::Harness,
    pub(super) toolrail: &'a mut crate::toolrail::ToolRail,
    pub(super) tz: &'a mut TzOffset,
    pub(super) dock: &'a mut crate::dock::Dock,
    pub(super) show_perf: &'a mut bool,
    pub(super) record_deals: &'a mut Option<bool>,
    pub(super) history: &'a mut super::tabs::HistorySettings,
    pub(super) drawing_chrome: &'a mut crate::surfaces::DrawingChromeSurface,
    pub(super) toast: &'a mut crate::surfaces::ToastSurface,
}

#[derive(Clone, Copy)]
pub(crate) struct ArrangementRead<'a> {
    pub(super) tabs: &'a ArrangementHost,
    pub(super) config: &'a AppConfig,
    pub(super) toolrail: &'a crate::toolrail::ToolRail,
    pub(super) tz: &'a TzOffset,
    pub(super) dock: &'a crate::dock::Dock,
    pub(super) show_perf: bool,
    pub(super) record_deals: Option<bool>,
    pub(super) history: &'a super::tabs::HistorySettings,
    pub(super) drawing_chrome: &'a crate::surfaces::DrawingChromeSurface,
}

impl ArrangementAdapter<'_> {
    /// Open `feed_id`/`symbol` in a new tab and make it active.
    ///
    /// Opening a market a tab already holds is allowed — two views of one
    /// book are a legitimate thing to want. For MetaTrader that means two
    /// listeners on one port, and the second one loses the bind: that tab
    /// shows the bridge's own bind-failure notice, which is the honest answer
    /// and the reason `[metatrader.ports]` maps a port per symbol.
    pub(super) fn open_tab(
        &mut self,
        feed_id: String,
        symbol: String,
        spec: Option<BarConfiguration>,
    ) {
        self.open_with_plan(feed_id, symbol, spec, None);
    }
    pub(super) fn open_with_plan(
        &mut self,
        feed_id: String,
        symbol: String,
        spec: Option<BarConfiguration>,
        opening: Option<quantick_workspace::arrangement::Transition>,
    ) {
        if let Some(plan) = &opening {
            self.tabs
                .validate_transition(plan)
                .expect("current opening effect before I/O");
        }

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
        match opening {
            Some(plan) => self.adopt_planned_tab(feed_id, symbol, handle, spec, plan),
            None => self.adopt_tab(feed_id, symbol, handle, spec),
        }
    }
    /// Take a market that is already streaming as a new tab, and make it the
    /// active one.
    ///
    /// The bar spec is inherited from the tab you were on: opening a second
    /// market to compare it against the first is the reason to do this, and
    /// landing on a different aggregation would defeat that. A feed that
    /// declares its own `default_bars`/`default_layout` overrides the
    /// inheritance — the declaration exists because that market reads
    /// differently, which is exactly when inheriting would mislead.
    /// `spec` overrides both, and exists for the one caller that already knows
    /// the answer: a workspace restoring the bar rule this market was last
    /// read on. Inheriting there would quietly discard what the user saved.
    pub(super) fn adopt_tab(
        &mut self,
        feed_id: String,
        symbol: String,
        feed: FeedHandle,
        spec: Option<BarConfiguration>,
    ) {
        let opening = self.tabs.plan_open();
        self.adopt_planned_tab(feed_id, symbol, feed, spec, opening);
    }
    fn adopt_planned_tab(
        &mut self,
        feed_id: String,
        symbol: String,
        feed: FeedHandle,
        spec: Option<BarConfiguration>,
        opening: quantick_workspace::arrangement::Transition,
    ) {
        self.tabs
            .validate_transition(&opening)
            .expect("current runtime construction");
        let quantick_workspace::arrangement::Effect::Append { id } = opening.effect() else {
            unreachable!("opening plan")
        };
        let id = id.0;
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "TAB_OPENED",
            tab = id,
            feed = %feed_id,
            symbol = %symbol,
            tabs = self.tabs.len() + 1,
            action = "activate_new_tab",
            "opening a market in a new tab"
        );
        let spec = spec.unwrap_or_else(|| {
            self.config
                .startup_spec_for(&feed_id)
                .unwrap_or_else(|| *self.active_tab().flow_pane.state.spec())
        });
        let trades_dir = self.workspace.trades_dir().to_path_buf();
        // Cmd trading is app-wide (the trades-dir rule): a new tab starts
        // with the settings every other tab already carries.
        let cmd_trading = self.active_tab().paper.account().cmd_trading();
        let inherited_strategies = self
            .active_tab()
            .paper
            .account()
            .order_strategies()
            .to_vec();
        let inherited_selection = self
            .active_tab()
            .paper
            .account()
            .selected_order_strategy()
            .map(|strategy| strategy.name.clone());
        // Orientation travels with the working state the new tab inherits —
        // a market opened to compare against the active one is only
        // comparable the same way up. Per pane; a pane the source tab does
        // not have follows its flow chart.
        // The layers the active tab is *actually showing*, read before the new
        // tab is pushed. This used to be `self.layer_defaults` — the map read
        // off the file at startup — which was only harmless while that map was
        // whatever partial thing the trader's file happened to hold. Now that a
        // file's silence resolves to the shipped answer (`chart_layers::load`),
        // that map speaks for every layer, and applying it here would undo the
        // switches of the session mid-flight. Reading the live state is also
        // what the comment below has always promised.
        let inherited_risk = self.active_tab().paper.account().risk_settings().clone();
        let inherited_capital = self.active_tab().paper.account().capital().clone();
        let inherited_money = self.active_tab().paper.account().instrument_money().clone();
        let inherited_layers = self.active_tab().flow_pane.layer_states(self.style);
        let flow_inverted = self.active_tab().flow_pane.price_view.is_inverted();
        let time_inverted = self
            .active_tab()
            .time_pane()
            .map_or(flow_inverted, |pane| pane.price_view.is_inverted());
        // The layout the trader is looking at is the one the new chart
        // opens on — read before the new tab takes the focus.
        let inherited_layout = (!self.tabs.is_empty()).then(|| {
            self.workspace
                .layouts()
                .session()
                .resolve_layout(self.active_tab().focused_pane().layout_id())
        });
        let flow_pane_id = self.pane_ids.alloc();
        let mut tab = Tab::new(flow_pane_id, feed_id, symbol, spec, feed, trades_dir);
        tab.paper.set_cmd_trading(cmd_trading);
        tab.paper
            .account_mut()
            .set_order_strategies(inherited_strategies, inherited_selection.as_deref());
        // The risk per trade travels with them. It is app-wide like the rest
        // of the ticket's settings, and a tab that opened without it would
        // hand the trader a bare quantity field on a market they meant to
        // size the same way as the one beside it.
        tab.paper.account_mut().set_risk_settings(inherited_risk);
        tab.paper.account_mut().set_capital(inherited_capital);
        tab.paper
            .account_mut()
            .set_instrument_money(inherited_money);
        tab.flow_pane.request_opening_layout(inherited_layout);
        self.tabs.append(opening, tab);
        let config = self.config.clone();
        self.active_tab_mut().refresh_chip_label(&config);
        self.active_tab_mut().ensure_book_capture(&config);
        self.active_tab_mut().apply_feed_bubble_preset(&config);
        self.active_tab_mut().apply_feed_declared_layout(&config);
        // The new tab opens on the layers the user left showing, over the
        // preset it just put on: opening a second market is not a request to
        // bring back the chrome they switched off.
        self.active_tab_mut()
            .flow_pane
            .apply_layer_states(&inherited_layers);
        // The scripted footprint/zoom hooks reach tabs opened later too: the
        // replay tab a validation run autostarts is the tab the run means,
        // and it does not exist yet when the boot hooks fire.
        #[cfg(any(feature = "scenario-harness", test))]
        {
            if self.harness.footprint() {
                self.active_tab_mut().flow_pane.footprint.visible = true;
            }
            if let Some(px) = self.harness.candle_width() {
                self.active_tab_mut().flow_pane.viewport.set_px_per_bar(px);
            }
        }
        // After the declared layout ran: that is what decides whether the
        // new tab has a time pane to orient at all.
        let tab = self.active_tab_mut();
        tab.flow_pane.price_view.set_inverted(flow_inverted);
        for time_pane in tab.time_panes.iter_mut() {
            time_pane.price_view.set_inverted(time_inverted);
        }
    }
    /// Close the tab at `index`, activating a neighbour.
    ///
    /// The last tab stays: a window with no market has nothing to draw. What
    /// the closed tab owned goes with it — dropping its `FeedHandle` closes
    /// the receivers its feed thread sends into, and dropping its panes drops
    /// the indicator worker and book worker handles, whose run loops end when
    /// their command channels disconnect. No joins, no shutdown protocol.
    pub(super) fn close_tab(&mut self, index: usize) {
        self.tabs.close(
            index,
            self.indicators,
            self.workspace.layouts_mut().session_mut(),
        );
    }
    /// Move tabs along the strip with the core's existing positional wrap.
    pub(super) fn cycle_tab(&mut self, delta: isize) {
        self.tabs.cycle(delta);
    }
    /// Put the chrome back — the mirror of the [`Self::capture_arrangement`]
    /// half that produced it.
    ///
    /// One function because there are two callers and no way for the compiler
    /// to notice when only one of them learns a new field: the startup
    /// workspace and a named bookmark describe the same thing, and
    /// [`ui_state::NamedArrangement`] says so in as many words. Restoring them
    /// through two copies of the same eight lines is how a field comes to
    /// persist but never come back from a bookmark — a bug with no compile
    /// error behind it.
    ///
    /// The starred tools are the one thing it does not speak for: they live at
    /// the file's own level rather than inside an arrangement
    /// ([`ui_state::Workspace::favorite_tools`]), precisely so that opening a
    /// bookmark cannot rearrange the rail the trader curated.
    pub(super) fn restore_chrome(&mut self, chrome: &ui_state::SavedChrome) {
        *self.tz = TzOffset::new(chrome.timezone_minutes);
        self.dock
            .restore(chrome.dock_visible, chrome.dock_tab.map(Into::into));
        self.toolrail.set_dock(chrome.rail_dock.into());
        self.toolrail.set_visible(chrome.rail_visible);
        *self.show_perf = chrome.perf_readings;
        *self.record_deals = chrome.record_deals;
        self.history.progressive_history = chrome.progressive_history;
        // A token this release does not know keeps the reach it had — the
        // default on startup, whatever the trader picked when a bookmark is
        // opened mid-session. Never a silent fallback to something else: the
        // reach decides how much a press fetches.
        if let Some(reach) = chrome
            .history_reach
            .as_deref()
            .and_then(history_reach::HistoryReach::from_token)
        {
            self.history.history_reach = reach;
        }
        // Through the setter, so a hand-edited workspace cannot restore a span
        // the campaign could never reach.
        if let Some(minutes) = chrome.history_reach_span_minutes {
            self.history.set_span_minutes(minutes);
        }
        self.history.venue_lead_in = chrome.venue_lead_in;
        self.drawing_chrome
            .restore_inspector_position(chrome.inspector_position);
    }
    /// Put the window back the way the bookmark called `name` recorded it.
    ///
    /// The saved markets are opened as new tabs and the tabs that were on
    /// screen are closed afterwards, rather than the reverse: `close_tab`
    /// refuses to close the last tab — a window with no market has nothing to
    /// draw — so growing before shrinking is what lets the whole strip be
    /// replaced. Closing goes through the same path a `Ctrl+W` takes, so a
    /// simulated position ends in the labeled, journaled flatten the
    /// paper-trading contract promises instead of vanishing with its tab.
    ///
    /// The startup workspace is left alone. Opening a bookmark is a thing you
    /// do to *this session*; making it the opening screen is `Save workspace`,
    /// one entry above.
    pub(super) fn open_named_workspace(&mut self, name: &str) {
        let Some(entry) = self
            .workspace
            .session()
            .bookmarks()
            .iter()
            .find(|held| held.name == name)
            .cloned()
        else {
            self.note_workspace(format!("No workspace called \"{name}\""));
            return;
        };
        if entry.tabs.is_empty() {
            // `restore` drops empty bookmarks at load, so this is only
            // reachable from a file edited under a running app.
            self.note_workspace(format!("\"{name}\" has no market left to open"));
            return;
        }
        let replaced = self.tabs.len();
        self.run_restore(
            quantick_workspace::arrangement::RestoreMode::Named,
            &entry.tabs,
            entry.active_tab,
            entry.chrome.as_ref(),
            None,
        );
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "UI_STATE_NAMED_OPENED",
            name = %name,
            tabs = self.tabs.len(),
            closed = replaced,
            active = self.tabs.active_index(),
            action = "replace_tab_strip",
            "named workspace opened"
        );
        self.note_workspace(format!(
            "Opened \"{name}\" — {} {}",
            self.tabs.len(),
            if self.tabs.len() == 1 {
                "chart tab"
            } else {
                "chart tabs"
            }
        ));
    }

    fn active_tab(&self) -> &Tab {
        &self.tabs[self.tabs.active_index()]
    }
    pub(super) fn active_tab_mut(&mut self) -> &mut Tab {
        self.tabs.runtime_mut(self.tabs.active_index())
    }
    pub(super) fn note_workspace(&mut self, message: String) {
        self.toast.note(message, std::time::Instant::now());
    }
    pub(super) fn refresh_recent_workspaces(&mut self) {
        let existing = crate::workspace_bundle::existing_recent(self.workspace.session().recent());
        self.workspace.set_recent_on_disk(existing);
    }
}

impl ArrangementRead<'_> {
    /// The tabs and the chrome as they stand — the part a startup workspace
    /// and a named one describe identically, so both capture through here.
    pub(super) fn capture_arrangement(&self) -> (Vec<ui_state::SavedTab>, ui_state::SavedChrome) {
        let tabs = self
            .tabs
            .iter()
            .map(|tab| ui_state::SavedTab {
                feed: tab.feed_id.clone(),
                symbol: tab.symbol.clone(),
                layout: tab.layout.into(),
                split_fraction: Some(tab.split_fraction),
                context_collapsed: tab.context_collapsed,
                focus: Some(ui_state::SavedFocus::from_side(tab.focused_side()).0),
                focus_slot: ui_state::SavedFocus::from_side(tab.focused_side()).1,
                flow_bars: tab.flow_pane.state.spec().to_config_string(),
                // Only a pane that exists has an interval worth recording; a
                // tab that never showed the split restores on the default,
                // which is what it had.
                time_bars: tab
                    .time_pane()
                    .map(|pane| pane.state.spec().to_config_string()),
                context_bars: tab
                    .time_panes
                    .iter()
                    .map(|pane| pane.state.spec().to_config_string())
                    .collect(),
                flow_layout: tab.flow_pane.layout_id().map(|layout| layout.0),
                context_layouts: tab
                    .time_panes
                    .iter()
                    .map(|pane| {
                        pane.layout_id()
                            .map_or(crate::ui_state::LAYOUT_UNRECORDED, |layout| layout.0)
                    })
                    .collect(),
                flow_legend_collapsed: tab.flow_pane.legend_collapsed,
                // A tab with no time pane has no second legend, and `false`
                // is what it will restore into when one is opened: a pane
                // that never existed cannot have been folded.
                time_legend_collapsed: tab.time_pane().is_some_and(|pane| pane.legend_collapsed),
            })
            .collect();
        let chrome = ui_state::SavedChrome {
            timezone_minutes: self.tz.minutes(),
            dock_visible: self.dock.visible(),
            dock_tab: self.dock.tab().map(Into::into),
            rail_visible: self.toolrail.visible(),
            rail_dock: self.toolrail.dock().into(),
            perf_readings: self.show_perf,
            // Never written any more: the stars are a standing choice and live
            // at the top of the file. An arrangement that carried a copy would
            // be an arrangement that could overwrite them on open.
            legacy_favorite_tools: Vec::new(),
            record_deals: self.record_deals,
            progressive_history: self.history.progressive_history,
            // The default writes no key: a workspace that says nothing about
            // the reach restores the press the button has always had, which is
            // exactly what the default is.
            // Written whenever it differs from what the config seeds, so a
            // workspace only carries an opinion its owner actually formed.
            history_reach_span_minutes: (self.history.history_reach_span_minutes
                != self.config.history.reach_span_minutes)
                .then_some(self.history.history_reach_span_minutes),
            history_reach: (self.history.history_reach != history_reach::HistoryReach::default())
                .then(|| self.history.history_reach.token().to_owned()),
            venue_lead_in: self.history.venue_lead_in,
            inspector_position: self.drawing_chrome.remembered_inspector_position(),
        };
        (tabs, chrome)
    }
}
