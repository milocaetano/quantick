use super::*;
use crate::test_support::{FakeAccess, FakeWindow};
use quantick_stores::market_list::{CatalogMarket, OpenMarket, Venue, VenueKind};
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
    palette: Option<String>,
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
        }
    }
}

impl WorkspaceBookHost for Host {
    fn workspace_book(&mut self) -> Option<&mut WorkspaceBook> {
        Some(&mut self.book)
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

    fn open_market_palette(&mut self, query: &str) -> Result<(), ControlError> {
        self.palette = Some(query.to_owned());
        Ok(())
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
    assert_eq!(host.palette.as_deref(), Some("> "));
    assert_eq!(access.events.len(), 1, "the palette coming up is journaled");
}

#[test]
fn the_palette_settles_an_add_by_adding_then_opening_and_a_refusal_adds_nothing() {
    let mut host = Host::new();
    let choice = |symbol: &str| OpenChoice {
        feed_id: "binance".to_owned(),
        symbol: symbol.to_owned(),
        bar: "tick:50".to_owned(),
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
        inherit_bar: false,
        workspace: "Default".to_owned(),
        background: false,
    };
    let refused = settle(&mut host, PaletteIntent::Add(choice.clone()));
    assert!(refused.unwrap_err().starts_with("Bars tick"));
    let elsewhere = OpenChoice {
        bar: "tick:50".to_owned(),
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
