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
use quantick_engine::bar_registry::BarConfiguration as Bars;
use quantick_session::{close_queue::CloseScope, workspace_menu::WorkspaceCommand};
use quantick_workspace::market_list as list;

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

    /// Tab shortcuts (§10): `Ctrl+T` new, `Ctrl+W` close, `Ctrl+Tab` cycle; none under the palette.
    pub(super) fn handle_tab_keys(&mut self, ctx: &egui::Context) {
        self.read_view_keys(ctx);
        let mut keys = super::home_wiring::read_tab_keys(ctx);
        if !self.surfaces.market_palette.is_open() {
            if std::mem::take(&mut keys.close_tab) && !self.chrome.title_bar.surface.is_home() {
                self.apply_tab_action(TabAction::Close(self.runtime.tabs.active_index()));
            }
            let asked = self.arrangement_adapter().apply_tab_keys(keys);
            settle_strip(self, asked);
        }
    }

    /// Carry out what the tab strip asked for; a close asks first while the
    /// tab holds unsaved workspace edits.
    pub(super) fn apply_tab_action(&mut self, action: TabAction) {
        if let TabAction::Close(index) = action {
            let tab = (index < self.runtime.tabs.len()).then(|| self.runtime.tabs.id_at(index));
            let close = WorkspaceCommand::Close(CloseScope::Tabs, Vec::from_iter(tab));
            return super::tab_workspaces::apply_workspace_command(self, close);
        }
        let asked = self.arrangement_adapter().apply_tab_action(action);
        settle_strip(self, asked);
    }
}

/// Close the tab at `index` without asking; answers whether it went.
pub(super) fn close_now(app: &mut QuantickApp, index: usize) -> bool {
    let asked = app
        .arrangement_adapter()
        .apply_tab_action(TabAction::Close(index));
    let closed = asked.1.is_some();
    settle_strip(app, asked);
    closed
}

#[cfg(test)]
impl QuantickApp {
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
    /// A named reading rather than an expression inside the header's own
    /// frame, so the rule can be asserted without painting a header. What
    /// reads it back without looking at the screen is the semantic scene,
    /// which takes the same reading this delegates to.
    #[must_use]
    pub(super) fn heatmap_lamp_on(&self) -> bool {
        // `#[cfg(test)]` because the header takes the pane's reading directly:
        // keeping a second production entry point to the same answer is how the
        // two drift.
        self.active_tab()
            .flow_pane
            .layer_switched_on(ChartLayer::Heatmap, &self.runtime.style)
    }
}

/// A live market a close took goes to Recent with its bar; `+` opens the palette.
fn settle_strip(app: &mut QuantickApp, (palette, closed): super::arrangement_adapter::StripAsk) {
    if let Some(tab) = closed.filter(|tab| tab.replay.is_none()) {
        let now = crate::metrics::wall_clock_ms();
        let stamp = quantick_civil::fmt_offset_minute(now, app.runtime.tz);
        let closed = list::ClosedMarket {
            feed_id: tab.feed_id.clone(),
            symbol: tab.symbol.clone(),
            at: stamp.get(11..).unwrap_or_default().to_owned(),
            price: last_price(&tab),
            bar: tab.flow_pane.state.spec().to_config_string(),
            context_bars: super::arrangement_adapter::saved_tab_of(&tab).context_bars,
        };
        app.surfaces.market_palette.recent.closed(closed);
    }
    if palette {
        app.surfaces.market_palette.open("", None);
    }
}

/// What "Open a market" lists: the config's markets and this window's tabs.
pub(super) fn market_snapshot(app: &QuantickApp) -> list::MarketSnapshot {
    let (venues, markets) = app.config.market_catalog(&app.added_symbols);
    let (tabs, book) = (&app.runtime.tabs, &app.runtime.workspace.book);
    let open = (tabs.iter().enumerate())
        .map(|(index, tab)| list::OpenMarket {
            tab_id: tabs.id_at(index),
            position: index + 1,
            feed_id: tab.feed_id.clone(),
            symbol: tab.symbol.clone(),
            asset: app.config.market_asset(&tab.feed_id, &tab.symbol),
            workspace: book.tab_row(tabs.id_at(index)).0,
            bar: tab.flow_pane.state.spec().to_config_string(),
            context_bars: super::arrangement_adapter::saved_tab_of(tab).context_bars,
            replay: tab.replay.is_some(),
            streaming: tab.latest_trade_ms.is_some(),
            price: last_price(tab),
            trades_per_s: None,
        })
        .collect();
    let workspaces = book.book().list().into_iter().map(|row| row.name);
    list::MarketSnapshot {
        venues,
        markets,
        open,
        recent: app.surfaces.market_palette.recent.clone(),
        starter_feed: app.config.default_feed.clone(),
        workspaces: workspaces.collect(),
        inherited_bar: Some(app.active_tab().flow_pane.state.spec().to_config_string()),
    }
}

/// Bring the tab at `index` forward, off Home.
pub(super) fn focus_tab(app: &mut QuantickApp, index: usize) {
    app.runtime.tabs.select(index);
    app.chrome.title_bar.surface.show_market();
}

/// Open `choice` on `bar` (none: `adopt_tab`'s), its workspace left as the tab's arrival.
pub(super) fn open(app: &mut QuantickApp, choice: &list::OpenChoice, bar: Option<Bars>) -> u64 {
    let shown = app.runtime.tabs.active_index();
    (app.arrangement_adapter()).open_tab(choice.feed_id.clone(), choice.symbol.clone(), bar);
    let timeframes = super::saved_context_intervals(&choice.context_bars, None);
    app.active_tab_mut().open_context_on(&timeframes);
    let tab = app.runtime.tabs.active_id();
    let sync = app.runtime.workspace.book.sync_mut();
    sync.join(tab, choice.arrival());
    (choice.background).then(|| app.runtime.tabs.select(shown));
    tab
}

/// Add a symbol to a feed's list, or take one out.
pub(super) fn edit_catalog(app: &mut QuantickApp, market: (&str, &str), add: bool) -> Refusable {
    match add {
        true => app.symbol_catalog().add(market.0, market.1),
        false => app.symbol_catalog().remove(market.0, market.1),
    }
}

/// Show the palette; `switch` puts it in switch mode on the active tab.
pub(super) fn open_palette(app: &mut QuantickApp, query: &str, switch: bool) -> Refusable {
    let port = app as &dyn super::control_host::ControlPort;
    let target = switch.then(|| quantick_control_handlers::market_switch::target(port));
    app.surfaces.market_palette.open(query, target.transpose()?);
    Ok(())
}

/// Do what the palette asked. A refusal stays under its field.
pub(super) fn apply_palette_intent(app: &mut QuantickApp, intent: list::PaletteIntent) {
    let port = app as &mut dyn super::control_host::ControlPort;
    match quantick_control_handlers::market_palette::settle(port, intent) {
        Ok(None) => {}
        Ok(Some(note)) => {
            app.surfaces.market_palette.close();
            app.surfaces.toast.note(note, std::time::Instant::now());
        }
        Err(reason) => app.surfaces.market_palette.refuse(reason),
    }
}

type Refusable = Result<(), String>;

fn last_price(tab: &crate::tab::Tab) -> Option<String> {
    let bar = tab.flow_pane.state.bars().last()?;
    Some(bar.close.to_string())
}
