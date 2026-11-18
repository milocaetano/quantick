use super::*;
use crate::contract_roll::AutoRoll;
use crate::market_tab::FeedSelection;
use crate::market_tab::RecoveredFeed;
use crate::market_tab::{LentTab, MarketTab, MarketTabHost};
use crate::test_support::{FakeAccess, FakeWindow};
use quantick_session::{HistoryAct, HistoryOutcome, SessionSnapshot};
use quantick_stores::config::AppConfig;
use quantick_stores::market_list::{
    CatalogMarket, OpenMarket, SwitchChoice, Venue, VenueKind, roll_toast, switch_toast,
};
use quantick_stores::workspaces::WorkspaceBook;

/// Binance with BTCUSDT from the config and PEPEUSDT the trader added; tab 5
/// shows BTCUSDT at tick 50 on Default.
struct Host {
    snapshot: MarketSnapshot,
    book: WorkspaceBook,
    focused: Vec<usize>,
    opened: Vec<(String, String, String, bool)>,
    /// The workspace each opened tab is to join, by tab id.
    arrivals: Vec<(u64, Option<String>)>,
    catalog: Vec<(String, bool)>,
    palette: Option<(String, bool)>,
    /// Tab 5: what it shows, and every switch and paper state asked of it.
    tab: PaletteTab,
    /// The feed offers what the palette lists; the config rolls BTCUSDT on
    /// to PEPEUSDT.
    config: quantick_stores::config::AppConfig,
    flow_bars: Vec<(u64, String)>,
    default: quantick_sources::history_reach::HistoryReach,
    /// Whether the window lends its workspaces; a host without them refuses.
    lends_workspaces: bool,
}

impl Host {
    fn new() -> Self {
        let market = |symbol: &str, user_added| CatalogMarket {
            feed_id: "binance".to_owned(),
            symbol: symbol.to_owned(),
            asset: symbol.to_owned(),
            aliases: Vec::new(),
            user_added,
        };
        let snapshot = MarketSnapshot {
            venues: vec![Venue {
                feed_id: "binance".to_owned(),
                name: "Binance".to_owned(),
                chip: 'B',
                kind: VenueKind::Live,
                bar: Some("tick:50".to_owned()),
                bridged: false,
            }],
            markets: vec![market("BTCUSDT", false), market("PEPEUSDT", true)],
            open: vec![OpenMarket {
                tab_id: 5,
                position: 1,
                feed_id: "binance".to_owned(),
                symbol: "BTCUSDT".to_owned(),
                asset: "BTCUSDT".to_owned(),
                workspace: "Default".to_owned(),
                bar: "tick:50".to_owned(),
                context_bars: Vec::new(),
                replay: false,
                streaming: true,
                price: None,
                trades_per_s: None,
            }],
            workspaces: vec!["Default".to_owned(), "Swing".to_owned()],
            ..MarketSnapshot::default()
        };
        let mut book = WorkspaceBook::default();
        book.open_tab(1);
        book.save_as(1, "Swing").unwrap();
        // Tab 9 is the one an open makes: the book does not hold it yet, as
        // the window's book adopts a tab only once its panes stand.
        Self {
            snapshot,
            book,
            focused: Vec::new(),
            opened: Vec::new(),
            arrivals: Vec::new(),
            catalog: Vec::new(),
            palette: None,
            tab: PaletteTab {
                market: ("binance".to_owned(), "BTCUSDT".to_owned()),
                switches: Vec::new(),
                paper_open: false,
            },
            config: crate::test_support::fake_config(&[(
                "binance",
                &["BTCUSDT", "PEPEUSDT"],
                &[("BTCUSDT", "PEPEUSDT")],
            )]),
            flow_bars: Vec::new(),
            default: crate::test_support::FAKE_DEFAULT_REACH,
            lends_workspaces: true,
        }
    }
}

/// The host's book, lent through [`crate::workspace_book::WorkspaceBookHost`] on its
/// one market tab.
struct LentBook<'a>(&'a mut WorkspaceBook);

impl crate::workspace_book::WorkspaceOwner for LentBook<'_> {
    fn workspace_book(&mut self) -> Option<&mut WorkspaceBook> {
        Some(self.0)
    }
    fn persist_workspace_book(&mut self) -> Result<(), String> {
        Ok(())
    }
}

impl crate::TabDirectory for LentBook<'_> {
    fn active_tab_index(&self) -> usize {
        0
    }
    fn tab_position(&self, tab_id: u64) -> Option<usize> {
        (tab_id == 5).then_some(0)
    }
    fn tab_id_at(&self, _index: usize) -> u64 {
        5
    }
}

impl MarketPaletteHost for Host {
    fn market_snapshot(&self) -> MarketSnapshot {
        self.snapshot.clone()
    }

    fn focus_tab(&mut self, index: usize) {
        self.focused.push(index);
    }

    fn open_tab(&mut self, choice: &OpenChoice, bar: Option<BarConfiguration>) -> TabOpening {
        let (feed, symbol) = (choice.feed_id.clone(), choice.symbol.clone());
        let bar = bar.map_or_else(|| INHERITED.to_owned(), |bar| bar.to_config_string());
        self.opened.push((feed, symbol, bar, choice.background));
        self.arrivals.push((9, choice.arrival()));
        Ok(9)
    }

    fn edit_catalog(&mut self, _feed: &str, symbol: &str, add: bool) -> Result<(), String> {
        if symbol == "NOPE" {
            return Err("Binance does not list NOPE".to_owned());
        }
        self.catalog.push((symbol.to_owned(), add));
        Ok(())
    }

    fn open_market_palette(
        &mut self,
        query: &str,
        target: Option<SwitchTarget>,
    ) -> Result<(), ControlError> {
        self.palette = Some((query.to_owned(), target.is_some()));
        Ok(())
    }

    fn set_flow_bar(&mut self, tab: u64, bar: BarConfiguration) -> Result<(), String> {
        self.flow_bars.push((tab, bar.to_config_string()));
        Ok(())
    }
}

/// Tab 5 is the only market tab.
impl crate::TabDirectory for Host {
    fn active_tab_index(&self) -> usize {
        0
    }
    fn tab_position(&self, tab_id: u64) -> Option<usize> {
        (tab_id == 5).then_some(0)
    }
    fn tab_id_at(&self, _index: usize) -> u64 {
        5
    }
}

impl crate::workspace_book::WorkspaceBookHost for Host {
    fn workspaces(&mut self) -> Option<Box<dyn crate::workspace_book::WorkspaceOwner + '_>> {
        let lent: Box<dyn crate::workspace_book::WorkspaceOwner + '_> =
            Box::new(LentBook(&mut self.book));
        self.lends_workspaces.then_some(lent)
    }
}

/// The market tab 5 shows, and every switch and paper state asked of it.
struct PaletteTab {
    market: (String, String),
    switches: Vec<(String, String)>,
    paper_open: bool,
}

impl MarketTab for PaletteTab {
    fn market(&self) -> FeedSelection {
        FeedSelection {
            feed_id: self.market.0.clone(),
            symbol: self.market.1.clone(),
            replay: false,
        }
    }
    fn switch_market(&mut self, _config: &AppConfig, feed_id: &str, symbol: &str) {
        self.market = (feed_id.to_owned(), symbol.to_owned());
        self.switches.push(self.market.clone());
    }
    fn paper_open(&self) -> bool {
        self.paper_open
    }
    fn unlisted(&self) -> Option<String> {
        None
    }
    fn roll_memory(&mut self) -> &mut AutoRoll {
        unreachable!("the palette never reads the roll memory")
    }
    fn recover(&mut self, _config: &AppConfig, _keep_timeline: bool) -> RecoveredFeed {
        unreachable!("the palette never recovers the feed")
    }
    fn pages_history(&self, _config: &AppConfig) -> bool {
        false
    }
    fn history(&mut self, _config: &AppConfig, _act: HistoryAct) -> HistoryOutcome {
        unreachable!("the palette never presses History")
    }
    fn session(&self) -> SessionSnapshot {
        unreachable!("the palette never presses History")
    }
}

impl MarketTabHost for Host {
    fn tab_count(&self) -> usize {
        1
    }
    fn config(&self) -> &AppConfig {
        &self.config
    }
    fn market_tab(&self, _index: usize) -> Option<&dyn MarketTab> {
        Some(&self.tab)
    }
    fn market_tab_mut(&mut self, _index: usize) -> Option<LentTab<'_>> {
        Some(LentTab {
            tab: &mut self.tab,
            config: &self.config,
            history_default: &mut self.default,
        })
    }
    fn drawing_gesture_on(&self, _feed_id: &str, _from: &str, _to: &str) -> bool {
        false
    }
    fn carry_market_drawings(&mut self, _feed_id: &str, _from: &str, _to: &str) -> usize {
        0
    }
}

/// What [`Host::open_tab`] records for an open that names no bar.
const INHERITED: &str = "inherited";

type Handler = fn(&mut Host, &mut FakeAccess, &ActorContext, &Value) -> Result<Value, ControlError>;

fn call(handler: Handler, host: &mut Host, value: Value) -> Result<Value, ControlError> {
    handler(
        host,
        &mut FakeAccess::with_budget(0),
        &FakeWindow::assistant(),
        &value,
    )
}

#[test]
fn the_same_market_bar_and_workspace_focuses_the_tab_and_opens_nothing() {
    let mut host = Host::new();
    let answer = call(
        open_call,
        &mut host,
        serde_json::json!({"feed": "binance", "symbol": "BTCUSDT"}),
    )
    .unwrap();
    assert_eq!(
        (answer["tab_id"].as_str(), answer["focused"].as_bool()),
        (Some("5"), Some(true))
    );
    assert_eq!(host.focused, [0]);
    assert!(host.opened.is_empty());
}

#[test]
fn another_bar_or_workspace_opens_a_second_tab_and_puts_it_on_the_workspace() {
    let mut host = Host::new();
    let answer = call(
        open_call,
        &mut host,
        serde_json::json!({"feed": "binance", "symbol": "BTCUSDT", "bar": "tick:0200",
            "workspace": "Swing", "background": true}),
    )
    .unwrap();
    assert_eq!(answer["focused"], false);
    assert_eq!(
        answer["bar"], "tick:200",
        "the bar answers as the config writes it"
    );
    assert_eq!(
        host.opened,
        [(
            "binance".to_owned(),
            "BTCUSDT".to_owned(),
            "tick:200".to_owned(),
            true
        )]
    );
    assert_eq!(answer["workspace"], "Swing");
    assert_eq!(
        host.arrivals,
        [(9, Some("Swing".to_owned()))],
        "the new tab joins Swing when the book adopts it"
    );
    assert!(
        host.book.tab_state(9).is_none(),
        "the open never sets a workspace on a tab the book does not hold yet"
    );
}

/// A workspace the open cannot take is refused before any tab exists: a
/// refused call opens nothing.
#[test]
fn a_refused_workspace_opens_no_tab() {
    let mut host = Host::new();
    let refused = call(
        open_call,
        &mut host,
        serde_json::json!({"feed": "binance", "symbol": "PEPEUSDT", "workspace": "Gone"}),
    );
    assert!(refused.is_err());
    assert!(host.opened.is_empty() && host.arrivals.is_empty());
}

#[test]
fn a_stranger_market_a_bad_bar_and_an_unknown_workspace_open_nothing() {
    let mut host = Host::new();
    for value in [
        serde_json::json!({"feed": "binance", "symbol": "SOLUSDT"}),
        serde_json::json!({"feed": "binance", "symbol": "BTCUSDT", "bar": "tick"}),
        serde_json::json!({"feed": "binance", "symbol": "BTCUSDT", "workspace": "Nope"}),
        serde_json::json!({"feed": "binance"}),
    ] {
        assert!(
            call(open_call, &mut host, value.clone()).is_err(),
            "{value}"
        );
    }
    assert!(host.opened.is_empty() && host.focused.is_empty());
}

#[test]
fn removal_takes_only_what_the_trader_added() {
    let mut host = Host::new();
    let removed = call(
        remove_call,
        &mut host,
        serde_json::json!({"feed": "binance", "symbol": "PEPEUSDT"}),
    )
    .unwrap();
    assert_eq!(removed["symbol"], "PEPEUSDT");
    let refused = call(
        remove_call,
        &mut host,
        serde_json::json!({"feed": "binance", "symbol": "BTCUSDT"}),
    );
    assert!(refused.unwrap_err().message.contains("config file"));
    assert_eq!(host.catalog, [("PEPEUSDT".to_owned(), false)]);
}

#[test]
fn the_palette_opens_with_its_query() {
    let mut host = Host::new();
    let mut access = FakeAccess::with_budget(0);
    let value = serde_json::json!({"query": "> "});
    let answer = palette_call(&mut host, &mut access, &FakeWindow::assistant(), &value).unwrap();
    assert_eq!(answer["query"], "> ");
    assert_eq!(answer["switch"], false);
    assert_eq!(host.palette, Some(("> ".to_owned(), false)));
    assert_eq!(access.events.len(), 1, "the palette coming up is journaled");
    let value = serde_json::json!({"switch": true});
    let answer = palette_call(&mut host, &mut access, &FakeWindow::assistant(), &value).unwrap();
    assert_eq!(answer["switch"], true);
    assert_eq!(host.palette, Some((String::new(), true)), "switch mode");
    assert_eq!(access.events[1].payload["switch"], true);
}

#[test]
fn switch_mode_on_a_recording_is_refused_before_the_palette_shows() {
    let mut host = Host::new();
    host.snapshot.open[0].replay = true;
    let mut access = FakeAccess::with_budget(0);
    let value = serde_json::json!({"switch": true});
    let refused = palette_call(&mut host, &mut access, &FakeWindow::assistant(), &value);
    let refused = refused.unwrap_err().message;
    assert_eq!(refused, "A recorded session has no market to switch");
    assert_eq!(host.palette, None);
    assert!(access.events.is_empty(), "nothing came up to journal");
    show(&mut host, "", false).unwrap();
    assert_eq!(
        host.palette,
        Some((String::new(), false)),
        "plain mode still opens"
    );
}

#[test]
fn the_palette_settles_an_add_by_adding_then_opening_and_a_refusal_adds_nothing() {
    let mut host = Host::new();
    let choice = |symbol: &str| OpenChoice {
        feed_id: "binance".to_owned(),
        symbol: symbol.to_owned(),
        bar: "tick:50".to_owned(),
        context_bars: Vec::new(),
        inherit_bar: false,
        workspace: "Default".to_owned(),
        background: false,
    };
    assert_eq!(
        settle(&mut host, PaletteIntent::Add(choice("NOPE"))),
        Err("Binance does not list NOPE".to_owned())
    );
    assert!(host.catalog.is_empty() && host.opened.is_empty());
    let toast = settle(&mut host, PaletteIntent::Open(choice("BTCUSDT"))).unwrap();
    assert_eq!(toast.as_deref(), Some("Focused BTCUSDT \u{b7} tab 1"));
    let removal = PaletteIntent::Remove {
        feed_id: "binance".to_owned(),
        symbol: "PEPEUSDT".to_owned(),
    };
    assert_eq!(
        settle(&mut host, removal),
        Ok(None),
        "a removal keeps the palette open"
    );
}

/// An add the open would refuse adds nothing: the bar and the workspace are
/// checked before the symbol reaches the catalog.
#[test]
fn an_add_with_a_bad_bar_or_workspace_adds_nothing() {
    let mut host = Host::new();
    let choice = OpenChoice {
        feed_id: "binance".to_owned(),
        symbol: "SOLUSDT".to_owned(),
        bar: "tick".to_owned(),
        context_bars: Vec::new(),
        inherit_bar: false,
        workspace: "Default".to_owned(),
        background: false,
    };
    let refused = settle(&mut host, PaletteIntent::Add(choice.clone()));
    assert!(refused.unwrap_err().starts_with("Bars tick"));
    let elsewhere = OpenChoice {
        bar: "tick:50".to_owned(),
        context_bars: Vec::new(),
        workspace: "Nope".to_owned(),
        ..choice
    };
    assert_eq!(
        settle(&mut host, PaletteIntent::Add(elsewhere)),
        Err("No workspace is named Nope".to_owned())
    );
    assert!(host.catalog.is_empty() && host.opened.is_empty());
}

/// A replay row's Enter focuses its tab by id and opens nothing; a tab gone
/// since is refused.
#[test]
fn a_focus_brings_the_tab_forward_and_opens_nothing() {
    let mut host = Host::new();
    let focus = |tab_id| PaletteIntent::Focus {
        tab_id,
        symbol: "BTCUSDT".to_owned(),
    };
    assert_eq!(
        settle(&mut host, focus(5)),
        Ok(Some("Focused BTCUSDT \u{b7} tab 1".to_owned()))
    );
    assert_eq!(host.focused, [0]);
    assert!(settle(&mut host, focus(77)).is_err());
    assert!(host.opened.is_empty());
}

/// A market with no bar of its own opens naming none, so the window's rule
/// for a new tab decides, and the answer reports the bar that rule gives.
#[test]
fn a_market_without_a_bar_opens_on_the_inherited_one() {
    let mut host = Host::new();
    host.snapshot.venues[0].bar = None;
    host.snapshot.inherited_bar = Some("volume:7".to_owned());
    let answer = call(
        open_call,
        &mut host,
        serde_json::json!({"feed": "binance", "symbol": "PEPEUSDT"}),
    )
    .unwrap();
    assert_eq!(answer["bar"], "volume:7");
    assert_eq!(host.opened[0].2, INHERITED);
}

/// Picking a kind in the footer opens it on its registry default.
#[test]
fn a_picked_bar_kind_opens_on_its_registry_default() {
    for kind in ["tick", "volume", "dollar", "imbalance", "time"] {
        let size = default_bar_size(kind);
        assert!(
            BUILTIN_BARS.parse(&format!("{kind}:{size}")).is_ok(),
            "{kind}:{size}"
        );
    }
    assert_eq!(default_bar_size("no-such-kind"), "");
}

fn switch_to(symbol: &str) -> SwitchChoice {
    SwitchChoice {
        tab_id: 5,
        feed_id: "binance".to_owned(),
        symbol: symbol.to_owned(),
        bar: None,
        workspace: None,
        add: false,
    }
}

/// A switch goes through the source switch; the bar and workspace move only
/// when the trader changed them, and a refusal leaves the tab where it was.
#[test]
fn the_palette_switches_the_tab_through_the_source_switch() {
    let mut host = Host::new();
    host.book.open_tab(5);
    let toast = settle(&mut host, PaletteIntent::Switch(switch_to("PEPEUSDT"))).unwrap();
    assert_eq!(toast.as_deref(), Some(switch_toast("PEPEUSDT").as_str()));
    assert_eq!(
        host.tab.switches,
        [("binance".to_owned(), "PEPEUSDT".to_owned())]
    );
    assert!(host.flow_bars.is_empty(), "the tab keeps its bar");
    assert_eq!(
        host.book.tab_state(5).unwrap().workspace,
        "Default",
        "and its workspace"
    );

    let mut changed = switch_to("BTCUSDT");
    (changed.bar, changed.workspace) = (Some("tick:500".to_owned()), Some("Swing".to_owned()));
    settle(&mut host, PaletteIntent::Switch(changed)).unwrap();
    assert_eq!(host.flow_bars, [(5, "tick:500".to_owned())]);
    assert_eq!(host.book.tab_state(5).unwrap().workspace, "Swing");

    for refused in [
        SwitchChoice {
            bar: Some("tick:nope".to_owned()),
            ..switch_to("PEPEUSDT")
        },
        SwitchChoice {
            workspace: Some("Nowhere".to_owned()),
            ..switch_to("PEPEUSDT")
        },
        switch_to("NOTLISTED"),
        SwitchChoice {
            tab_id: 77,
            ..switch_to("PEPEUSDT")
        },
    ] {
        assert!(settle(&mut host, PaletteIntent::Switch(refused)).is_err());
    }
    assert_eq!(host.tab.switches.len(), 2, "no refusal switched");
    assert!(host.catalog.is_empty());
}

/// A workspace picked in the palette never drops a tab's edits unasked: the
/// switch is refused before the tab, its bar or its workspace moves.
#[test]
fn a_workspace_pick_that_would_drop_edits_is_refused_before_the_switch() {
    let mut host = Host::new();
    host.book.set_autosave(false);
    host.book.open_tab(5);
    let edited = quantick_stores::workspaces::WorkspaceRecipe {
        footprint_style: Some("heatmap".to_owned()),
        ..Default::default()
    };
    host.book.observe(5, &edited).unwrap();
    let picked = SwitchChoice {
        workspace: Some("Swing".to_owned()),
        ..switch_to("PEPEUSDT")
    };
    let reason = settle(&mut host, PaletteIntent::Switch(picked)).unwrap_err();
    let expected = quantick_control_schema::window_menu::switch_keeps_edits("Default", "Swing");
    assert_eq!(reason, expected);
    assert!(host.tab.switches.is_empty(), "the tab stays where it was");
    assert_eq!(host.book.tab_state(5).unwrap().workspace, "Default");
    assert_eq!(host.book.tab_copy(5), Some(&edited), "the edit is kept");
}

/// A window that lends no workspaces refuses a workspace pick before the tab
/// moves, rather than switching the market and failing on the workspace.
#[test]
fn a_workspace_pick_without_workspaces_is_refused_before_the_switch() {
    let mut host = Host::new();
    host.book.open_tab(5);
    host.lends_workspaces = false;
    let picked = SwitchChoice {
        workspace: Some("Swing".to_owned()),
        ..switch_to("PEPEUSDT")
    };
    let reason = settle(&mut host, PaletteIntent::Switch(picked)).unwrap_err();
    assert_eq!(reason, crate::workspace_book::unavailable().message);
    assert!(host.tab.switches.is_empty(), "the tab stays where it was");
    assert!(host.catalog.is_empty());
    assert_eq!(host.book.tab_state(5).unwrap().workspace, "Default");
}

/// With autosave on, an edit still on its way is written through first, as
/// a row's switch does: then the switch drops nothing and goes ahead.
#[test]
fn a_workspace_pick_writes_a_pending_autosave_edit_first() {
    let mut host = Host::new();
    host.book.set_autosave(true);
    host.book.open_tab(5);
    let edited = quantick_stores::workspaces::WorkspaceRecipe {
        footprint_style: Some("heatmap".to_owned()),
        ..Default::default()
    };
    host.book.observe(5, &edited).unwrap();
    let picked = SwitchChoice {
        workspace: Some("Swing".to_owned()),
        ..switch_to("PEPEUSDT")
    };
    settle(&mut host, PaletteIntent::Switch(picked)).unwrap();
    let default = host.book.recipe("Default").unwrap();
    assert_eq!(
        default.footprint_style.as_deref(),
        Some("heatmap"),
        "written"
    );
    assert_eq!(host.book.tab_state(5).unwrap().workspace, "Swing");
}

/// A bar the pane would refuse once on the new market (a deal bar: the fresh
/// timeline holds no deal count) is refused before the tab moves, so the
/// switch is never half done.
#[test]
fn a_bar_the_new_market_refuses_is_refused_before_the_switch() {
    let mut host = Host::new();
    host.book.open_tab(5);
    let refused = SwitchChoice {
        bar: Some("trades:2000".to_owned()),
        workspace: Some("Swing".to_owned()),
        ..switch_to("PEPEUSDT")
    };
    let reason = settle(&mut host, PaletteIntent::Switch(refused)).unwrap_err();
    assert!(reason.starts_with("Bars trades:2000: "), "{reason}");
    assert!(host.tab.switches.is_empty(), "the tab stays where it was");
    assert!(host.flow_bars.is_empty());
    assert_eq!(host.book.tab_state(5).unwrap().workspace, "Default");
}

/// What the switch would refuse is asked before the add: a refused switch
/// lists nothing.
#[test]
fn a_refused_add_and_switch_adds_nothing() {
    let mut host = Host::new();
    for refused in [
        SwitchChoice {
            tab_id: 77,
            ..switch_to("SOLUSDT")
        },
        SwitchChoice {
            feed_id: "nowhere".to_owned(),
            ..switch_to("SOLUSDT")
        },
        SwitchChoice {
            bar: Some("trades:2000".to_owned()),
            ..switch_to("SOLUSDT")
        },
        SwitchChoice {
            workspace: Some("Nowhere".to_owned()),
            ..switch_to("SOLUSDT")
        },
    ] {
        let add = SwitchChoice {
            add: true,
            ..refused
        };
        assert!(settle(&mut host, PaletteIntent::Switch(add)).is_err());
    }
    assert!(host.catalog.is_empty(), "nothing was added");
    assert!(host.tab.switches.is_empty());
}

#[test]
fn an_add_and_switch_adds_first_and_a_refused_add_switches_nothing() {
    let mut host = Host::new();
    let add = |symbol: &str| SwitchChoice {
        add: true,
        ..switch_to(symbol)
    };
    assert!(settle(&mut host, PaletteIntent::Switch(add("NOPE"))).is_err());
    assert!(host.tab.switches.is_empty());
    settle(&mut host, PaletteIntent::Switch(add("PEPEUSDT"))).unwrap();
    assert_eq!(host.catalog, [("PEPEUSDT".to_owned(), true)]);
    assert_eq!(host.tab.switches.len(), 1);
}

/// The roll row runs the roll's own rule and act: refused with a position
/// open or once the tab moved on, rolled when flat.
#[test]
fn the_palette_rolls_through_the_roll_rule() {
    let mut host = Host::new();
    let roll = || PaletteIntent::Roll {
        tab_id: 5,
        from_symbol: "BTCUSDT".to_owned(),
    };
    host.tab.paper_open = true;
    let refused = settle(&mut host, roll()).unwrap_err();
    assert!(refused.contains("paper position"), "{refused}");
    assert!(host.tab.switches.is_empty());
    host.tab.paper_open = false;
    let toast = settle(&mut host, roll()).unwrap();
    assert_eq!(toast.as_deref(), Some(roll_toast("PEPEUSDT").as_str()));
    assert_eq!(host.tab.market.1, "PEPEUSDT");
    let stale = settle(&mut host, roll()).unwrap_err();
    assert!(stale.contains("no longer shows BTCUSDT"), "{stale}");
}

/// A window that lends recording tabs and shows the palette when `lends`
/// says so.
struct Lender {
    lends: bool,
    moves: Vec<String>,
    palette: Option<(String, bool)>,
}

struct Tabs<'a>(&'a mut Vec<String>);

impl MarketTabs for Tabs<'_> {
    fn focus_tab(&mut self, index: usize) {
        self.0.push(format!("focus {index}"));
    }
    fn open_tab(&mut self, choice: &OpenChoice, bar: Option<BarConfiguration>) -> TabOpening {
        let bar = bar.map(|bar| bar.to_config_string());
        self.0.push(format!("open {} {bar:?}", choice.symbol));
        Ok(9)
    }
    fn set_flow_bar(&mut self, tab: u64, bar: BarConfiguration) -> Result<(), String> {
        self.0.push(format!("bar {tab} {}", bar.to_config_string()));
        Ok(())
    }
}

impl MarketPaletteOwners for Lender {
    fn listed_markets(&self) -> MarketSnapshot {
        MarketSnapshot {
            starter_feed: "binance".to_owned(),
            ..MarketSnapshot::default()
        }
    }
    fn market_tabs(&mut self) -> Option<Box<dyn MarketTabs + '_>> {
        let tabs: Box<dyn MarketTabs + '_> = Box::new(Tabs(&mut self.moves));
        self.lends.then_some(tabs)
    }
    fn show_market_palette(&mut self, query: &str, target: Option<SwitchTarget>) -> PaletteShowing {
        self.palette = Some((query.to_owned(), target.is_some()));
        match self.lends {
            true => Ok(()),
            false => Err(ControlError::invalid_request(
                "markets cannot be opened here",
            )),
        }
    }
}

fn lent_choice(symbol: &str) -> OpenChoice {
    OpenChoice {
        feed_id: "binance".to_owned(),
        symbol: symbol.to_owned(),
        bar: String::new(),
        context_bars: Vec::new(),
        inherit_bar: true,
        workspace: "Default".to_owned(),
        background: false,
    }
}

#[test]
fn the_primitives_reach_the_lent_owners() {
    let mut window = Lender {
        lends: true,
        moves: Vec::new(),
        palette: None,
    };
    assert_eq!(window.market_snapshot().starter_feed, "binance");
    window.focus_tab(2);
    assert_eq!(window.open_tab(&lent_choice("ETHUSDT"), None), Ok(9));
    let bar = BUILTIN_BARS.parse("tick:50").unwrap();
    window.set_flow_bar(5, bar).unwrap();
    assert_eq!(
        window.moves,
        ["focus 2", "open ETHUSDT None", "bar 5 tick:50"]
    );
    window.open_market_palette("> ", None).unwrap();
    assert_eq!(window.palette, Some(("> ".to_owned(), false)));
}

#[test]
fn a_window_that_lends_nothing_refuses_each_primitive() {
    let mut window = Lender {
        lends: false,
        moves: Vec::new(),
        palette: None,
    };
    let unavailable = "markets cannot be opened here";
    let open = window.open_tab(&lent_choice("X"), None);
    assert_eq!(open.unwrap_err(), unavailable);
    let edit = window.edit_catalog("binance", "X", true);
    assert_eq!(edit.unwrap_err(), unavailable);
    let bar = BUILTIN_BARS.parse("tick:50").unwrap();
    assert_eq!(window.set_flow_bar(5, bar).unwrap_err(), unavailable);
    let palette = window.open_market_palette("", None).unwrap_err();
    assert_eq!(palette.message, unavailable);
    window.focus_tab(0);
    assert!(window.moves.is_empty());
}
