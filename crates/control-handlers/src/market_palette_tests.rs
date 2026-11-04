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
                bar: "tick:50".to_owned(),
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
        book.open_tab(9);
        Self {
            snapshot,
            book,
            focused: Vec::new(),
            opened: Vec::new(),
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

    fn open_tab(
        &mut self,
        market: (&str, &str),
        bar: BarConfiguration,
        behind: bool,
    ) -> TabOpening {
        let (feed, symbol) = (market.0.to_owned(), market.1.to_owned());
        self.opened
            .push((feed, symbol, bar.to_config_string(), behind));
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
    assert_eq!(
        host.book.tab_state(9).map(|state| state.workspace),
        Some("Swing")
    );
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
