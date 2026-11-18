//! Open a market, show its palette and take an added market back out. The
//! palette's Enter and Delete and the three calls go through [`open`],
//! [`remove`] and [`settle`] here, over the headless market list, so the two
//! doors cannot decide differently. The host lends primitives only.
use crate::dock::ActionDock;
use crate::notify::NotifyAccess;
use crate::workspace_book::WorkspaceBookHost;
use quantick_control::wire::WireU64;
use quantick_control::{error::ControlError, registry::RegistryError, wire::ActorContext};
use quantick_control_host::authority::MARKET_MODULE_ID;
use quantick_control_host::journal::NewEvent;
use quantick_control_schema::market_palette::PALETTE_OPENED_EVENT_KIND;
use quantick_control_schema::market_palette::{
    CatalogRemoveInput, CatalogRemoveResult, MarketOpenInput, MarketOpenResult,
    MarketPaletteOpenInput, MarketPaletteOpenResult, catalog_remove_descriptor,
    market_open_descriptor, market_palette_open_descriptor,
};
use quantick_engine::bar_registry::{BUILTIN_BARS, BarConfiguration};
use quantick_stores::market_list::{
    MarketSnapshot, OpenChoice, PaletteIntent, SwitchTarget, focus_target, focus_toast, open_toast,
    removal_refusal,
};
use quantick_stores::symbol_catalog::SymbolCatalog;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;

const UNAVAILABLE: &str = "markets cannot be opened here";

/// The new tab's id, or why it did not open.
pub type TabOpening = Result<u64, String>;

/// The palette on screen, or why it is not.
pub type PaletteShowing = Result<(), ControlError>;

/// What the window lends the palette: the owners each primitive acts on,
/// borrowed for one call. Each default lends nothing, which every call
/// answers as unavailable.
pub trait MarketPaletteOwners {
    /// What "Open a market" lists now: the config's markets and the open tabs.
    fn listed_markets(&self) -> MarketSnapshot {
        MarketSnapshot::default()
    }
    /// The open tabs, as an open, a focus or a bar change moves them.
    fn market_tabs(&mut self) -> Option<Box<dyn MarketTabs + '_>> {
        None
    }
    /// Each feed's symbol list, which an add or a removal edits.
    fn market_catalog(&mut self) -> Option<SymbolCatalog<'_>> {
        None
    }
    /// Show the palette on `query`, switching `target`'s tab when there is one.
    ///
    /// # Errors
    /// A window without the palette.
    fn show_market_palette(
        &mut self,
        _query: &str,
        _target: Option<SwitchTarget>,
    ) -> PaletteShowing {
        Err(ControlError::invalid_request(UNAVAILABLE))
    }
}

/// The open tabs as the palette moves them, lent for one call.
pub trait MarketTabs {
    /// Bring the tab at this zero-based place forward, off Home.
    fn focus_tab(&mut self, index: usize);
    /// Open a tab on the choice's market, on `bar` (none: the one any new
    /// tab takes), joining the choice's workspace ([`OpenChoice::arrival`]),
    /// and answer its id; a background choice keeps the tab on screen.
    fn open_tab(&mut self, choice: &OpenChoice, bar: Option<BarConfiguration>) -> TabOpening;
    /// Put tab `tab`'s flow pane on `bar`, as its bars menu does.
    fn set_flow_bar(&mut self, tab: u64, bar: BarConfiguration) -> Result<(), String>;
}

pub trait MarketPaletteHost {
    /// What "Open a market" lists now.
    fn market_snapshot(&self) -> MarketSnapshot {
        MarketSnapshot::default()
    }
    /// Bring the market tab at this zero-based place forward.
    fn focus_tab(&mut self, _index: usize) {}
    /// Open a tab on the choice's market, on `bar` (none: the one any new
    /// tab takes), joining the choice's workspace ([`OpenChoice::arrival`]),
    /// and answer its id; a background choice keeps the tab on screen.
    fn open_tab(&mut self, _choice: &OpenChoice, _bar: Option<BarConfiguration>) -> TabOpening {
        Err(UNAVAILABLE.to_owned())
    }
    /// Add a symbol to a feed's list, or with `add` false take it out.
    fn edit_catalog(&mut self, _feed: &str, _symbol: &str, _add: bool) -> Result<(), String> {
        Err(UNAVAILABLE.to_owned())
    }
    /// Show the palette; with a `target`, in switch mode on that tab.
    fn open_market_palette(
        &mut self,
        _query: &str,
        _target: Option<SwitchTarget>,
    ) -> Result<(), ControlError> {
        Err(ControlError::invalid_request(UNAVAILABLE))
    }
    /// Put tab `tab`'s flow pane on `bar`, as its bars menu does.
    fn set_flow_bar(&mut self, _tab: u64, _bar: BarConfiguration) -> Result<(), String> {
        Err(UNAVAILABLE.to_owned())
    }
}

/// The window's palette primitives are the owners it lends ([`MarketPaletteOwners`]).
impl<T: MarketPaletteOwners + ?Sized> MarketPaletteHost for T {
    fn market_snapshot(&self) -> MarketSnapshot {
        self.listed_markets()
    }
    fn focus_tab(&mut self, index: usize) {
        if let Some(mut tabs) = self.market_tabs() {
            tabs.focus_tab(index);
        }
    }
    fn open_tab(&mut self, choice: &OpenChoice, bar: Option<BarConfiguration>) -> TabOpening {
        let mut tabs = self.market_tabs().ok_or(UNAVAILABLE)?;
        tabs.open_tab(choice, bar)
    }
    fn edit_catalog(&mut self, feed: &str, symbol: &str, add: bool) -> Result<(), String> {
        let mut catalog = self.market_catalog().ok_or(UNAVAILABLE)?;
        match add {
            true => catalog.add(feed, symbol),
            false => catalog.remove(feed, symbol),
        }
    }
    fn open_market_palette(
        &mut self,
        query: &str,
        target: Option<SwitchTarget>,
    ) -> Result<(), ControlError> {
        self.show_market_palette(query, target)
    }
    fn set_flow_bar(&mut self, tab: u64, bar: BarConfiguration) -> Result<(), String> {
        let mut tabs = self.market_tabs().ok_or(UNAVAILABLE)?;
        tabs.set_flow_bar(tab, bar)
    }
}

/// Show the palette on `query`; with `switch`, in switch mode on the active
/// tab, which a recording refuses.
///
/// # Errors
/// The refusal, or a host without a palette.
pub fn show<H: crate::market_switch::SwitchHost + ?Sized>(
    app: &mut H,
    query: &str,
    switch: bool,
) -> Result<(), ControlError> {
    let target = switch.then(|| crate::market_switch::target(app));
    let target = target.transpose().map_err(ControlError::invalid_request)?;
    app.open_market_palette(query, target)
}

/// What an open did: the choice as checked, the tab, and its place when an
/// open tab was focused instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Opened {
    pub choice: OpenChoice,
    pub tab: u64,
    pub focused: Option<usize>,
}

/// Open `asked`, or focus the tab that already shows it on that bar and
/// workspace. Refuses a bar that does not parse, a market the config does
/// not offer and an unknown workspace before anything opens.
///
/// # Errors
/// The reason, in the trader's words.
pub fn open<H: MarketPaletteHost + WorkspaceBookHost + ?Sized>(
    app: &mut H,
    asked: &OpenChoice,
) -> Result<Opened, String> {
    let named = named_bar(asked)?;
    let snapshot = app.market_snapshot();
    let choice = snapshot.choice(
        (asked.feed_id.as_str(), asked.symbol.as_str()),
        named.map(|spec| spec.to_config_string()),
        Some(asked.workspace.clone()),
        asked.background,
    )?;
    let (feed, symbol) = (choice.feed_id.as_str(), choice.symbol.as_str());
    if let Some(tab) = focus_target(&snapshot.open, feed, symbol, &choice.bar, &choice.workspace) {
        app.focus_tab(tab.position - 1);
        let (tab, focused) = (tab.tab_id, Some(tab.position));
        return Ok(Opened {
            choice,
            tab,
            focused,
        });
    }
    let tab = app.open_tab(&choice, named_bar(&choice)?)?;
    Ok(Opened {
        choice,
        tab,
        focused: None,
    })
}

/// The bar `asked` names, parsed; `None` when it leaves the bar to the new
/// tab ([`OpenChoice::inherit_bar`]).
fn named_bar(asked: &OpenChoice) -> Result<Option<BarConfiguration>, String> {
    if asked.inherit_bar {
        return Ok(None);
    }
    let spec = BUILTIN_BARS.parse(&asked.bar);
    spec.map(Some)
        .map_err(|error| format!("Bars {}: {error}", asked.bar))
}

/// Bring tab `tab_id` forward, wherever it sits on the strip now.
fn focus<H: MarketPaletteHost + ?Sized>(
    app: &mut H,
    tab_id: u64,
    symbol: &str,
) -> Result<String, String> {
    let snapshot = app.market_snapshot();
    let tab = (snapshot.open.iter())
        .find(|tab| tab.tab_id == tab_id)
        .ok_or_else(|| format!("The tab showing {symbol} has closed"))?;
    app.focus_tab(tab.position - 1);
    Ok(focus_toast(symbol, tab.position))
}

/// Take a symbol the trader added out of its feed's list, unless the market
/// list refuses: a symbol the config declares, or one a tab is showing.
///
/// # Errors
/// The refusal.
pub fn remove<H: MarketPaletteHost + ?Sized>(
    app: &mut H,
    feed: &str,
    symbol: &str,
) -> Result<(), String> {
    if let Some(reason) = removal_refusal(&app.market_snapshot().world(), feed, symbol) {
        return Err(reason);
    }
    app.edit_catalog(feed, symbol, false)
}

/// Do what the palette asked: `Some(toast)` when it opened something and
/// closes, `None` when it stays open.
///
/// # Errors
/// The refusal the palette shows under its field.
pub fn settle<H: crate::market_switch::SwitchHost + ?Sized>(
    app: &mut H,
    intent: PaletteIntent,
) -> Result<Option<String>, String> {
    let choice = match intent {
        PaletteIntent::Open(choice) => choice,
        // Everything the open could refuse, before the catalog changes: a
        // refused add adds nothing.
        PaletteIntent::Add(choice) => {
            named_bar(&choice)?;
            app.market_snapshot().check_workspace(&choice.workspace)?;
            app.edit_catalog(&choice.feed_id, &choice.symbol, true)?;
            choice
        }
        PaletteIntent::Focus { tab_id, symbol } => return focus(app, tab_id, &symbol).map(Some),
        PaletteIntent::Remove { feed_id, symbol } => {
            return remove(app, &feed_id, &symbol).map(|()| None);
        }
        PaletteIntent::Switch(choice) => {
            return crate::market_switch::switch(app, &choice).map(Some);
        }
        PaletteIntent::Roll {
            tab_id,
            from_symbol,
        } => return crate::market_switch::roll(app, tab_id, &from_symbol).map(Some),
    };
    let opened = open(app, &choice)?;
    Ok(Some(open_toast(&opened.choice, opened.focused)))
}

/// The size a bar kind opens on when the trader picks it: its registry
/// default, as the config writes it; empty for a kind the registry lacks.
#[must_use]
pub fn default_bar_size(kind: &str) -> String {
    (BUILTIN_BARS.find(kind)).map_or_else(
        |_| String::new(),
        |bar| bar.parameter.format(bar.parameter.default),
    )
}

/// Outside the replay trace: a recorded call must not open tabs or edit the
/// venue lists when a session re-runs.
pub fn register<H: crate::market_switch::SwitchHost + ?Sized, A: NotifyAccess>(
    registry: &mut impl ActionDock<H, A>,
) -> Result<(), RegistryError> {
    registry.register_outside_replay(market_open_descriptor(), open_call)?;
    registry.register_outside_replay(market_palette_open_descriptor(), palette_call)?;
    registry.register_outside_replay(catalog_remove_descriptor(), remove_call)
}

fn input<T: DeserializeOwned>(value: &Value) -> Result<T, ControlError> {
    serde_json::from_value(value.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))
}

fn answer<T: Serialize>(result: &T) -> Result<Value, ControlError> {
    serde_json::to_value(result).map_err(|error| ControlError::invalid_request(error.to_string()))
}

fn open_call<H: MarketPaletteHost + WorkspaceBookHost + ?Sized, A>(
    app: &mut H,
    _: &mut A,
    _: &ActorContext,
    value: &Value,
) -> Result<Value, ControlError> {
    let request: MarketOpenInput = input(value)?;
    let market = (request.feed.as_str(), request.symbol.as_str());
    let (bar, workspace) = (request.bar.clone(), request.workspace.clone());
    let asked = (app.market_snapshot())
        .choice(market, bar, workspace, request.background)
        .map_err(ControlError::invalid_request)?;
    let Opened {
        choice,
        tab,
        focused,
    } = open(app, &asked).map_err(ControlError::invalid_request)?;
    answer(&MarketOpenResult {
        tab_id: WireU64::new(tab),
        focused: focused.is_some(),
        feed: choice.feed_id,
        symbol: choice.symbol,
        bar: choice.bar,
        workspace: choice.workspace,
        background: choice.background,
    })
}

fn palette_call<H: crate::market_switch::SwitchHost + ?Sized, A: NotifyAccess>(
    app: &mut H,
    access: &mut A,
    actor: &ActorContext,
    value: &Value,
) -> Result<Value, ControlError> {
    let MarketPaletteOpenInput { query, switch } = input(value)?;
    show(app, &query, switch)?;
    let payload = serde_json::json!({ "query": query, "switch": switch });
    access.record_event(NewEvent::by(
        MARKET_MODULE_ID,
        PALETTE_OPENED_EVENT_KIND,
        actor,
        payload,
    ));
    answer(&MarketPaletteOpenResult { query, switch })
}

fn remove_call<H: MarketPaletteHost + ?Sized, A>(
    app: &mut H,
    _: &mut A,
    _: &ActorContext,
    value: &Value,
) -> Result<Value, ControlError> {
    let CatalogRemoveInput { feed, symbol } = input(value)?;
    remove(app, &feed, &symbol).map_err(ControlError::invalid_request)?;
    answer(&CatalogRemoveResult { feed, symbol })
}

#[cfg(test)]
#[path = "market_palette_tests.rs"]
mod market_palette_tests;
